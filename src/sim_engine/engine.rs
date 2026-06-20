use crate::CPU;
use crate::DSIM3_CFG_PATH;
use crate::DSIM3_OUT_DIR;
use crate::PE::pe_top::PE;
use crate::memory::dramsim3_wrapper::dramsim3_wrapper;
use crate::memory::mem_portal::{dram_portal, dram_req, portal_mode, portal_req};
use crate::sim_engine::request_router::{decode_pe_inst, is_pe_request};
use std::collections::VecDeque;
#[cfg(test)]
use std::sync::atomic::{AtomicU8, Ordering};

const BATCH_SZ: u64 = 0;

#[cfg(test)]
const SCHED_PROBE_INVOKED: u8 = 1 << 0;
#[cfg(test)]
const SCHED_PROBE_ENTERED_HOST: u8 = 1 << 1;
#[cfg(test)]
const SCHED_PROBE_ENTERED_PIM: u8 = 1 << 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EngineMode {
    PIM,
    HOST,
    switch_delay,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EngineSchedulingMode {
    Unconfigured,
    CGO_only,
    Host_CGO_share,
    Host_FGO_share,
    HostOnly,
}

enum EngineProcessor {
    CGO(CPU),
    FGO(PE),
}

#[derive(Clone, Copy)]
enum FgoRequestState {
    Idle,
    PimInFlight,
    HostInFlight,
}

// CGO scheduling is cycle/batch based because the CPU runs autonomously. FGO scheduling is
// request based: the engine admits one PE instruction or one host request, waits for completion,
// and then gives the other source priority.

pub struct Engine {
    processor: EngineProcessor,
    host_pool: VecDeque<portal_req>,
    host_complete_queue: VecDeque<dram_req>,
    dram_port: dram_portal,
    dsim3: dramsim3_wrapper,
    scheduling_mode: EngineSchedulingMode,
    clock_cycle: u64,
    //Following are batched scheduler internal variables
    mode: EngineMode,
    next_mode: EngineMode,
    last_service_mode: EngineMode,
    PIM_tick_watermark: u64,
    PIM_tick_rec: u64,
    MEM_req_watermarkL: u64,
    MEM_tick_rec: u64,
    first_host_switch_started: bool,
    //Following are F3FS scheduler internal variables
    fgo_request_state: FgoRequestState,
    fgo_next_service: EngineMode,
    switch_delay_remaining: u64,
    switch_pause_cycles: u64,
    switch_resume_cycles: u64,
    // Test-only pin-out. This field and all writes to it are absent from production builds.
    #[cfg(test)]
    scheduler_probe: AtomicU8,
}

impl Engine {
    pub fn new_cgo() -> Self {
        Self::build(true)
    }

    pub fn new_fgo() -> Self {
        Self::build(false)
    }

    fn build(is_cgo: bool) -> Self {
        let mut dram_port = dram_portal::new();
        dram_port.set_mode(portal_mode::PIM);

        let processor = if is_cgo {
            EngineProcessor::CGO(CPU::new_with_dram_port(dram_port.clone()))
        } else {
            EngineProcessor::FGO(PE::new_with_dram_port(dram_port.clone()))
        };

        let mut dsim3 = dramsim3_wrapper::new(DSIM3_CFG_PATH, DSIM3_OUT_DIR, 0, 0, 0, 0);
        dsim3.SetPimMode(true);

        Self {
            processor,
            dram_port,
            host_pool: VecDeque::new(),
            host_complete_queue: VecDeque::new(),
            dsim3,
            scheduling_mode: EngineSchedulingMode::Unconfigured,
            mode: EngineMode::PIM,
            next_mode: EngineMode::PIM,
            last_service_mode: EngineMode::PIM,
            PIM_tick_watermark: 0,
            PIM_tick_rec: 0,
            MEM_req_watermarkL: BATCH_SZ,
            MEM_tick_rec: 0,
            first_host_switch_started: false,
            clock_cycle: 0,
            fgo_request_state: FgoRequestState::Idle,
            fgo_next_service: EngineMode::PIM,
            switch_delay_remaining: 0,
            switch_pause_cycles: 0,
            switch_resume_cycles: 0,
            #[cfg(test)]
            scheduler_probe: AtomicU8::new(0),
        }
    }

    pub fn set_scheduling_mode(
        &mut self,
        scheduling_mode: EngineSchedulingMode,
    ) -> Result<(), &'static str> {
        if self.scheduling_mode != EngineSchedulingMode::Unconfigured {
            // NOTE: Support live scheduler reconfiguration by defining how active processor and
            // DRAM requests are drained and how transition timing is applied.
            return Err("engine scheduling mode can only be configured once");
        }
        if scheduling_mode == EngineSchedulingMode::Unconfigured {
            return Err("cannot configure an engine with Unconfigured scheduling mode");
        }

        let compatible = matches!(
            (&self.processor, scheduling_mode),
            (EngineProcessor::CGO(_), EngineSchedulingMode::CGO_only)
                | (
                    EngineProcessor::CGO(_),
                    EngineSchedulingMode::Host_CGO_share
                )
                | (EngineProcessor::CGO(_), EngineSchedulingMode::HostOnly)
                | (
                    EngineProcessor::FGO(_),
                    EngineSchedulingMode::Host_FGO_share
                )
                | (EngineProcessor::FGO(_), EngineSchedulingMode::HostOnly)
        );
        if !compatible {
            return Err("scheduling mode is incompatible with the engine processor");
        }

        self.scheduling_mode = scheduling_mode;
        if scheduling_mode == EngineSchedulingMode::HostOnly {
            self.force_host_mode();
            self.first_host_switch_started = true;
        } else {
            self.force_pim_mode();
        }
        Ok(())
    }

    pub fn get_cpu(&mut self) -> &mut CPU {
        match &mut self.processor {
            EngineProcessor::CGO(cpu) => cpu,
            EngineProcessor::FGO(_) => panic!("cannot access CPU on an FGO engine"),
        }
    }

    pub fn get_pe(&mut self) -> &mut PE {
        match &mut self.processor {
            EngineProcessor::FGO(pe) => pe,
            EngineProcessor::CGO(_) => panic!("cannot access PE on a CGO engine"),
        }
    }

    pub fn get_dram_port(&mut self) -> &mut dram_portal {
        &mut self.dram_port
    }

    pub fn set_external_signal_delays(&mut self, pause_cycles: u64, resume_cycles: u64) {
        self.switch_pause_cycles = pause_cycles;
        self.switch_resume_cycles = resume_cycles;
        if let EngineProcessor::CGO(cpu) = &mut self.processor {
            cpu.set_external_signal_delays(pause_cycles, resume_cycles);
        }
    }

    fn switch_delay_done(&self) -> bool {
        match self.last_service_mode {
            EngineMode::PIM => {
                let processor_ready = match &self.processor {
                    EngineProcessor::CGO(cpu) => cpu.ready4signal(),
                    EngineProcessor::FGO(_) => true,
                };
                processor_ready
                    && self.switch_delay_remaining == 0
                    && self.dram_port.req_drained_for_mode(portal_mode::PIM)
                    && self.dsim3.is_drained()
            }
            EngineMode::HOST => {
                self.switch_delay_remaining == 0
                    && self.dram_port.req_drained_for_mode(portal_mode::HOST)
                    && self.dsim3.is_drained()
            }
            EngineMode::switch_delay => false,
        }
    }

    /*
     * switch() simulates the following automata:
     * Host -> switch_delay(self-looping) -> PIM -> switch_delay(self-looping) -> Host
     */
    fn switch(&mut self, from: EngineMode) {
        match from {
            EngineMode::PIM => {
                self.next_mode = EngineMode::switch_delay;
                self.last_service_mode = EngineMode::PIM;
                self.switch_delay_remaining = match self.processor {
                    EngineProcessor::FGO(_) => self.switch_pause_cycles,
                    EngineProcessor::CGO(_) => 0,
                };
            }
            EngineMode::HOST => {
                self.next_mode = EngineMode::switch_delay;
                self.last_service_mode = EngineMode::HOST;
                self.switch_delay_remaining = match self.processor {
                    EngineProcessor::FGO(_) => self.switch_resume_cycles,
                    EngineProcessor::CGO(_) => 0,
                };
            }
            EngineMode::switch_delay => {
                if self.switch_delay_remaining > 0 {
                    self.switch_delay_remaining -= 1;
                    self.next_mode = EngineMode::switch_delay;
                    return;
                }
                if !self.switch_delay_done() {
                    self.next_mode = EngineMode::switch_delay;
                    return;
                }

                // Keep last_service_mode as the last non-delay service mode until
                // the next real mode requests a switch. switch_delay may last multiple cycles, so
                // overwriting it here would lose the direction needed to leave the
                // self-loop.
                match self.last_service_mode {
                    EngineMode::PIM => {
                        self.next_mode = EngineMode::HOST;
                        self.dram_port.set_mode(portal_mode::HOST);
                        self.dsim3.SetPimMode(false);
                    }
                    EngineMode::HOST => {
                        self.next_mode = EngineMode::PIM;
                        self.dram_port.set_mode(portal_mode::PIM);
                        self.dsim3.SetPimMode(true);
                    }
                    EngineMode::switch_delay => {
                        self.next_mode = EngineMode::switch_delay;
                    }
                }
            }
        }
    }
    fn force_pim_mode(&mut self) {
        self.mode = EngineMode::PIM;
        self.next_mode = EngineMode::PIM;
        self.dram_port.set_mode(portal_mode::PIM);
        self.dsim3.SetPimMode(true);
    }

    fn force_host_mode(&mut self) {
        self.mode = EngineMode::HOST;
        self.next_mode = EngineMode::HOST;
        self.dram_port.set_mode(portal_mode::HOST);
        self.dsim3.SetPimMode(false);
    }

    pub fn schedule(&mut self) {
        #[cfg(test)]
        let mode_before = {
            self.scheduler_probe
                .fetch_or(SCHED_PROBE_INVOKED, Ordering::Relaxed);
            self.mode
        };

        match self.scheduling_mode {
            EngineSchedulingMode::Unconfigured => {
                panic!("cannot schedule an engine before configuring its scheduling mode")
            }
            EngineSchedulingMode::CGO_only => {
                self.force_pim_mode();
            }
            EngineSchedulingMode::HostOnly => {
                self.force_host_mode();
                // dram_portal is stack-backed, so reverse submission preserves FIFO issue order.
                while let Some(req) = self.host_pool.pop_back() {
                    self.dram_port.submit(req);
                }
            }
            EngineSchedulingMode::Host_CGO_share => self.schedule_host_cgo_share(),
            EngineSchedulingMode::Host_FGO_share => self.schedule_host_fgo_share(),
        }

        #[cfg(test)]
        match (mode_before, self.next_mode) {
            (EngineMode::switch_delay, EngineMode::HOST) => {
                self.scheduler_probe
                    .fetch_or(SCHED_PROBE_ENTERED_HOST, Ordering::Relaxed);
            }
            (EngineMode::switch_delay, EngineMode::PIM) => {
                self.scheduler_probe
                    .fetch_or(SCHED_PROBE_ENTERED_PIM, Ordering::Relaxed);
            }
            _ => {}
        }
    }

    /*
     * This implements a batch-based CFS for CGO and host
     */
    fn schedule_host_cgo_share(&mut self) {
        match self.mode {
            EngineMode::PIM => {
                let should_switch_to_host = if self.first_host_switch_started {
                    // PIM->HOST condition for all rest of time
                    self.PIM_tick_watermark <= self.PIM_tick_rec && !self.host_pool.is_empty()
                } else {
                    // PIM->HOST confition for first time
                    (self.host_pool.len() as u64) > self.MEM_req_watermarkL
                };

                if should_switch_to_host {
                    self.first_host_switch_started = true;
                    self.PIM_tick_rec = 0;
                    match &mut self.processor {
                        EngineProcessor::CGO(cpu) => cpu.signal_pause(),
                        EngineProcessor::FGO(_) => unreachable!(),
                    }
                    self.switch(self.mode);
                } else {
                    self.PIM_tick_rec += 1;
                }
            }

            EngineMode::HOST => {
                /*
                 * Although schedule() suppose to be combinational, dsim3 will internally handle request
                 * one by one. In this case, it's okey to blaze all request from Host to dram_port as we
                 * assume switching happened between req-buffer and DDR queue
                 */
                let mut batch = Vec::new();
                while let Some(req) = self.host_pool.pop_front() {
                    batch.push(req);
                    self.MEM_tick_rec += 1;

                    if self.MEM_tick_rec > self.MEM_req_watermarkL {
                        break;
                    }
                }
                for req in batch.into_iter().rev() {
                    self.dram_port.submit(req);
                }

                if self.MEM_tick_rec > self.MEM_req_watermarkL || self.host_pool.is_empty() {
                    self.PIM_tick_watermark = self.MEM_tick_rec;
                    self.MEM_tick_rec = 0;
                    match &mut self.processor {
                        EngineProcessor::CGO(cpu) => cpu.signal_resume(),
                        EngineProcessor::FGO(_) => unreachable!(),
                    }
                    self.switch(self.mode);
                }
            }
            EngineMode::switch_delay => {
                self.switch(EngineMode::switch_delay);
            }
        }
    }

    fn fgo_has_buffered_inst(&self) -> bool {
        match &self.processor {
            EngineProcessor::FGO(pe) => pe.has_buffered_inst(),
            EngineProcessor::CGO(_) => unreachable!(),
        }
    }

    fn fgo_issue_pim(&mut self) {
        match &mut self.processor {
            EngineProcessor::FGO(pe) => pe.allow_next(),
            EngineProcessor::CGO(_) => unreachable!(),
        }
        self.fgo_request_state = FgoRequestState::PimInFlight;
    }

    fn fgo_pim_finished(&mut self) -> bool {
        match &mut self.processor {
            EngineProcessor::FGO(pe) => pe.has_finished(),
            EngineProcessor::CGO(_) => unreachable!(),
        }
    }

    fn fgo_switch_to(&mut self, target: EngineMode) {
        match (self.mode, target) {
            (EngineMode::PIM, EngineMode::HOST) => self.switch(EngineMode::PIM),
            (EngineMode::HOST, EngineMode::PIM) => self.switch(EngineMode::HOST),
            _ => {}
        }
    }

    fn fgo_select_in_pim_mode(&mut self) {
        let pim_ready = self.fgo_has_buffered_inst();
        let host_ready = !self.host_pool.is_empty();

        match self.fgo_next_service {
            EngineMode::PIM if pim_ready => self.fgo_issue_pim(),
            EngineMode::HOST if host_ready => self.fgo_switch_to(EngineMode::HOST),
            _ if pim_ready => self.fgo_issue_pim(),
            _ if host_ready => self.fgo_switch_to(EngineMode::HOST),
            _ => {}
        }
    }

    fn fgo_select_in_host_mode(&mut self) {
        let pim_ready = self.fgo_has_buffered_inst();
        let host_ready = !self.host_pool.is_empty();

        match self.fgo_next_service {
            EngineMode::PIM if pim_ready => self.fgo_switch_to(EngineMode::PIM),
            EngineMode::HOST if host_ready => self.fgo_issue_host(),
            _ if pim_ready => self.fgo_switch_to(EngineMode::PIM),
            _ if host_ready => self.fgo_issue_host(),
            _ => {}
        }
    }

    fn fgo_issue_host(&mut self) {
        let req = self
            .host_pool
            .pop_front()
            .expect("host request must exist before FGO host issue");
        self.dram_port.submit(req);
        self.fgo_request_state = FgoRequestState::HostInFlight;
    }

    /*
     * TODO
     * This is the current FGO schedule algorithm
     * It's now a basic round-robin, act like a stub for future F3FS implementation
     */
    fn schedule_host_fgo_share(&mut self) {
        match self.mode {
            EngineMode::switch_delay => {
                self.switch(EngineMode::switch_delay);
            }
            EngineMode::PIM => {
                if matches!(self.fgo_request_state, FgoRequestState::PimInFlight)
                    && self.fgo_pim_finished()
                {
                    self.fgo_request_state = FgoRequestState::Idle;
                    self.fgo_next_service = EngineMode::HOST;
                }

                if matches!(self.fgo_request_state, FgoRequestState::Idle) {
                    self.fgo_select_in_pim_mode();
                }
            }
            EngineMode::HOST => {
                if matches!(self.fgo_request_state, FgoRequestState::HostInFlight)
                    && self.dram_port.req_drained_for_mode(portal_mode::HOST)
                    && self.dsim3.is_drained()
                {
                    self.fgo_request_state = FgoRequestState::Idle;
                    self.fgo_next_service = EngineMode::PIM;
                }

                if matches!(self.fgo_request_state, FgoRequestState::Idle) {
                    self.fgo_select_in_host_mode();
                }
            }
        }
    }

    /*
     * This is the function used by Host to send request
     */
    pub fn host_push_req(&mut self, req: portal_req) {
        match req {
            // If request is from host and it's PE instruction
            portal_req::HOST_REQ { req } if is_pe_request(req.get_addr()) => {
                let instruction = decode_pe_inst(req.get_addr())
                    .unwrap_or_else(|err| panic!("cannot decode PE request: {err}"));
                match &mut self.processor {
                    EngineProcessor::FGO(pe) => pe.push_host_req(req, instruction),
                    EngineProcessor::CGO(_) => {
                        panic!("cannot route a PE instruction to a CGO/CPU engine")
                    }
                }
            }

            //If request is from host but it's a regular DRAM access
            portal_req::HOST_REQ { req } => {
                self.host_pool.push_back(portal_req::HOST_REQ { req });
            }
            _ => {
                eprintln!("Host cannot push PIM request");
                unreachable!()
            }
            // portal_req::PIM_REQ { req } => {
            //     self.host_pool.push_back(portal_req::PIM_REQ { req });
            // }
        }
    }

    pub fn canAccept(&mut self, addr: u64, is_write: bool) -> bool {
        if is_pe_request(addr) {
            return matches!(self.processor, EngineProcessor::FGO(_))
                && decode_pe_inst(addr).is_ok();
        }
        self.dsim3.WillAcceptTransaction(addr, is_write)
    }

    fn drain_current_port_to_dram(&mut self) {
        loop {
            let Some(mut req) = self.dram_port.get_one_req() else {
                break;
            };

            let is_pim = req.is_pim();
            let is_write = !req.is_read();
            let addr = req.get_addr();

            if self.dsim3.WillAcceptTransaction(addr, is_write) {
                req.set_id(self.dsim3.get_req_id());
                req.set_issue_time(self.clock_cycle);
                self.dsim3.AddTransactionReq(req);
            } else {
                if is_pim {
                    self.dram_port.submit(portal_req::PIM_REQ { req });
                } else {
                    self.dram_port.submit(portal_req::HOST_REQ { req });
                }

                break;
            }
        }
    }

    fn drain_host_completions(&mut self) {
        if let EngineProcessor::FGO(pe) = &mut self.processor {
            while pe.has_complete() {
                self.host_complete_queue.push_back(
                    pe.take_completed()
                        .expect("PE completion queue changed while being drained"),
                );
            }
        }

        while let Some(req) = self.dram_port.take_host_completed() {
            self.host_complete_queue.push_back(req);
        }
    }

    pub fn get_host_complete(&mut self) -> Option<dram_req> {
        self.host_complete_queue.pop_front()
    }

    pub fn host_has_complete(&self) -> bool {
        !self.host_complete_queue.is_empty()
    }

    pub fn tick(&mut self) {
        match self.scheduling_mode {
            EngineSchedulingMode::Unconfigured => {
                panic!("cannot tick an engine before configuring its scheduling mode")
            }
            EngineSchedulingMode::CGO_only | EngineSchedulingMode::Host_CGO_share => {
                match &mut self.processor {
                    EngineProcessor::CGO(cpu) => cpu.tick(),
                    EngineProcessor::FGO(_) => unreachable!(),
                }
            }
            EngineSchedulingMode::Host_FGO_share => {
                if matches!(self.mode, EngineMode::PIM)
                    && matches!(self.fgo_request_state, FgoRequestState::PimInFlight)
                {
                    match &mut self.processor {
                        EngineProcessor::FGO(pe) => pe.tick(),
                        EngineProcessor::CGO(_) => unreachable!(),
                    }
                }
            }
            EngineSchedulingMode::HostOnly => {}
        }
        self.drain_current_port_to_dram();

        for req in self.dsim3.ClockTick() {
            self.dram_port.complete(req);
        }
        self.drain_host_completions();

        self.schedule();
        self.mode = self.next_mode;
        self.clock_cycle += 1;
    }
}

#[cfg(test)]
#[path = "engine_test.rs"]
mod engine_test;
