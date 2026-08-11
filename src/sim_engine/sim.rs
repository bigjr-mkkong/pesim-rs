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

use crate::memory::dramsim3_wrapper::dramsim3_wrapper;
use crate::memory::mem_portal::{cacheline_payload, dram_req};
use crate::sim_engine::engine::{Engine, EngineRequest, EngineSchedulingMode};
use crate::sim_engine::engine_alloc::{
    PHY_BANK_SZ, PSEUDO_BANK_CACHELINES, PSEUDO_BANKS_PER_LOGICAL_BANK, engine_alloc,
    pseudo_bank_location,
};
use crate::sim_engine::request_router::{decode_pim_cmd, pim_cmd, validate_pim_cmd_access};
use crate::sim_engine::timing_harness::timing_harness;
use rayon::ThreadPool;
use rayon::ThreadPoolBuilder;
use rayon::prelude::*;
use std::collections::HashMap;
use std::path::PathBuf;

const PIM_CMD_PAYLOAD_SIZE_BYTES: u32 = std::mem::size_of::<u64>() as u32;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum engine_cfg {
    CGO {
        ch: u64,
        ra: u64,
        bg: u64,
        ba: u64,
        pb: u64,
    },
    FGO {
        ch: u64,
        ra: u64,
        bg: u64,
        ba: u64,
        pb: u64,
    },
}

impl engine_cfg {
    fn other_processor(self) -> Self {
        match self {
            engine_cfg::CGO { ch, ra, bg, ba, pb } => engine_cfg::FGO { ch, ra, bg, ba, pb },
            engine_cfg::FGO { ch, ra, bg, ba, pb } => engine_cfg::CGO { ch, ra, bg, ba, pb },
        }
    }
}

pub enum SimMode {
    Host,
    Pim,
}

pub struct SimConfig {
    pub config_file: PathBuf,
    pub output_dir: PathBuf,
    pub controller_id: u32,
    pub controller_base: u64,
    pub controller_size: u64,
    pub pim_size: u64,
}

pub struct Sim {
    engines: HashMap<engine_cfg, Engine>,
    dsim3: dramsim3_wrapper,
    dsim3_comp_queue: Vec<dram_req>,
    engine_comp_queue: Vec<dram_req>,
    immediate_complete_next: Vec<dram_req>,
    immediate_complete_ready: Vec<dram_req>,
    pending_pim_cmds: HashMap<u64, PendingPimCmd>,
    harness: timing_harness,
    sim_mode: SimMode,
    allocator: engine_alloc,
    //Preset of Engine scheduling mode for allocated CGO engine
    cgo_alloc_scheduling_mode: EngineSchedulingMode,
    fgo_alloc_scheduling_mode: EngineSchedulingMode,
    engine_tick_pool: Option<ThreadPool>,
    engine_tick_pool_threads: usize,
    controller_id: u32,
    controller_base: u64,
    controller_size: u64,
    config_file: PathBuf,
    output_dir: PathBuf,
}

struct PendingPimCmd {
    request: dram_req,
    command: pim_cmd,
    remaining: usize,
    emit_completion: bool,
}

impl Sim {
    #[cfg(test)]
    pub fn new() -> Self {
        let (config_file, output_dir) =
            crate::dsim3_paths(crate::PIM_DSIM3_CFG_PATH, crate::DSIM3_OUT_DIR);
        Self::from_config(SimConfig {
            config_file,
            output_dir,
            controller_id: 1,
            controller_base: 0,
            controller_size: 8 * 1024 * 1024 * 1024,
            pim_size: 2 * 1024 * 1024 * 1024,
        })
    }

    pub fn from_config(config: SimConfig) -> Self {
        config
            .controller_base
            .checked_add(config.controller_size)
            .expect("controller address range overflow");

        let mut dsim3_inst = dramsim3_wrapper::new_with_address_base(
            &config.config_file,
            &config.output_dir,
            0,
            0,
            0,
            0,
            config.controller_base,
        );
        let modeled_capacity = dsim3_inst.get_capacity_bytes();
        assert!(
            modeled_capacity >= config.controller_size,
            "DRAMSim3 capacity {modeled_capacity} is smaller than controller range {}",
            config.controller_size
        );
        assert_eq!(
            dsim3_inst.get_channels(),
            1,
            "each gem5 PESim controller must use a one-channel DRAMSim3 configuration"
        );

        let pim_enabled = config.pim_size > 0;
        dsim3_inst.SetPimMode(false);

        let allocator = if pim_enabled {
            assert!(
                config.pim_size > 0,
                "PIM-enabled controller needs a nonzero PIM region"
            );
            assert_eq!(
                config.pim_size % PHY_BANK_SZ,
                0,
                "PIM region must be 64 MiB engine aligned"
            );
            assert!(
                config.pim_size <= config.controller_size,
                "PIM region cannot exceed the controller range"
            );
            let mut locations = Vec::new();
            let mut offset = 0;
            while offset < config.pim_size {
                let guest_addr = config
                    .controller_base
                    .checked_add(offset)
                    .expect("PIM address overflow");
                let decoded = dsim3_inst.global_addr_to_local_components(guest_addr);
                let pb = decoded.bank_local_addr / PSEUDO_BANK_CACHELINES;
                assert!(
                    pb < PSEUDO_BANKS_PER_LOGICAL_BANK,
                    "decoded pseudo-bank index is outside one logical bank"
                );
                let location = pseudo_bank_location {
                    ch: decoded.channel,
                    ra: decoded.rank,
                    bg: decoded.bank_group,
                    ba: decoded.bank,
                    pb,
                };
                assert!(
                    !locations.iter().any(|existing: &pseudo_bank_location| {
                        existing.ch == location.ch
                            && existing.ra == location.ra
                            && existing.bg == location.bg
                            && existing.ba == location.ba
                            && existing.pb == location.pb
                    }),
                    "PIM prefix mapped two addresses to the same engine"
                );
                locations.push(location);
                offset += PHY_BANK_SZ;
            }
            engine_alloc::from_pseudo_banks(locations)
        } else {
            engine_alloc::from_pseudo_banks(Vec::new())
        };

        Self {
            engines: HashMap::new(),
            dsim3: dsim3_inst,
            dsim3_comp_queue: Vec::new(),
            engine_comp_queue: Vec::new(),
            immediate_complete_next: Vec::new(),
            immediate_complete_ready: Vec::new(),
            pending_pim_cmds: HashMap::new(),
            harness: timing_harness::new(config.controller_id),
            sim_mode: if pim_enabled {
                SimMode::Pim
            } else {
                SimMode::Host
            },
            allocator,
            cgo_alloc_scheduling_mode: if std::env::var("PIM_SCENARIO").as_deref()
                == Ok("sequential")
            {
                EngineSchedulingMode::Sequential
            } else {
                EngineSchedulingMode::Host_CGO_share
            },
            fgo_alloc_scheduling_mode: if std::env::var("PIM_SCENARIO").as_deref()
                == Ok("sequential")
            {
                EngineSchedulingMode::Sequential
            } else {
                EngineSchedulingMode::Host_FGO_share
            },
            engine_tick_pool: None,
            engine_tick_pool_threads: 0,
            controller_id: config.controller_id,
            controller_base: config.controller_base,
            controller_size: config.controller_size,
            config_file: config.config_file,
            output_dir: config.output_dir,
        }
    }

    pub fn add_engines(&mut self, cfg: engine_cfg) {
        if self.engines.contains_key(&cfg.other_processor()) {
            panic!("Cannot add engine: pseudo bank is already owned by another processor type");
        }

        match self.engines.entry(cfg) {
            std::collections::hash_map::Entry::Occupied(_) => {
                panic!("Cannot add engine with given cfg: already existed");
            }
            std::collections::hash_map::Entry::Vacant(ent) => {
                let engine = match cfg {
                    engine_cfg::CGO { ch, ra, bg, ba, pb } => Engine::new_cgo_configured(
                        self.controller_id,
                        self.controller_base,
                        &self.config_file,
                        &self.output_dir,
                        ch,
                        ra,
                        bg,
                        ba,
                        pb,
                    ),
                    engine_cfg::FGO { ch, ra, bg, ba, pb } => Engine::new_fgo_configured(
                        self.controller_id,
                        self.controller_base,
                        &self.config_file,
                        &self.output_dir,
                        ch,
                        ra,
                        bg,
                        ba,
                        pb,
                    ),
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
            EngineSchedulingMode::CGO_only
            | EngineSchedulingMode::Host_CGO_share
            | EngineSchedulingMode::Sequential => {
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

    pub fn print_cgo_switch_stats(&self) {
        let mut records = self
            .engines
            .iter()
            .filter_map(|(cfg, engine)| engine.cgo_switch_stats().map(|stats| (*cfg, stats)))
            .collect::<Vec<_>>();
        records.sort_by_key(|(cfg, _)| match cfg {
            engine_cfg::CGO { ch, ra, bg, ba, pb } | engine_cfg::FGO { ch, ra, bg, ba, pb } => {
                (*ch, *ra, *bg, *ba, *pb)
            }
        });

        for (cfg, stats) in records {
            let engine_cfg::CGO { ch, ra, bg, ba, pb } = cfg else {
                unreachable!("only CGO engines expose CGO switch statistics");
            };
            for (direction, direction_stats) in [
                ("PIM_TO_HOST", stats.pim_to_host),
                ("HOST_TO_PIM", stats.host_to_pim),
            ] {
                println!(
                    "CGO_SWITCH_TIMING controller={} dram_channel={ch} rank={ra} bank_group={bg} bank={ba} pseudo_bank={pb} direction={direction} requests={} commits={} cancellations={} source_quiesce_cycles={} promoted_drain_cycles={} fixed_delay_cycles={} commit_guard_cycles={} total_cycles={} parked_transactions_total={} parked_transactions_max={} promoted_transactions_total={} promoted_transactions_max={}",
                    self.controller_id,
                    direction_stats.requests,
                    direction_stats.commits,
                    direction_stats.cancellations,
                    direction_stats.source_quiesce_cycles,
                    direction_stats.promoted_drain_cycles,
                    direction_stats.fixed_delay_cycles,
                    direction_stats.commit_guard_cycles,
                    direction_stats.total_cycles(),
                    direction_stats.parked_transactions_total,
                    direction_stats.parked_transactions_max,
                    direction_stats.promoted_transactions_total,
                    direction_stats.promoted_transactions_max,
                );
            }
        }
    }

    fn contains_addr(&self, addr: u64) -> bool {
        addr >= self.controller_base && addr - self.controller_base < self.controller_size
    }

    #[cfg(test)]
    fn configured_engine_count_for_test(&self) -> usize {
        self.allocator.configured_engine_count()
    }

    pub fn canAccept(&mut self, addr: u64, is_write: bool) -> bool {
        if !self.contains_addr(addr) {
            return false;
        }
        let request = EngineRequest {
            addr,
            is_write,
            decoded_cmd: Ok(None),
        };

        self.can_accept_regular_memory(request)
    }

    fn can_accept_pim_cmd(&mut self, request: EngineRequest, cmd: pim_cmd) -> bool {
        if !matches!(self.sim_mode, SimMode::Pim)
            || cmd.expects_write() != request.is_write
            || matches!(
                cmd,
                pim_cmd::PIM_Query
                    | pim_cmd::Ctrl_CGO_Alloc { .. }
                    | pim_cmd::Ctrl_FGO_Alloc { .. }
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
        let pb = addr_bulk.bank_local_addr / PSEUDO_BANK_CACHELINES;
        if pb >= PSEUDO_BANKS_PER_LOGICAL_BANK {
            return None;
        }
        let cgo_cfg = engine_cfg::CGO {
            ch: addr_bulk.channel,
            ra: addr_bulk.rank,
            bg: addr_bulk.bank_group,
            ba: addr_bulk.bank,
            pb,
        };
        let fgo_cfg = engine_cfg::FGO {
            ch: addr_bulk.channel,
            ra: addr_bulk.rank,
            bg: addr_bulk.bank_group,
            ba: addr_bulk.bank,
            pb,
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
        assert!(
            self.contains_addr(addr),
            "request address {addr:#x} is outside controller {} range",
            self.controller_id
        );
        self.enqueue_regular_memory(req, payload_sz_bytes);
    }

    pub fn canAcceptPimCmd(
        &mut self,
        offset: u64,
        payload: cacheline_payload,
        payload_sz_bytes: u32,
    ) -> bool {
        self.canAcceptPimCmdAccess(offset, payload, payload_sz_bytes, true)
    }

    pub fn canAcceptPimCmdAccess(
        &mut self,
        offset: u64,
        payload: cacheline_payload,
        payload_sz_bytes: u32,
        is_write: bool,
    ) -> bool {
        self.decode_pim_cmd_request(offset, &payload, payload_sz_bytes, is_write)
            .map(|cmd| {
                let request = EngineRequest {
                    addr: offset,
                    is_write,
                    decoded_cmd: Ok(Some(cmd)),
                };
                self.can_accept_pim_cmd(request, cmd)
            })
            .unwrap_or(false)
    }

    pub fn enqueuePimCmd(
        &mut self,
        offset: u64,
        payload: cacheline_payload,
        payload_sz_bytes: u32,
    ) -> bool {
        self.enqueue_pim_cmd_request(offset, payload, payload_sz_bytes, true, false)
    }

    pub fn enqueuePimCmdAccess(
        &mut self,
        offset: u64,
        payload: cacheline_payload,
        payload_sz_bytes: u32,
        is_write: bool,
    ) -> bool {
        self.enqueue_pim_cmd_request(offset, payload, payload_sz_bytes, is_write, !is_write)
    }

    fn decode_pim_cmd_request(
        &self,
        offset: u64,
        payload: &cacheline_payload,
        payload_sz_bytes: u32,
        is_write: bool,
    ) -> Result<pim_cmd, &'static str> {
        if !matches!(self.sim_mode, SimMode::Pim) {
            return Err("PIM command sent to a non-PIM controller");
        }
        if payload_sz_bytes != PIM_CMD_PAYLOAD_SIZE_BYTES {
            return Err("PIM command payload must be eight bytes");
        }

        let cmd = decode_pim_cmd(offset, payload)?;
        validate_pim_cmd_access(cmd, is_write)?;
        Ok(cmd)
    }

    fn enqueue_pim_cmd_request(
        &mut self,
        offset: u64,
        payload: cacheline_payload,
        payload_sz_bytes: u32,
        is_write: bool,
        emit_completion: bool,
    ) -> bool {
        let cmd = match self.decode_pim_cmd_request(offset, &payload, payload_sz_bytes, is_write) {
            Ok(cmd) => cmd,
            Err(err) => {
                eprintln!(
                    "warning: ignoring invalid PIM MMIO command at offset {offset:#x}: {err}"
                );
                return false;
            }
        };
        let mut req = dram_req::new_with_payload(offset, payload, !is_write, false);
        req.mark_host_pim_command();

        match cmd {
            pim_cmd::PIM_Query => {
                let mut req = req;
                let (total, finished) = self.pim_progress();
                req.set_payload_word0((u64::from(total) << 32) | u64::from(finished));
                if emit_completion {
                    self.enqueue_next_cycle_completion(req);
                }
            }
            cmd @ (pim_cmd::Ctrl_CGO_Alloc { .. } | pim_cmd::Ctrl_FGO_Alloc { .. }) => {
                self.enqueue_sim_control_cmd(req, cmd, emit_completion);
            }
            cmd => self.enqueue_decoded_pim_cmd(req, cmd, emit_completion),
        }
        true
    }

    fn pim_progress(&self) -> (u32, u32) {
        let total = u32::try_from(self.engines.len()).expect("PIM engine count exceeds u32");
        let finished = u32::try_from(
            self.engines
                .values()
                .filter(|engine| engine.reached_final_barrier())
                .count(),
        )
        .expect("finished PIM engine count exceeds u32");
        (total, finished)
    }

    #[cfg(test)]
    fn enqueue_pim_cmd_with_completion_for_test(
        &mut self,
        offset: u64,
        payload: cacheline_payload,
        payload_sz_bytes: u32,
        is_write: bool,
    ) -> bool {
        self.enqueue_pim_cmd_request(offset, payload, payload_sz_bytes, is_write, true)
    }

    fn enqueue_sim_control_cmd(&mut self, mut req: dram_req, cmd: pim_cmd, emit_completion: bool) {
        let (allocated, scheduling_mode) = match cmd {
            pim_cmd::Ctrl_CGO_Alloc { asid } => (
                self.allocator.alloc_cgo(asid),
                self.cgo_alloc_scheduling_mode,
            ),
            pim_cmd::Ctrl_FGO_Alloc { asid } => (
                self.allocator.alloc_fgo(asid),
                self.fgo_alloc_scheduling_mode,
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

        if emit_completion {
            req.set_payload_word0(allocated_count as u64);
            self.enqueue_next_cycle_completion(req);
        }
    }

    fn enqueue_decoded_pim_cmd(&mut self, mut req: dram_req, cmd: pim_cmd, emit_completion: bool) {
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
                if matches!(cmd, pim_cmd::CGO_Start) {
                    self.harness
                        .log_CGO_start(*cfg, engine.clock_cycle(), req_id);
                }
                target_count += 1;
            }
        }

        if target_count == 0 {
            eprintln!("warning: PIM command has no initialized compatible engine");
            if emit_completion {
                self.immediate_complete_next.push(req);
            }
            return;
        }

        assert!(
            self.pending_pim_cmds
                .insert(
                    req_id,
                    PendingPimCmd {
                        request: req,
                        command: cmd,
                        remaining: target_count,
                        emit_completion,
                    },
                )
                .is_none(),
            "duplicate pending PIM command request id"
        );
    }

    fn collect_engine_completion(&mut self) {
        for (cfg, engine) in self.engines.iter_mut() {
            if self.harness.is_tracking_CGO(*cfg) && engine.harness_CGO_finished() {
                self.harness.log_CGO_finish(
                    *cfg,
                    engine.clock_cycle().saturating_sub(1),
                    engine.harness_read_CGO_outputs(),
                );
            }

            while let Some(req) = engine.get_host_complete() {
                // Engine-local DRAM request IDs and simulator-level PIM
                // command IDs come from independent counters and may have
                // the same numeric value.  Only the explicit host-command
                // token can retire a broadcast command; ordinary mapped
                // host traffic must remain an ordinary engine completion.
                if !req.is_host_pim_command() {
                    self.engine_comp_queue.push(req);
                    continue;
                }
                let Some(req_id) = req.get_id() else {
                    self.engine_comp_queue.push(req);
                    continue;
                };
                let Some(pending) = self.pending_pim_cmds.get(&req_id) else {
                    self.engine_comp_queue.push(req);
                    continue;
                };
                let cmd = pending.command;
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
                    let pending = self
                        .pending_pim_cmds
                        .get_mut(&req_id)
                        .expect("PIM command completion must have a pending request");
                    assert!(
                        pending.remaining > 0,
                        "PIM command completed too many times"
                    );

                    pending.remaining -= 1;
                    pending.remaining == 0
                };

                if is_drained {
                    let completed = self
                        .pending_pim_cmds
                        .remove(&req_id)
                        .expect("drained PIM command must still be pending");
                    if completed.emit_completion {
                        self.engine_comp_queue.push(completed.request);
                    }
                }
            }
        }
    }

    fn engine_tick_worker_count(&self) -> usize {
        let engine_count = self.engines.len();
        if engine_count == 0 {
            return 0;
        }

        std::thread::available_parallelism()
            .map(usize::from)
            .unwrap_or(1)
            .min(engine_count)
    }

    fn ensure_engine_tick_pool(&mut self) {
        let worker_count = self.engine_tick_worker_count();
        if worker_count == 0 || self.engine_tick_pool_threads == worker_count {
            return;
        }

        self.engine_tick_pool = Some(
            ThreadPoolBuilder::new()
                .num_threads(worker_count)
                .thread_name(|idx| format!("pesim-engine-tick-{idx}"))
                .build()
                .unwrap_or_else(|err| panic!("cannot build engine tick thread pool: {err}")),
        );
        self.engine_tick_pool_threads = worker_count;
    }

    fn tick_engines_parallel(&mut self) {
        if self.engines.is_empty() {
            return;
        }

        self.ensure_engine_tick_pool();
        let Self {
            engines,
            engine_tick_pool,
            ..
        } = self;
        let pool = engine_tick_pool
            .as_ref()
            .expect("engine tick pool must exist when engines are present");

        pool.install(|| {
            engines.par_iter_mut().for_each(|(cfg, engine)| {
                let cfg = *cfg;
                if let Err(payload) =
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| engine.tick()))
                {
                    eprintln!("PIM_ERROR reason=engine_tick_panic cfg={cfg:?}");
                    std::panic::resume_unwind(payload);
                }
            });
        });
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

        self.tick_engines_parallel();
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
