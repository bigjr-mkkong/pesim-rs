use crate::CPU;
use crate::DSIM3_CFG_PATH;
use crate::DSIM3_OUT_DIR;
use crate::memory::dramsim3_wrapper::dramsim3_wrapper;
use crate::memory::mem_portal::{dram_portal, dram_req, portal_mode, portal_req};

const BATCH_SZ: u64 = 0;

#[derive(Clone, Copy)]
enum EngineMode {
    PIM,
    HOST,
    switch_delay,
}

#[derive(Clone, Copy)]
pub enum EngineSchedulingMode {
    PimOnly,
    ScheduledHostPim,
    HostOnly,
}

/*
 * TODO
 * Add sim_pe: PE into this engine
 * PE behave different with CPU. CPU will autonomously running and buffer all user request when it's
 * running. But PE works under the assumption that no request will arrive when it's executing.
 *
 * In this case, although host_pool still act like a secondary source of dram_req goes into
 * dram_port, but the scheduler behave differently.
 *
 * PE_scheduler will basically be a F3FS scheduler. Instead of being a cycle-level
 * scheduler,
 * it's more like a request level scheduler. It will finish one request either from PE or host_pool,
 * and determine which one will have the right to issue next request.
 *
 * In this case, there are two things need to be done in PE side:
 *
 * Done in PE: implemented an imem buffer for host-issued commands, allow_next(), and
 * completion-based has_finished() for architectural updates / DRAM operation completion.
 *
 * Remaining TODOs:
 * Task #1: De-couple mechanism-wise and scheduler algorithm state update from switch()
 *      #1 explain: tick counter(for example, PIM_tick_watermark) are policy-wise variable, which
 *      suppose to be updated by scheduler instead of switch(), move them out of switch
 * Task #2: Rename EngineSchedulingMode::ScheduledHostPim into Host_CGO_share, Rename
 * EngineSchedulingMode::PimOnly into CGO_only, and add Host_FGO_share type.
 *
 * Task #3: Modify Engine new_* functions according to above name changing
 *
 * Task #4: Add a switch-delay simulation similiar to CPU::update_extsig_rdy() for PE switch() when
 * simulator running under Host_FGO_share mode
 * - In fact they are the same. The reason the switch-delay simulation exists in engine level
 * instead of PE level is PE is request-wise processor and does not contain stale state when
 * PE::has_finished() return true. In this case, the switch-delay logic can exists in engine level.
 *
 * Task #5: Add new branch inside tick() for PE ticking. The scheduler for PE can left as
 * round-robin(PIM then MEM then PIM ...)
 *
 * Current design of scheduler algorithm completly sitting inside struct Engine. Although this is not a good
 * practice, design more complex abstraction layer for scheduler would also be over-killed. In this
 * case, stick with current all-in-one scheduler internal state keep is fine
 */

pub struct Engine {
    sim_cpu: CPU,
    host_pool: Vec<portal_req>,
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
}

impl Engine {
    pub fn new() -> Self {
        Self::new_pim_only()
    }

    pub fn new_pim_only() -> Self {
        Self::with_scheduling_mode(EngineSchedulingMode::PimOnly)
    }

    pub fn new_scheduled_host_pim() -> Self {
        Self::with_scheduling_mode(EngineSchedulingMode::ScheduledHostPim)
    }

    pub fn new_host_only() -> Self {
        Self::with_scheduling_mode(EngineSchedulingMode::HostOnly)
    }

    pub fn with_scheduling_mode(scheduling_mode: EngineSchedulingMode) -> Self {
        let mut dram_port = dram_portal::new();
        let host_only = matches!(scheduling_mode, EngineSchedulingMode::HostOnly);
        let initial_portal_mode = if host_only {
            portal_mode::HOST
        } else {
            portal_mode::PIM
        };

        dram_port.set_mode(initial_portal_mode);

        let mut dsim3 = dramsim3_wrapper::new(DSIM3_CFG_PATH, DSIM3_OUT_DIR, 0, 0, 0, 0);
        dsim3.SetPimMode(!host_only);

        Self {
            sim_cpu: CPU::new_with_dram_port(dram_port.clone()),
            dram_port,
            host_pool: Vec::new(),
            dsim3,
            scheduling_mode,
            mode: if host_only {
                EngineMode::HOST
            } else {
                EngineMode::PIM
            },
            next_mode: if host_only {
                EngineMode::HOST
            } else {
                EngineMode::PIM
            },
            last_service_mode: if host_only {
                EngineMode::HOST
            } else {
                EngineMode::PIM
            },
            PIM_tick_watermark: 0,
            PIM_tick_rec: 0,
            MEM_req_watermarkL: BATCH_SZ,
            MEM_tick_rec: 0,
            first_host_switch_started: host_only,
            clock_cycle: 0,
        }
    }

    pub fn get_cpu(&mut self) -> &mut CPU {
        &mut self.sim_cpu
    }

    pub fn get_dram_port(&mut self) -> &mut dram_portal {
        &mut self.dram_port
    }

    pub fn set_external_signal_delays(&mut self, pause_cycles: u64, resume_cycles: u64) {
        self.sim_cpu
            .set_external_signal_delays(pause_cycles, resume_cycles);
    }

    /*
     * switch() simulates the following automata:
     * Host -> switch_delay(self-looping) -> PIM -> switch_delay(self-looping) -> Host
     */
    fn switch_delay_done(&self) -> bool {
        match self.last_service_mode {
            EngineMode::PIM => {
                self.sim_cpu.ready4signal()
                    && self.dram_port.req_drained_for_mode(portal_mode::PIM)
                    && self.dsim3.is_drained()
            }
            EngineMode::HOST => {
                self.dram_port.req_drained_for_mode(portal_mode::HOST) && self.dsim3.is_drained()
            }
            EngineMode::switch_delay => false,
        }
    }

    fn switch(&mut self, from: EngineMode) {
        match from {
            EngineMode::PIM => {
                // From PIM to HOST.  Once this path is taken, subsequent PIM
                // windows use the measured host-service watermark instead of
                // the initial host-queue-depth threshold.
                self.first_host_switch_started = true;
                self.PIM_tick_rec = 0;
                self.next_mode = EngineMode::switch_delay;
                self.last_service_mode = EngineMode::PIM;
            }
            EngineMode::HOST => {
                // From HOST to PIM
                self.PIM_tick_watermark = self.MEM_tick_rec;
                self.MEM_tick_rec = 0;
                self.next_mode = EngineMode::switch_delay;
                self.last_service_mode = EngineMode::HOST;
            }
            EngineMode::switch_delay => {
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
            EngineSchedulingMode::PimOnly => {
                self.force_pim_mode();
                self.PIM_tick_rec += 1;
                return;
            }
            EngineSchedulingMode::HostOnly => {
                self.force_host_mode();
                while let Some(req) = self.host_pool.pop() {
                    self.dram_port.submit(req);
                }
                return;
            }
            EngineSchedulingMode::ScheduledHostPim => {
                // continue
            }
        }

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
                    self.sim_cpu.signal_pause(); //Signal sim_cpu to stop
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
                while let Some(req) = self.host_pool.pop() {
                    self.dram_port.submit(req);
                    self.MEM_tick_rec += 1;

                    if self.MEM_tick_rec > self.MEM_req_watermarkL {
                        break;
                    }
                }

                if self.MEM_tick_rec > self.MEM_req_watermarkL || self.host_pool.is_empty() {
                    self.sim_cpu.signal_resume();
                    self.switch(self.mode);
                }
            }
            EngineMode::switch_delay => {
                self.switch(EngineMode::switch_delay);
            }
        }
    }

    /*
     * This is the function used by Host to send request
     */
    pub fn host_push_req(&mut self, req: portal_req) {
        self.host_pool.push(req);
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
        if !matches!(self.scheduling_mode, EngineSchedulingMode::HostOnly) {
            // This tick will eat previous commited dram_req or generate new dram_req.
            self.sim_cpu.tick();
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
