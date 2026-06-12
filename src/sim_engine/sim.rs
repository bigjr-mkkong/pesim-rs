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

use crate::memory::dramsim3_wrapper::dramsim3_wrapper;
use crate::memory::mem_portal::{dram_req, portal_req};
use crate::sim_engine::engine::Engine;
use crate::{DSIM3_CFG_PATH, DSIM3_OUT_DIR};
use std::collections::HashMap;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
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
            std::collections::hash_map::Entry::Vacant(ent) => {
                ent.insert(Engine::new());
            }
        }
    }

    pub fn canAccept(&mut self, addr: u64, is_write: bool) -> bool {
        if let Some(cfg) = self.get_engine_cfg(addr) {
            return self
                .engines
                .get_mut(&cfg)
                .expect("Cannot detect available engine")
                .canAccept(addr, is_write);
        }

        self.dsim3.WillAcceptTransaction(addr, is_write)
    }

    //This function will return None if addr belongs to non-pim area
    fn get_engine_cfg(&mut self, addr: u64) -> Option<engine_cfg> {
        let addr_bulk = self.dsim3.global_addr_to_local_components(addr);
        let cgo_cfg = engine_cfg::CGO {
            ch: addr_bulk.channel,
            ra: addr_bulk.rank,
            bg: addr_bulk.bank_group,
            ba: addr_bulk.bank,
        };
        let fgo_cfg = engine_cfg::FGO {
            ch: addr_bulk.channel,
            ra: addr_bulk.rank,
            bg: addr_bulk.bank_group,
            ba: addr_bulk.bank,
        };

        if self.engines.contains_key(&cgo_cfg) {
            Some(cgo_cfg)
        } else if self.engines.contains_key(&fgo_cfg) {
            Some(fgo_cfg)
        } else {
            None
        }
    }

    pub fn enqueue(&mut self, addr: u64, is_write: bool) {
        let mut req = dram_req::new(addr, !is_write, false);

        if let SimMode::Pim = self.sim_mode {
            if let Some(cfg) = self.get_engine_cfg(addr) {
                self.engines
                    .get_mut(&cfg)
                    .expect("Cannot detect available engine")
                    .host_push_req(portal_req::HOST_REQ { req: req.clone() });
            }
        }

        req.set_id(self.dsim3.get_req_id());
        req.set_issue_time(self.dsim3.get_clock_tick() as u64);

        // Always push into host dsim so host dsim3 will maintain valid state after PIM simulation.
        self.dsim3.AddTransactionReq(req);
    }

    pub fn has_complete(&self) -> bool {
        if let SimMode::Host = self.sim_mode {
            return !self.dsim3_comp_queue.is_empty();
        }

        !self.dsim3_comp_queue.is_empty()
            || self.engines.values().any(|eng| eng.host_has_complete())
    }

    pub fn get_complete(&mut self) -> Option<dram_req> {
        if let Some(req) = self.dsim3_comp_queue.pop() {
            return Some(req);
        }

        if let SimMode::Pim = self.sim_mode {
            for engine in self.engines.values_mut() {
                if let Some(req) = engine.get_host_complete() {
                    return Some(req);
                }
            }
        }

        None
    }

    pub fn tick(&mut self) {
        let completed = self.dsim3.ClockTick();

        if let SimMode::Host = self.sim_mode {
            self.dsim3_comp_queue.extend(completed);
            return;
        }

        std::thread::scope(|scope| {
            for engine in self.engines.values_mut() {
                scope.spawn(move || engine.tick());
            }
        });

        // In PESIM mode, mapped host completions come from engines. Keep only
        // regular DRAM completions from mono_dsim3.
        for req in completed {
            if self.get_engine_cfg(req.get_addr()).is_none() {
                self.dsim3_comp_queue.push(req);
            }
        }
    }
}
