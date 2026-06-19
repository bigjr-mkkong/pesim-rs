use crate::CPU;
use crate::DSIM3_CFG_PATH;
use crate::DSIM3_OUT_DIR;
use crate::PE::pe_top::PE;
use crate::memory::dramsim3_wrapper::dramsim3_wrapper;
use crate::memory::mem_portal::{dram_portal, dram_req, portal_mode, portal_req};
use std::collections::VecDeque;

const BATCH_SZ: u64 = 0;

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
        }
    }

    pub fn set_scheduling_mode(
        &mut self,
        scheduling_mode: EngineSchedulingMode,
    ) -> Result<(), &'static str> {
        if self.scheduling_mode != EngineSchedulingMode::Unconfigured {
            // TODO: Support live scheduler reconfiguration by defining how active processor and
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

    /*
     * switch() simulates the following automata:
     * Host -> switch_delay(self-looping) -> PIM -> switch_delay(self-looping) -> Host
     */
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
        match self.scheduling_mode {
            EngineSchedulingMode::Unconfigured => {
                panic!("cannot schedule an engine before configuring its scheduling mode")
            }
            EngineSchedulingMode::CGO_only => {
                self.force_pim_mode();
                self.PIM_tick_rec += 1;
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
    }

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
        self.host_pool.push_back(req);
    }

    pub fn canAccept(&mut self, addr: u64, is_write: bool) -> bool {
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

    pub fn get_host_complete(&mut self) -> Option<dram_req> {
        self.dram_port.take_host_completed()
    }

    pub fn host_has_complete(&self) -> bool {
        self.dram_port.host_has_complete()
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

        self.schedule();
        self.mode = self.next_mode;
        self.clock_cycle += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PE::types::inst;

    #[test]
    fn scheduling_configuration_is_one_time_and_processor_checked() {
        let mut cgo = Engine::new_cgo();
        assert_eq!(
            cgo.set_scheduling_mode(EngineSchedulingMode::Host_FGO_share),
            Err("scheduling mode is incompatible with the engine processor")
        );
        cgo.set_scheduling_mode(EngineSchedulingMode::CGO_only)
            .unwrap();
        assert_eq!(
            cgo.set_scheduling_mode(EngineSchedulingMode::Host_CGO_share),
            Err("engine scheduling mode can only be configured once")
        );

        let mut fgo = Engine::new_fgo();
        assert_eq!(
            fgo.set_scheduling_mode(EngineSchedulingMode::CGO_only),
            Err("scheduling mode is incompatible with the engine processor")
        );
        fgo.set_scheduling_mode(EngineSchedulingMode::Host_FGO_share)
            .unwrap();
    }

    #[test]
    #[should_panic(expected = "cannot tick an engine before configuring")]
    fn unconfigured_engine_rejects_tick() {
        Engine::new_cgo().tick();
    }

    #[test]
    fn fgo_switch_delay_counts_complete_cycles_in_both_directions() {
        let mut engine = Engine::new_fgo();
        engine.set_external_signal_delays(2, 3);
        engine
            .set_scheduling_mode(EngineSchedulingMode::Host_FGO_share)
            .unwrap();

        engine.switch(EngineMode::PIM);
        engine.mode = engine.next_mode;
        for _ in 0..2 {
            engine.schedule();
            engine.mode = engine.next_mode;
            assert_eq!(engine.mode, EngineMode::switch_delay);
        }
        engine.schedule();
        engine.mode = engine.next_mode;
        assert_eq!(engine.mode, EngineMode::HOST);

        engine.switch(EngineMode::HOST);
        engine.mode = engine.next_mode;
        for _ in 0..3 {
            engine.schedule();
            engine.mode = engine.next_mode;
            assert_eq!(engine.mode, EngineMode::switch_delay);
        }
        engine.schedule();
        engine.mode = engine.next_mode;
        assert_eq!(engine.mode, EngineMode::PIM);
    }

    #[test]
    fn fgo_round_robin_completes_one_pe_and_fifo_host_request_at_a_time() {
        let mut engine = Engine::new_fgo();
        engine
            .set_scheduling_mode(EngineSchedulingMode::Host_FGO_share)
            .unwrap();

        {
            let pe = engine.get_pe();
            pe.get_Arf().write_vRF(1, [4; 8]);
            pe.get_Arf().write_vRF(2, [5; 8]);
            pe.get_Arf().write_vRF(4, [20; 8]);
            pe.get_Arf().write_vRF(5, [3; 8]);
            pe.push_host_inst(inst::ADD128 {
                vRD: 3,
                vRS0: 1,
                vRS1: 2,
            });
            pe.push_host_inst(inst::SUB128 {
                vRD: 6,
                vRS0: 4,
                vRS1: 5,
            });
        }

        engine.host_push_req(portal_req::HOST_REQ {
            req: dram_req::new(0x40, true, false),
        });
        engine.host_push_req(portal_req::HOST_REQ {
            req: dram_req::new(0x80, true, false),
        });

        let mut completed_addrs = Vec::new();
        for _ in 0..20_000 {
            engine.tick();
            while let Some(req) = engine.get_host_complete() {
                completed_addrs.push(req.get_addr());
            }

            let pe_done = {
                let pe = engine.get_pe();
                pe.get_Arf().read_vRF(3) == [9; 8] && pe.get_Arf().read_vRF(6) == [17; 8]
            };
            if pe_done && completed_addrs.len() == 2 {
                break;
            }
        }

        assert_eq!(completed_addrs, vec![0x40, 0x80]);
        assert_eq!(engine.get_pe().get_Arf().read_vRF(3), [9; 8]);
        assert_eq!(engine.get_pe().get_Arf().read_vRF(6), [17; 8]);
    }

    #[test]
    fn fgo_waits_for_memory_instruction_completion() {
        let mut engine = Engine::new_fgo();
        engine
            .set_scheduling_mode(EngineSchedulingMode::Host_FGO_share)
            .unwrap();
        {
            let pe = engine.get_pe();
            pe.get_fmem().mem_write_s(0x300, 2468).unwrap();
            pe.push_host_inst(inst::LD32 {
                sRD: 7,
                addr: 0x300,
            });
        }

        for _ in 0..20_000 {
            engine.tick();
            if engine.get_pe().get_Arf().read_sRF(7) == 2468 {
                assert!(!engine.get_pe().has_buffered_inst());
                return;
            }
        }

        panic!("FGO memory instruction did not complete through the engine DRAM path");
    }
}
