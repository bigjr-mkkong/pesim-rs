use crate::CPU;
use crate::DSIM3_CFG_PATH;
use crate::DSIM3_OUT_DIR;
use crate::memory::dramsim3_wrapper::dramsim3_wrapper;
use crate::memory::mem_portal::{dram_portal, portal_mode, portal_req};

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
}

pub struct Engine {
    sim_cpu: CPU,
    host_pool: Vec<portal_req>,
    dram_port: dram_portal,
    dsim3: dramsim3_wrapper,
    active_port_drained: bool,
    scheduling_mode: EngineSchedulingMode,
    //Following are scheduler internal variables
    mode: EngineMode,
    next_mode: EngineMode,
    coming_from_mode: EngineMode,
    PIM_tick_watermark: u64,
    PIM_tick_rec: u64,
    MEM_req_watermarkL: u64,
    MEM_tick_rec: u64,
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

    pub fn with_scheduling_mode(scheduling_mode: EngineSchedulingMode) -> Self {
        let mut dram_port = dram_portal::new();
        dram_port.set_mode(portal_mode::PIM);

        let mut dsim3 = dramsim3_wrapper::new(DSIM3_CFG_PATH, DSIM3_OUT_DIR, 0, 0, 0, 0);
        dsim3.SetPimMode(true);

        Self {
            sim_cpu: CPU::new_with_dram_port(dram_port.clone()),
            dram_port,
            host_pool: Vec::new(),
            dsim3,
            active_port_drained: true,
            scheduling_mode,
            mode: EngineMode::PIM,
            next_mode: EngineMode::PIM,
            coming_from_mode: EngineMode::PIM,
            PIM_tick_watermark: 0,
            PIM_tick_rec: 0,
            MEM_req_watermarkL: BATCH_SZ,
            MEM_tick_rec: 0,
        }
    }

    pub fn get_cpu(&mut self) -> &mut CPU {
        &mut self.sim_cpu
    }

    pub fn get_dram_port(&mut self) -> &mut dram_portal {
        &mut self.dram_port
    }

    /*
     *Host -> SW_stale -> PIM -> SW_stale -> Host
     */
    fn switch(&mut self, from: EngineMode) {
        if let EngineMode::PIM = from {
            self.PIM_tick_rec = 0;
            self.next_mode = EngineMode::switch_delay;
            self.coming_from_mode = EngineMode::PIM;
        } else if let EngineMode::HOST = from {
            self.PIM_tick_watermark = self.MEM_tick_rec;
            self.MEM_tick_rec = 0;
            self.next_mode = EngineMode::switch_delay;
            self.coming_from_mode = EngineMode::HOST;
        } else {
            if let EngineMode::PIM = self.coming_from_mode {
                self.next_mode = EngineMode::HOST;
                self.coming_from_mode = EngineMode::switch_delay;
                self.dram_port.set_mode(portal_mode::HOST);
                self.dsim3.SetPimMode(false);
            } else if let EngineMode::HOST = self.coming_from_mode {
                self.next_mode = EngineMode::PIM;
                self.coming_from_mode = EngineMode::switch_delay;
                self.dram_port.set_mode(portal_mode::PIM);
                self.dsim3.SetPimMode(true);
            } else {
                panic!("Cannot switch() from switch_delay to switch_delay");
            }
        }
    }
    fn force_pim_mode(&mut self) {
        self.mode = EngineMode::PIM;
        self.next_mode = EngineMode::PIM;
        self.dram_port.set_mode(portal_mode::PIM);
        self.dsim3.SetPimMode(true);
    }

    pub fn schedule(&mut self) {
        if let EngineSchedulingMode::PimOnly = self.scheduling_mode {
            self.force_pim_mode();
            self.PIM_tick_rec += 1;
            return;
        }

        self.next_mode = self.mode;

        match self.mode {
            EngineMode::PIM => {
                //TODO
                //Having a real schedule algorithm
                // if self.PIM_tick_watermark <= self.PIM_tick_rec {
                //     self.sim_cpu.signal_pause(); //Signal sim_cpu to stop
                //     self.switch(EngineMode::PIM);
                // } else {
                //     self.PIM_tick_rec += 1;
                // }
                self.PIM_tick_rec += 1;
            }
            EngineMode::HOST => {
                while let Some(req) = self.host_pool.pop() {
                    self.dram_port.submit(req);
                    self.active_port_drained = false;
                    self.MEM_tick_rec += 1;

                    if self.MEM_tick_rec > self.MEM_req_watermarkL {
                        self.sim_cpu.signal_resume();
                        self.switch(EngineMode::HOST);
                        break;
                    }
                }
            }
            EngineMode::switch_delay => {
                if let EngineMode::PIM = self.coming_from_mode {
                    if self.sim_cpu.ready4signal()
                        && self.active_port_drained
                        && self.dsim3.is_drained()
                    {
                        self.switch(EngineMode::switch_delay);
                    }
                    //Otherwise stay at switch_delay mode
                } else if self.active_port_drained && self.dsim3.is_drained() {
                    self.switch(EngineMode::switch_delay);
                }
            }
        }
    }

    pub fn host_push_req(&mut self, req: portal_req) {
        self.host_pool.push(req);
    }

    fn drain_active_port_to_dram(&mut self) {
        self.active_port_drained = true;

        loop {
            let Some(req) = self.dram_port.get_one_req() else {
                break;
            };

            let is_pim = req.is_pim();
            let is_write = !req.is_read();
            let addr = req.get_addr();

            if self.dsim3.WillAcceptTransaction(addr, is_write) {
                self.dsim3.AddTransactionReq(req);
            } else {
                if is_pim {
                    self.dram_port.submit(portal_req::PIM_REQ { req });
                } else {
                    self.dram_port.submit(portal_req::HOST_REQ { req });
                }

                self.active_port_drained = false;
                break;
            }
        }
    }

    pub fn tick(&mut self) {
        if let EngineSchedulingMode::PimOnly = self.scheduling_mode {
            self.force_pim_mode();
        }

        self.sim_cpu.tick(); // This tick will eat previous commited dram_req or generate new dram_req
        self.drain_active_port_to_dram();

        for req in self.dsim3.ClockTick() {
            self.dram_port.complete(req);
        }

        self.schedule();
        self.mode = self.next_mode;
    }
}
