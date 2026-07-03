/*
 * Integration Notes:
 * This is Highest level of simulator
 * This simulator will eventually hooked up with gem5
 * Here are some functions required by gem5:
 *      void printStats();
 *      void resetStats();
 *
 *      bool canAccept(uint64_t addr, bool is_write) const; //Done
 *      void enqueue(uint64_t addr, bool is_write); //Done
 *      void enqueue_with_data(uint64_t addr, cacheline payload, bool is_write);
 *
 *      double clockPeriod() const;
 *      unsigned int queueSize() const;
 *      unsigned int burstSize() const;
 *
 *      bool hasComplete() const;           // return true if any request from gem5 has completed,
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
 *
 * gem5 side already had ffi headers implemented. wrapper is not using it rn as rust side haven't
 * done yet but it's all ready
 */

use crate::dsim3_paths;
use crate::memory::dramsim3_wrapper::dramsim3_wrapper;
use crate::memory::mem_portal::{cacheline_payload, dram_req};
use crate::sim_engine::engine::{Engine, EngineRequest, EngineSchedulingMode};
use crate::sim_engine::engine_alloc::engine_alloc;
use crate::sim_engine::request_router::{decode_pim_cmd, pim_cmd};
use crate::sim_engine::timing_harness::timing_harness;
use std::collections::HashMap;

const PIM_CMD_PAYLOAD_SIZE_BYTES: u32 = std::mem::size_of::<u64>() as u32;

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
    engine_comp_queue: Vec<dram_req>,
    immediate_complete_next: Vec<dram_req>,
    immediate_complete_ready: Vec<dram_req>,
    pending_pim_cmds: HashMap<u64, (dram_req, usize)>,
    harness: timing_harness,
    sim_mode: SimMode,
    allocator: engine_alloc,
    //Preset of Engine scheduling mode for allocated CGO engine
    cgo_alloc_scheduling_mode: EngineSchedulingMode,
}

impl Sim {
    pub fn new() -> Self {
        let (cfg_path, out_dir) = dsim3_paths();
        let mut dsim3_inst = dramsim3_wrapper::new(cfg_path, out_dir, 0, 0, 0, 0);
        dsim3_inst.SetPimMode(false); //Set dsim3 as non-pim as it handle normal traces
        let allocator = engine_alloc::new(0..1, 0..1, 0..1, 1..3);

        Self {
            engines: HashMap::new(),
            dsim3: dsim3_inst,
            dsim3_comp_queue: Vec::new(),
            engine_comp_queue: Vec::new(),
            immediate_complete_next: Vec::new(),
            immediate_complete_ready: Vec::new(),
            pending_pim_cmds: HashMap::new(),
            harness: timing_harness::new(),
            sim_mode: SimMode::Pim,
            allocator,
            cgo_alloc_scheduling_mode: EngineSchedulingMode::Host_CGO_share,
        }
    }

    pub fn add_engines(&mut self, cfg: engine_cfg) {
        match self.engines.entry(cfg) {
            std::collections::hash_map::Entry::Occupied(_) => {
                panic!("Cannot add engine with given cfg: already existed");
            }
            std::collections::hash_map::Entry::Vacant(ent) => {
                let engine = match cfg {
                    engine_cfg::CGO { ch, ra, bg, ba } => Engine::new_cgo_at(ch, ra, bg, ba),
                    engine_cfg::FGO { ch, ra, bg, ba } => Engine::new_fgo_at(ch, ra, bg, ba),
                };
                ent.insert(engine);
            }
        }
    }

    pub fn set_engine_scheduling_mode(
        &mut self,
        cfg: engine_cfg,
        scheduling_mode: EngineSchedulingMode,
    ) -> Result<(), &'static str> {
        self.engines
            .get_mut(&cfg)
            .ok_or("cannot configure scheduling for an engine that does not exist")?
            .set_scheduling_mode(scheduling_mode)
    }

    pub fn set_cgo_alloc_scheduling_mode(
        &mut self,
        scheduling_mode: EngineSchedulingMode,
    ) -> Result<(), &'static str> {
        match scheduling_mode {
            EngineSchedulingMode::CGO_only | EngineSchedulingMode::Host_CGO_share => {
                self.cgo_alloc_scheduling_mode = scheduling_mode;
                Ok(())
            }
            _ => Err("CGO allocation scheduling mode must target CGO engines"),
        }
    }

    pub fn clock_period(&mut self) -> f64 {
        self.dsim3.get_TCK()
    }

    pub fn queue_size(&mut self) -> u32 {
        self.dsim3.get_queue_size().max(0) as u32
    }

    pub fn burst_size(&mut self) -> u32 {
        let bus_bytes = self.dsim3.get_bus_bits().max(0) as u32 / 8;
        let burst_length = self.dsim3.get_burst_length().max(0) as u32;
        bus_bytes.saturating_mul(burst_length)
    }

    pub fn canAccept(&mut self, addr: u64, is_write: bool) -> bool {
        let decoded_cmd = decode_pim_cmd(addr, &[0; 8]);
        let request = EngineRequest {
            addr,
            is_write,
            decoded_cmd,
        };

        match decoded_cmd {
            Ok(Some(cmd)) => self.can_accept_pim_cmd(request, cmd),
            Ok(None) => self.can_accept_regular_memory(request),
            Err(_) => true,
        }
    }

    fn can_accept_pim_cmd(&mut self, request: EngineRequest, cmd: pim_cmd) -> bool {
        if !matches!(self.sim_mode, SimMode::Pim)
            || cmd.expects_write() != request.is_write
            || matches!(
                cmd,
                pim_cmd::Ctrl_CGO_Alloc { .. } | pim_cmd::Ctrl_FGO_Alloc { .. }
            )
        {
            return true;
        }

        self.engines.values_mut().all(|engine| {
            !engine.can_accept_pim_cmd(cmd, request.is_write) || engine.canAccept(request)
        })
    }

    fn can_accept_regular_memory(&mut self, request: EngineRequest) -> bool {
        if matches!(self.sim_mode, SimMode::Pim) {
            if let Some(cfg) = self.get_engine_cfg(request.addr) {
                let engine = self
                    .engines
                    .get_mut(&cfg)
                    .expect("mapped engine must exist");
                if !engine.accepts_host_mem_requests() {
                    return true;
                }

                return engine.canAccept(request)
                    && self
                        .dsim3
                        .WillAcceptTransaction(request.addr, request.is_write);
            }
        }

        self.dsim3
            .WillAcceptTransaction(request.addr, request.is_write)
    }

    // This function returns None when no enabled engine owns this address.
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

    pub fn enqueue_with_data(
        &mut self,
        addr: u64,
        payload: cacheline_payload,
        payload_sz_bytes: u32,
        is_write: bool,
    ) {
        let req = dram_req::new_with_payload(addr, payload, !is_write, false);
        let decoded_cmd = decode_pim_cmd(addr, &payload);

        if payload_sz_bytes != PIM_CMD_PAYLOAD_SIZE_BYTES && !matches!(decoded_cmd, Ok(None)) {
            self.enqueue_next_cycle_completion(req);
            return;
        }

        match decoded_cmd {
            Ok(Some(cmd)) if !matches!(self.sim_mode, SimMode::Pim) => {
                eprintln!("warning: ignoring PIM command while Sim is in host mode");
                self.enqueue_next_cycle_completion(req);
            }
            Ok(Some(cmd)) if cmd.expects_write() != is_write => {
                eprintln!("warning: ignoring PIM command with invalid access direction");
                self.enqueue_next_cycle_completion(req);
            }
            Ok(Some(cmd @ (pim_cmd::Ctrl_CGO_Alloc { .. } | pim_cmd::Ctrl_FGO_Alloc { .. }))) => {
                self.enqueue_sim_control_cmd(req, cmd);
            }
            Ok(Some(cmd)) => self.enqueue_pim_cmd(req, cmd),
            Ok(None) => {
                self.enqueue_regular_memory(req, payload_sz_bytes);
            }
            Err(err) => {
                eprintln!("warning: ignoring invalid PIM command at {addr:#x}: {err}");
                self.enqueue_next_cycle_completion(req);
            }
        }
    }

    fn enqueue_sim_control_cmd(&mut self, mut req: dram_req, cmd: pim_cmd) {
        let (allocated, scheduling_mode) = match cmd {
            pim_cmd::Ctrl_CGO_Alloc { asid } => (
                self.allocator.alloc_cgo(asid),
                self.cgo_alloc_scheduling_mode,
            ),
            pim_cmd::Ctrl_FGO_Alloc { asid } => (
                self.allocator.alloc_fgo(asid),
                EngineSchedulingMode::Host_FGO_share,
            ),
            _ => panic!("unsupported simulator control command"),
        };

        let allocated_count = allocated.len();
        for cfg in allocated {
            if !self.engines.contains_key(&cfg) {
                self.add_engines(cfg);
                self.set_engine_scheduling_mode(cfg, scheduling_mode)
                    .expect("allocated engine should accept default scheduling mode");
            }
        }

        //This payload set the number of allocated engine, so it's fine
        req.set_payload_word0(allocated_count as u64);
        self.enqueue_next_cycle_completion(req);
    }

    fn enqueue_pim_cmd(&mut self, mut req: dram_req, cmd: pim_cmd) {
        req.set_id(self.dsim3.get_req_id());
        req.set_issue_time(self.dsim3.get_clock_tick() as u64);
        let request = EngineRequest {
            addr: req.get_addr(),
            is_write: !req.is_read(),
            decoded_cmd: Ok(Some(cmd)),
        };

        let mut target_count = 0;
        let req_id = req
            .get_id()
            .expect("PIM command must have an id before fan-out");
        for (cfg, engine) in self.engines.iter_mut() {
            if engine.can_accept_pim_cmd(cmd, request.is_write) {
                if !engine.canAccept(request) {
                    panic!("PIM command target engine cannot accept the request");
                }
                engine.enqueue_host_pim_request(req.clone(), cmd);
                if let pim_cmd::FGO(instruction) = cmd {
                    self.harness
                        .log_FGO_receive(*cfg, engine.clock_cycle(), req_id, instruction);
                }
                target_count += 1;
            }
        }

        if target_count == 0 {
            eprintln!("warning: PIM command has no initialized compatible engine");
            self.immediate_complete_next.push(req);
            return;
        }

        if matches!(cmd, pim_cmd::CGO_Query) {
            req.set_payload_word0(1);
        }
        assert!(
            self.pending_pim_cmds
                .insert(req_id, (req, target_count))
                .is_none(),
            "duplicate pending PIM command request id"
        );
    }

    fn collect_engine_completion(&mut self) {
        for (cfg, engine) in self.engines.iter_mut() {
            while let Some(req) = engine.get_host_complete() {
                let cmd = match decode_pim_cmd(req.get_addr(), req.get_payload()) {
                    Ok(Some(cmd @ (pim_cmd::FGO(_) | pim_cmd::CGO_Start | pim_cmd::CGO_Query))) => {
                        Some(cmd)
                    }
                    _ => None,
                };

                let Some(cmd) = cmd else {
                    self.engine_comp_queue.push(req);
                    continue;
                };

                let req_id = req
                    .get_id()
                    .expect("PIM command completion must carry a request id");
                if let pim_cmd::FGO(instruction) = cmd {
                    let cycle = engine.clock_cycle();
                    if let crate::PE::types::inst::ST128 { addr, .. } = instruction {
                        self.harness.log_FGO_result(
                            *cfg,
                            cycle,
                            req_id,
                            addr,
                            engine.harness_read_FGO_vector(addr),
                        );
                    }
                    self.harness
                        .log_FGO_retire(*cfg, cycle, req_id, instruction);
                }
                let is_drained = {
                    let (pending_req, remaining) = self
                        .pending_pim_cmds
                        .get_mut(&req_id)
                        .expect("PIM command completion must have a pending request");
                    assert!(*remaining > 0, "PIM command completed too many times");

                    if matches!(cmd, pim_cmd::CGO_Query) {
                        let all_finished =
                            pending_req.get_payload()[0] != 0 && req.get_payload()[0] != 0;
                        pending_req.set_payload_word0(all_finished as u64);
                    }

                    *remaining -= 1;
                    *remaining == 0
                };

                if is_drained {
                    let (completed_req, _) = self
                        .pending_pim_cmds
                        .remove(&req_id)
                        .expect("drained PIM command must still be pending");
                    self.engine_comp_queue.push(completed_req);
                }
            }
        }
    }

    fn enqueue_regular_memory(&mut self, mut req: dram_req, payload_sz_bytes: u32) {
        if let SimMode::Pim = self.sim_mode {
            if let Some(cfg) = self.get_engine_cfg(req.get_addr()) {
                let engine = self
                    .engines
                    .get_mut(&cfg)
                    .expect("Cannot detect available engine");
                if !engine.accepts_host_mem_requests() {
                    eprintln!("warning: ignoring host memory request to a PIM-only engine");
                    self.enqueue_next_cycle_completion(req);
                    return;
                }
                if !req.is_read()
                    && payload_sz_bytes as usize == std::mem::size_of::<cacheline_payload>()
                {
                    engine.mirror_host_write(req.get_addr(), req.get_payload());
                }
                engine.enqueue_host_mem_request(req.clone());
            }
        }

        req.set_id(self.dsim3.get_req_id());
        req.set_issue_time(self.dsim3.get_clock_tick() as u64);

        // Always push into host dsim so host dsim3 will maintain valid state after PIM simulation.
        self.dsim3.AddTransactionReq(req);
    }

    fn enqueue_next_cycle_completion(&mut self, mut req: dram_req) {
        req.set_id(self.dsim3.get_req_id());
        req.set_issue_time(self.dsim3.get_clock_tick() as u64);
        self.immediate_complete_next.push(req);
    }

    pub fn hasComplete(&self) -> bool {
        if !self.immediate_complete_ready.is_empty() {
            return true;
        }

        if let SimMode::Host = self.sim_mode {
            return !self.dsim3_comp_queue.is_empty();
        }

        !self.dsim3_comp_queue.is_empty() || !self.engine_comp_queue.is_empty()
    }

    pub fn getComplete(&mut self) -> Option<dram_req> {
        if let Some(req) = self.immediate_complete_ready.pop() {
            return Some(req);
        }

        if let Some(req) = self.dsim3_comp_queue.pop() {
            return Some(req);
        }

        if let Some(req) = self.engine_comp_queue.pop() {
            return Some(req);
        }

        None
    }

    pub fn tick(&mut self) {
        let completed = self.dsim3.ClockTick();

        if let SimMode::Host = self.sim_mode {
            self.dsim3_comp_queue.extend(completed);
            self.immediate_complete_ready
                .append(&mut self.immediate_complete_next);
            return;
        }

        std::thread::scope(|scope| {
            for engine in self.engines.values_mut() {
                scope.spawn(move || engine.tick());
            }
        });
        self.collect_engine_completion();

        // In PESIM mode, mapped host completions come from engines. Keep only
        // regular DRAM completions from mono_dsim3.
        for req in completed {
            if self.get_engine_cfg(req.get_addr()).is_none() {
                self.dsim3_comp_queue.push(req);
            }
        }

        self.immediate_complete_ready
            .append(&mut self.immediate_complete_next);
    }
}

#[cfg(test)]
#[path = "sim_test.rs"]
mod sim_test;
