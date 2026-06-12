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

use crate::memory::dramsim3_cxx_ffi::dramsim3_ffi::local_addr_bulk;
use crate::memory::dramsim3_wrapper::dramsim3_wrapper;
use crate::memory::mem_portal::{dram_req, portal_req};
use crate::sim_engine::engine::{Engine, EngineSchedulingMode};
use crate::{DSIM3_CFG_PATH, DSIM3_OUT_DIR};
use std::collections::HashMap;

#[derive(PartialEq, Eq, Hash)]
pub enum engine_cfg {
    CGO { ch: u64, ra: u64, bg: u64, ba: u64 },
    FGO { ch: u64, ra: u64, bg: u64, ba: u64 },
}

pub enum SimMode {
    Host,
    Pim,
}

pub struct Sim {
    engines: HashMap<engine_cfg, Engine>,
    host_pool: Vec<dram_req>,
    dsim3: dramsim3_wrapper,
    dsim3_comp_queue: Vec<dram_req>,
    sim_mode: SimMode,
}

impl Sim {
    pub fn new() -> Self {
        let mut dsim3_inst = dramsim3_wrapper::new(DSIM3_CFG_PATH, DSIM3_OUT_DIR, 0, 0, 0, 0);
        dsim3_inst.SetPimMode(false); //Set dsim3 as non-pim as it handle normal traces
        Self {
            engines: HashMap::new(),
            host_pool: Vec::new(),
            dsim3: dsim3_inst,
            dsim3_comp_queue: Vec::new(),
            sim_mode: SimMode::Host,
        }
    }

    pub fn add_engines(&mut self, cfg: engine_cfg) {
        match self.engines.entry(cfg) {
            std::collections::hash_map::Entry::Occupied(_) => {
                panic!("Cannot add engine with given cfg: already existed");
            }
            std::collections::hash_map::Entry::Vacant(mut ent) => {
                ent.insert(Engine::new());
            }
        }
    }

    pub fn can_accept(&mut self, addr: u64, is_write: bool) -> bool {
        self.dsim3.WillAcceptTransaction(addr, is_write)
    }

    //This function will return None if addr belongs to non-pim area
    fn get_engine_cfg(&mut self, addr: u64) -> Option<engine_cfg> {
        let addr_bulk = self.dsim3.global2local_addr_translate(addr);
        let cgo_cfg = engine_cfg::CGO {
            ch: addr_bulk.channel,
            ra: addr_bulk.rank,
            bg: addr_bulk.bank_group,
            ba: addr_bulk.bank,
        };
        let fgo_cfg = engine_cfg::CGO {
            ch: addr_bulk.channel,
            ra: addr_bulk.rank,
            bg: addr_bulk.bank_group,
            ba: addr_bulk.bank,
        };

        if self.engines.get(&cgo_cfg).is_some() {
            return Some(cgo_cfg);
        }

        if self.engines.get(&fgo_cfg).is_some() {
            return Some(fgo_cfg);
        }

        None
    }

    pub fn enqueue(&mut self, addr: u64, is_write: bool) {
        let req = dram_req::new(addr, !is_write, false);
        if let SimMode::Pim = self.sim_mode {
            //use self.dsim3 to translate addr into ch, ra, bg, ba, then push to corresponding
            //engine
            //Also push to self.dsim3 for synchronization purpose
            let port_req = portal_req::HOST_REQ { req: req.clone() };
            let cfg = self.get_engine_cfg(addr);
            if let Some(cfg_) = cfg {
                self.engines
                    .get_mut(&cfg_)
                    .expect("Cannot detect available engine")
                    .host_push_req(port_req);
            }
        }

        // always push into host dsim so host dsim3 will maintain valid state after PIM simulation
        // finished
        self.dsim3.AddTransactionReq(req);
    }

    pub fn has_complete(&self) -> bool {
        if let SimMode::Host = self.sim_mode {
            //only check if dsim3 has completed req
            !self.dsim3_comp_queue.is_empty()
        } else {
            //check both dsim3 and all engines
            let host_has_comp = !self.dsim3_comp_queue.is_empty();
            let mut eng_has_comp = false;
            for (_, eng) in &self.engines {
                eng_has_comp |= eng.host_has_complete();
            }
            host_has_comp | eng_has_comp
        }
    }

    pub fn canAccept(&mut self, addr: u64, is_write: bool) -> bool {
        let cfg = self.get_engine_cfg(addr);
        if let Some(cfg_) = cfg {
            self.engines
                .get_mut(&cfg_)
                .expect("can Accept(): Cannot find correspoding CFG")
                .host_has_complete()
        } else {
            !self.dsim3_comp_queue.is_empty()
        }
    }

    pub fn tick(&mut self) {
        //TODO
        //For host dsim3, tick it in host thread
        //For multiple engines, spawn a new thread to tick() them
        if let SimMode::Host = self.sim_mode {
            // call self.dsim3.tick() and obtain finished req
            self.dsim3_comp_queue.extend(self.dsim3.ClockTick());
        } else {
            // call each engine.tick() plus self.dsim3.tick() in multi thread
        }
    }
}
