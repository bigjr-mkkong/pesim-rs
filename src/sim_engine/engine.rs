use std::cell::RefCell;
use std::rc::Rc;
use crate::CPU;
use crate::memory::dramsim3_wrapper::dramsim3_wrapper;
use crate::memory::mem_portal::{portal_req, dram_portal, portal_mode};
use crate::DSIM3_CFG_PATH;
use crate::DSIM3_OUT_DIR;

const BATCH_SZ: u64 = 0;

enum EngineMode{
    PIM,
    HOST,
    switch_delay,
}

/*
 *
 * TODO
 * Engine will support both sim_cpu access dram and host access dram, ownership are coordinated
 * through schedule()
 *
 * sim_cpu should own a mutable reference to dram_port internal queue, and should be able to call
 * dram_port.submit() to submit request
 * Also Engine itself should able to buffer request when it's running in PIM mode, and push then 
 * out when it's running in HOST mode.
 *
 * Task:
 * 1. Scheduler has already been implemented, verify it's a Fair fixed batching based scheduler
 * 2. sim_cpu now does not have the ability to submit request to dramsim3 and ask if the access on
 *    certain address is finished. Put this logic inside MEM_stop_FSM::advance_winner()
 * 3. Make sure Engine.tick() also tick dramsim3::tick()
 */
struct Engine{
    sim_cpu: CPU,
    host_pool: Vec<portal_req>,
    dram_port: dram_portal,
    dsim3: dramsim3_wrapper,
    //Following are scheduler internal variables
    mode: EngineMode,
    next_mode: EngineMode,
    coming_from_mode: EngineMode,
    PIM_tick_watermark: u64,
    PIM_tick_rec: u64,
    MEM_req_watermarkL: u64,
    MEM_tick_rec: u64,
}

impl Engine{
    pub fn new() -> Self{
        Self{
            sim_cpu: CPU::new(),
            dram_port: dram_portal::new(),
            host_pool: Vec::new(),
            dsim3: dramsim3_wrapper::new(DSIM3_CFG_PATH, DSIM3_OUT_DIR,
                0, 0, 0, 0),
            mode: EngineMode::HOST,
            next_mode: EngineMode::switch_delay,
            coming_from_mode: EngineMode::HOST,
            PIM_tick_watermark: 0,
            PIM_tick_rec: 0,
            MEM_req_watermarkL: BATCH_SZ,
            MEM_tick_rec: 0
        }
    }

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
            } else if let EngineMode::HOST = self.coming_from_mode{
                self.next_mode = EngineMode::PIM;
                self.coming_from_mode = EngineMode::switch_delay;
                self.dram_port.set_mode(portal_mode::PIM);
            } else {
                panic!("Cannot switch() from switch_delay to switch_delay");
            }
        }
    }
    /*
     *Host -> SW_stale -> PIM -> SW_stale -> Host
     */
    pub fn schedule(&mut self) {
        match self.mode{
            EngineMode::PIM => {
                if self.PIM_tick_watermark <= self.PIM_tick_rec {
                    self.sim_cpu.signal_pause();//Signal sim_cpu to stop
                    self.switch(EngineMode::PIM);
                }
            },
            EngineMode::HOST => {
                let mut stop_trans = false;
                while !self.host_pool.is_empty() || !stop_trans {
                    if let Some(req) = self.host_pool.pop() {
                        self.dram_port.submit(req);
                        if self.MEM_tick_rec > self.MEM_req_watermarkL {
                            self.sim_cpu.signal_resume();
                            self.switch(EngineMode::HOST);
                            break;
                        }
                    }
                }
            },
            EngineMode::switch_delay => {
                if let EngineMode::PIM = self.coming_from_mode {
                    if self.sim_cpu.ready4signal() {
                        self.switch(EngineMode::switch_delay);
                    }
                    //Otherwise stay at switch_delay mode
                } else {
                    //Check DRAM state, if it's drained then switch to PIM mode
                    todo!();
                    self.switch(EngineMode::switch_delay);
                }
            }
        }
    }

    pub fn host_push_req(&mut self, req: portal_req) {
        self.host_pool.push(req);
    }

    pub fn tick(&mut self){
        self.sim_cpu.tick();
    }

}
