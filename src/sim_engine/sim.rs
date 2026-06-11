/*
 * Integration Notes:
 * This is Highest level of simulator
 * This simulator will eventually hooked up with gem5
 * Here are some functions required by gem5:
 *      void printStats();
 *      void resetStats();
 * 
 *      bool canAccept(uint64_t addr, bool is_write) const;
 *      void enqueue(uint64_t addr, bool is_write);
 * 
 *      double clockPeriod() const;
 *      unsigned int queueSize() const;
 *      unsigned int burstSize() const;
 * 
 *      bool hasComplete() const;           // return true if any request from gem5 has completed
 *      PEsim_rs_MemReq getComplete();      // return completed gem5 request
 * 
 *      void tick();
 *
 *      they also need to share an intermediate datastructure called PEsim_rs_MemReq, which looks
 *      like:
 *        
 *      struct PEsim_rs_MemReq{
 *          uint64_t addr = 0;
 *          uint64_t issue_time = 0;
 *          bool is_write = false;
 *      };
 *
 * Design Note:
 * sim should contain one regular DRAMsim3(called mono_dsim3) and several engine(same number as PEs we want to simulate)
 * sim have two working mode: Regular and PESIM.
 * In Regular Mode, sim will directly bypass host request to mono_dsim3 and obtain result from it
 * In PESIM mode, sim will both enqueue request into mono_dsim3 and corresponding Engine. However,
 * it will only pop resunt out from Engine instead of mono_dsim3(mono_dsim3 still tick with Engine).
 * This is because we want to maintain consistent dram timing model when switching back to Regular
 * from PESIM
 */

use crate::sim_engine::engine::{Engine, EngineSchedulingMode};
use crate::memory::mem_portal::{dram_req};
use crate::memory::dramsim3_wrapper::dramsim3_wrapper;
use std::collections::HashMap;
use crate::{DSIM3_CFG_PATH, DSIM3_OUT_DIR};

#[derive(PartialEq, Eq, Hash)]
pub enum engine_cfg{
    CGO{ch: u16, ra: u16, bg: u16, ba: u16},
    FGO{ch: u16, ra: u16, bg: u16, ba: u16},
}

pub enum SimMode{
    Host,
    Pim
}

pub struct Sim{
    engines: HashMap<engine_cfg, Engine>,
    host_pool: Vec<dram_req>,
    dsim3: dramsim3_wrapper,
    sim_mode: SimMode
}

impl Sim{
    pub fn new() -> Self{
        let mut dsim3_inst = dramsim3_wrapper::new(DSIM3_CFG_PATH, DSIM3_OUT_DIR, 0, 0, 0, 0);
        dsim3_inst.SetPimMode(false); //Set dsim3 as non-pim as it handle normal traces
        Self{
            engines: HashMap::new(),
            host_pool: Vec::new(),
            dsim3: dsim3_inst,
            sim_mode: SimMode::Host,
        }
    }

    pub fn add_engines(&mut self, cfg: engine_cfg){
        match self.engines.entry(cfg) {
            std::collections::hash_map::Entry::Occupied(_) => {
                panic!("Cannot add engine with given cfg: already existed");
            },
            std::collections::hash_map::Entry::Vacant(mut ent) => {
                ent.insert(Engine::new());
            }
        }
    }

    pub fn can_accept(&mut self, addr: u64, is_write: bool) -> bool{
        self.dsim3.WillAcceptTransaction(addr, is_write)
    }

    pub fn enqueue(&mut self, addr: u64, is_write: bool) {
        if let SimMode::Host = self.sim_mode {
            //directly push addr and is_write to self.dsim3 as dram_req
        } else {
            //use self.dsim3 to translate addr into ch, ra, bg, ba, then push to corresponding
            //engine
            //Also push to self.dsim3 for synchronization purpose
        }
    }

    pub fn tick(&mut self) {
        if let SimMode::Host = self.sim_mode {
            // call self.dsim3.tick()
        } else {
            // call each engine.tick() plus self.dsim3.tick() in multi thread 
        }
    }
}
