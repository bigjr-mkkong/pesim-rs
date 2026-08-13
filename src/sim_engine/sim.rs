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
use std::collections::HashMap;
use std::ops::{Deref, DerefMut};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;

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

struct EngineEntry {
    cfg: engine_cfg,
    engine: Engine,
}

type EngineShard = Arc<Mutex<Vec<EngineEntry>>>;

/// Reserve part of an SMT machine for the gem5/controller thread and avoid
/// saturating both hardware threads of every physical core.  With two-way SMT,
/// `logical / 2 * 1.6` is exactly 80% of the reported logical parallelism.
fn smt_aware_engine_worker_limit(logical_parallelism: usize) -> usize {
    (logical_parallelism.saturating_mul(4) / 5).max(1)
}

fn controller_engine_worker_limit(
    logical_parallelism: usize,
    controller_id: u32,
    pim_size: u64,
    controller_size: u64,
) -> usize {
    let process_limit = smt_aware_engine_worker_limit(logical_parallelism);
    if pim_size != controller_size {
        return process_limit;
    }

    // The full configuration owns two equally sized PIM controllers. Split
    // the process-wide budget between their persistent pools; controller 0
    // receives the odd worker. The minimum only matters on tiny test hosts.
    let controller_count = 2;
    let controller_index = usize::try_from(controller_id)
        .expect("controller id must fit usize")
        .min(controller_count - 1);
    ((process_limit + controller_count - 1 - controller_index) / controller_count).max(1)
}

enum EngineMut<'a> {
    Collecting(&'a mut Engine),
    Sharded {
        shard: MutexGuard<'a, Vec<EngineEntry>>,
        index: usize,
    },
}

impl Deref for EngineMut<'_> {
    type Target = Engine;

    fn deref(&self) -> &Self::Target {
        match self {
            EngineMut::Collecting(engine) => engine,
            EngineMut::Sharded { shard, index } => &shard[*index].engine,
        }
    }
}

impl DerefMut for EngineMut<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        match self {
            EngineMut::Collecting(engine) => engine,
            EngineMut::Sharded { shard, index } => &mut shard[*index].engine,
        }
    }
}

enum EngineLayout {
    Collecting(HashMap<engine_cfg, Engine>),
    Sharded {
        shards: Vec<EngineShard>,
        locations: HashMap<engine_cfg, (usize, usize)>,
        canonical_order: Vec<engine_cfg>,
    },
}

struct EngineStore {
    layout: EngineLayout,
}

impl EngineStore {
    fn new() -> Self {
        Self {
            layout: EngineLayout::Collecting(HashMap::new()),
        }
    }

    fn len(&self) -> usize {
        match &self.layout {
            EngineLayout::Collecting(engines) => engines.len(),
            EngineLayout::Sharded {
                canonical_order, ..
            } => canonical_order.len(),
        }
    }

    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn contains_key(&self, cfg: &engine_cfg) -> bool {
        match &self.layout {
            EngineLayout::Collecting(engines) => engines.contains_key(cfg),
            EngineLayout::Sharded { locations, .. } => locations.contains_key(cfg),
        }
    }

    fn insert(&mut self, cfg: engine_cfg, engine: Engine) {
        let EngineLayout::Collecting(engines) = &mut self.layout else {
            panic!("cannot add a PIM engine after fixed tick shards are active");
        };
        assert!(
            engines.insert(cfg, engine).is_none(),
            "Cannot add engine with given cfg: already existed"
        );
    }

    fn get_mut(&mut self, cfg: &engine_cfg) -> Option<EngineMut<'_>> {
        match &mut self.layout {
            EngineLayout::Collecting(engines) => engines.get_mut(cfg).map(EngineMut::Collecting),
            EngineLayout::Sharded {
                shards, locations, ..
            } => locations
                .get(cfg)
                .copied()
                .map(|(shard, index)| EngineMut::Sharded {
                    shard: shards[shard]
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner()),
                    index,
                }),
        }
    }

    fn for_each(&self, mut visit: impl FnMut(engine_cfg, &Engine)) {
        match &self.layout {
            EngineLayout::Collecting(engines) => {
                for (&cfg, engine) in engines {
                    visit(cfg, engine);
                }
            }
            EngineLayout::Sharded {
                shards,
                locations,
                canonical_order,
            } => {
                for &cfg in canonical_order {
                    let &(shard, index) = locations
                        .get(&cfg)
                        .expect("sharded engine must have a location");
                    let shard = shards[shard]
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    visit(cfg, &shard[index].engine);
                }
            }
        }
    }

    fn for_each_mut(&mut self, mut visit: impl FnMut(engine_cfg, &mut Engine)) {
        match &mut self.layout {
            EngineLayout::Collecting(engines) => {
                for (&cfg, engine) in engines {
                    visit(cfg, engine);
                }
            }
            EngineLayout::Sharded {
                shards,
                locations,
                canonical_order,
            } => {
                for &cfg in canonical_order.iter() {
                    let &(shard, index) = locations
                        .get(&cfg)
                        .expect("sharded engine must have a location");
                    let mut shard = shards[shard]
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    visit(cfg, &mut shard[index].engine);
                }
            }
        }
    }

    fn all_mut(&mut self, mut predicate: impl FnMut(&mut Engine) -> bool) -> bool {
        let mut result = true;
        self.for_each_mut(|_, engine| {
            if result && !predicate(engine) {
                result = false;
            }
        });
        result
    }

    fn count(&self, mut predicate: impl FnMut(&Engine) -> bool) -> usize {
        let mut count = 0;
        self.for_each(|_, engine| {
            if predicate(engine) {
                count += 1;
            }
        });
        count
    }

    fn prepare_fixed_shards(&mut self, worker_limit: usize) -> usize {
        let engine_count = self.len();
        if engine_count == 0 {
            return 0;
        }

        if let EngineLayout::Sharded { shards, .. } = &self.layout {
            return shards.len();
        }

        let worker_count = worker_limit.max(1).min(engine_count);
        self.prepare_fixed_shards_with_worker_count(worker_count);
        worker_count
    }

    fn prepare_fixed_shards_with_worker_count(&mut self, worker_count: usize) {
        assert!(
            worker_count > 0,
            "fixed engine shards need at least one worker"
        );
        let EngineLayout::Collecting(engines) =
            std::mem::replace(&mut self.layout, EngineLayout::Collecting(HashMap::new()))
        else {
            panic!("fixed engine shards may only be prepared once");
        };
        assert!(
            worker_count <= engines.len(),
            "fixed engine shard count cannot exceed engine count"
        );

        // HashMap::into_iter() traverses the same bucket order as iter().  Capture
        // that order before moving engines so command delivery and completion
        // collection retain their pre-sharding traversal semantics.
        let ordered_engines = engines.into_iter().collect::<Vec<_>>();
        let canonical_order = ordered_engines
            .iter()
            .map(|(cfg, _)| *cfg)
            .collect::<Vec<_>>();
        let mut shards = (0..worker_count).map(|_| Vec::new()).collect::<Vec<_>>();
        let mut locations = HashMap::with_capacity(ordered_engines.len());

        for (ordinal, (cfg, engine)) in ordered_engines.into_iter().enumerate() {
            let shard = ordinal % worker_count;
            let index = shards[shard].len();
            shards[shard].push(EngineEntry { cfg, engine });
            assert!(locations.insert(cfg, (shard, index)).is_none());
        }

        self.layout = EngineLayout::Sharded {
            shards: shards
                .into_iter()
                .map(|shard| Arc::new(Mutex::new(shard)))
                .collect(),
            locations,
            canonical_order,
        };
    }

    fn shard_handles(&self) -> Vec<EngineShard> {
        let EngineLayout::Sharded { shards, .. } = &self.layout else {
            panic!("engine tick shards must be prepared before starting workers");
        };
        shards.clone()
    }

    #[cfg(test)]
    fn shard_count(&self) -> usize {
        match &self.layout {
            EngineLayout::Collecting(_) => 0,
            EngineLayout::Sharded { shards, .. } => shards.len(),
        }
    }

    #[cfg(test)]
    fn canonical_cfgs(&self) -> Vec<engine_cfg> {
        match &self.layout {
            EngineLayout::Collecting(engines) => engines.keys().copied().collect(),
            EngineLayout::Sharded {
                canonical_order, ..
            } => canonical_order.clone(),
        }
    }

    #[cfg(test)]
    fn shard_cfgs(&self) -> Vec<Vec<engine_cfg>> {
        match &self.layout {
            EngineLayout::Collecting(_) => Vec::new(),
            EngineLayout::Sharded { shards, .. } => shards
                .iter()
                .map(|shard| {
                    shard
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                        .iter()
                        .map(|entry| entry.cfg)
                        .collect()
                })
                .collect(),
        }
    }
}

struct TickCoordinator {
    generation: AtomicU64,
    remaining_workers: AtomicUsize,
    shutdown: AtomicBool,
    panicked_engine: Mutex<Option<engine_cfg>>,
}

struct EngineTickWorkers {
    coordinator: Arc<TickCoordinator>,
    controller_shard: EngineShard,
    workers: Vec<JoinHandle<()>>,
}

impl EngineTickWorkers {
    fn new(mut shards: Vec<EngineShard>) -> Self {
        assert!(
            !shards.is_empty(),
            "engine workers require at least one shard"
        );
        // The gem5/controller thread executes one shard itself.  An N-way
        // layout therefore creates N-1 helpers and consumes at most N host
        // execution contexts.
        let controller_shard = shards.remove(0);
        let coordinator = Arc::new(TickCoordinator {
            generation: AtomicU64::new(0),
            remaining_workers: AtomicUsize::new(0),
            shutdown: AtomicBool::new(false),
            panicked_engine: Mutex::new(None),
        });

        let workers = shards
            .into_iter()
            .enumerate()
            .map(|(worker_id, shard)| {
                let coordinator = Arc::clone(&coordinator);
                std::thread::Builder::new()
                    .name(format!("pesim-engine-tick-{worker_id}"))
                    .spawn(move || Self::worker_loop(shard, coordinator))
                    .unwrap_or_else(|err| panic!("cannot spawn engine tick worker: {err}"))
            })
            .collect();

        Self {
            coordinator,
            controller_shard,
            workers,
        }
    }

    fn tick_shard(shard: &EngineShard) -> Option<engine_cfg> {
        let mut shard = shard
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        for entry in shard.iter_mut() {
            if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| entry.engine.tick()))
                .is_err()
            {
                eprintln!("PIM_ERROR reason=engine_tick_panic cfg={:?}", entry.cfg);
                return Some(entry.cfg);
            }
        }
        None
    }

    fn record_panic(coordinator: &TickCoordinator, panicked_engine: Option<engine_cfg>) {
        if let Some(cfg) = panicked_engine {
            let mut first_panic = coordinator
                .panicked_engine
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if first_panic.is_none() {
                *first_panic = Some(cfg);
            }
        }
    }

    fn worker_loop(shard: EngineShard, coordinator: Arc<TickCoordinator>) {
        let mut observed_generation = 0;
        loop {
            let mut idle_iterations = 0;
            while coordinator.generation.load(Ordering::Acquire) == observed_generation
                && !coordinator.shutdown.load(Ordering::Acquire)
            {
                if idle_iterations < 256 {
                    std::hint::spin_loop();
                } else if idle_iterations < 288 {
                    std::thread::yield_now();
                } else {
                    std::thread::park();
                }
                idle_iterations += 1;
            }
            if coordinator.shutdown.load(Ordering::Acquire) {
                return;
            }
            observed_generation = coordinator.generation.load(Ordering::Acquire);

            Self::record_panic(&coordinator, Self::tick_shard(&shard));
            let previous = coordinator.remaining_workers.fetch_sub(1, Ordering::AcqRel);
            assert!(
                previous > 0,
                "engine worker completed without an active tick"
            );
        }
    }

    fn tick(&self) {
        assert_eq!(
            self.coordinator.remaining_workers.load(Ordering::Acquire),
            0,
            "cannot start an engine tick while another tick is active"
        );
        *self
            .coordinator
            .panicked_engine
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
        self.coordinator
            .remaining_workers
            .store(self.workers.len(), Ordering::Release);
        self.coordinator
            .generation
            .fetch_update(Ordering::Release, Ordering::Relaxed, |generation| {
                generation.checked_add(1)
            })
            .expect("engine tick generation overflow");
        for worker in &self.workers {
            worker.thread().unpark();
        }

        Self::record_panic(&self.coordinator, Self::tick_shard(&self.controller_shard));

        let mut wait_iterations = 0;
        while self.coordinator.remaining_workers.load(Ordering::Acquire) != 0 {
            if wait_iterations < 256 {
                std::hint::spin_loop();
            } else {
                std::thread::yield_now();
            }
            wait_iterations += 1;
        }
        if let Some(cfg) = *self
            .coordinator
            .panicked_engine
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
        {
            panic!("PIM engine tick panicked: {cfg:?}");
        }
    }

    fn len(&self) -> usize {
        self.workers.len() + 1
    }

    #[cfg(test)]
    fn thread_ids(&self) -> Vec<std::thread::ThreadId> {
        self.workers
            .iter()
            .map(|worker| worker.thread().id())
            .collect()
    }
}

impl Drop for EngineTickWorkers {
    fn drop(&mut self) {
        self.coordinator.shutdown.store(true, Ordering::Release);
        for worker in &self.workers {
            worker.thread().unpark();
        }
        for worker in self.workers.drain(..) {
            worker
                .join()
                .expect("persistent engine tick worker panicked outside engine execution");
        }
    }
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
    engines: EngineStore,
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
    engine_tick_workers: Option<EngineTickWorkers>,
    engine_worker_limit: usize,
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
        let logical_parallelism = std::thread::available_parallelism()
            .map(usize::from)
            .unwrap_or(1);
        let engine_worker_limit = controller_engine_worker_limit(
            logical_parallelism,
            config.controller_id,
            config.pim_size,
            config.controller_size,
        );
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
            engines: EngineStore::new(),
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
            engine_tick_workers: None,
            engine_worker_limit,
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
        if self.engines.contains_key(&cfg) {
            panic!("Cannot add engine with given cfg: already existed");
        }

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
        self.engines.insert(cfg, engine);
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
        let mut records = Vec::new();
        self.engines.for_each(|cfg, engine| {
            if let Some(stats) = engine.cgo_switch_stats() {
                records.push((cfg, stats));
            }
        });
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
                    "CGO_SWITCH_TIMING controller={} dram_channel={ch} rank={ra} bank_group={bg} bank={ba} pseudo_bank={pb} direction={direction} requests={} commits={} cancellations={} source_quiesce_cycles={} promoted_drain_cycles={} fixed_delay_cycles={} commit_guard_cycles={} synchronization_cycles={} synchronization_min_cycles={} synchronization_max_cycles={} synchronization_average_cycles={} state_switch_cycles={} state_switch_min_cycles={} state_switch_max_cycles={} state_switch_average_cycles={} cancelled_synchronization_cycles={} cancelled_state_switch_cycles={} cancelled_handoff_cycles={} total_cycles={} parked_transactions_total={} parked_transactions_max={} promoted_transactions_total={} promoted_transactions_max={}",
                    self.controller_id,
                    direction_stats.requests,
                    direction_stats.commits,
                    direction_stats.cancellations,
                    direction_stats.source_quiesce_cycles,
                    direction_stats.promoted_drain_cycles,
                    direction_stats.fixed_delay_cycles,
                    direction_stats.commit_guard_cycles,
                    direction_stats.synchronization_cycles,
                    direction_stats.synchronization_min_cycles,
                    direction_stats.synchronization_max_cycles,
                    if direction_stats.commits == 0 {
                        0.0
                    } else {
                        (direction_stats.synchronization_cycles
                            - direction_stats.cancelled_synchronization_cycles)
                            as f64
                            / direction_stats.commits as f64
                    },
                    direction_stats.state_switch_cycles,
                    direction_stats.state_switch_min_cycles,
                    direction_stats.state_switch_max_cycles,
                    if direction_stats.commits == 0 {
                        0.0
                    } else {
                        (direction_stats.state_switch_cycles
                            - direction_stats.cancelled_state_switch_cycles)
                            as f64
                            / direction_stats.commits as f64
                    },
                    direction_stats.cancelled_synchronization_cycles,
                    direction_stats.cancelled_state_switch_cycles,
                    direction_stats.cancelled_handoff_cycles,
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

        self.engines.all_mut(|engine| {
            !engine.can_accept_pim_cmd(cmd, request.is_write) || engine.canAccept(request)
        })
    }

    fn can_accept_regular_memory(&mut self, request: EngineRequest) -> bool {
        if matches!(self.sim_mode, SimMode::Pim) {
            if let Some(cfg) = self.get_engine_cfg(request.addr) {
                let mut engine = self
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
        let finished = u32::try_from(self.engines.count(|engine| engine.reached_final_barrier()))
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
        let harness = &mut self.harness;
        self.engines.for_each_mut(|cfg, engine| {
            if engine.can_accept_pim_cmd(cmd, request.is_write) {
                if !engine.canAccept(request) {
                    panic!("PIM command target engine cannot accept the request");
                }
                engine.enqueue_host_pim_request(req.clone(), cmd);
                if let pim_cmd::FGO(instruction) = cmd {
                    harness.log_FGO_receive(cfg, engine.clock_cycle(), req_id, instruction);
                }
                if matches!(cmd, pim_cmd::CGO_Start) {
                    harness.log_CGO_start(cfg, engine.clock_cycle(), req_id);
                }
                target_count += 1;
            }
        });

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
        let harness = &mut self.harness;
        let pending_pim_cmds = &mut self.pending_pim_cmds;
        let engine_comp_queue = &mut self.engine_comp_queue;
        self.engines.for_each_mut(|cfg, engine| {
            if harness.is_tracking_CGO(cfg) && engine.harness_CGO_finished() {
                harness.log_CGO_finish(
                    cfg,
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
                    engine_comp_queue.push(req);
                    continue;
                }
                let Some(req_id) = req.get_id() else {
                    engine_comp_queue.push(req);
                    continue;
                };
                let Some(pending) = pending_pim_cmds.get(&req_id) else {
                    engine_comp_queue.push(req);
                    continue;
                };
                let cmd = pending.command;
                if let pim_cmd::FGO(instruction) = cmd {
                    let cycle = engine.clock_cycle();
                    if let crate::PE::types::inst::ST128 { addr, .. } = instruction {
                        harness.log_FGO_result(
                            cfg,
                            cycle,
                            req_id,
                            addr,
                            engine.harness_read_FGO_vector(addr),
                        );
                    }
                    harness.log_FGO_retire(cfg, cycle, req_id, instruction);
                }
                let is_drained = {
                    let pending = pending_pim_cmds
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
                    let completed = pending_pim_cmds
                        .remove(&req_id)
                        .expect("drained PIM command must still be pending");
                    if completed.emit_completion {
                        engine_comp_queue.push(completed.request);
                    }
                }
            }
        });
    }

    fn ensure_engine_tick_workers(&mut self, worker_count: usize) {
        if worker_count == 0 || self.engine_tick_workers.is_some() {
            return;
        }
        let shards = self.engines.shard_handles();
        assert_eq!(shards.len(), worker_count);
        self.engine_tick_workers = Some(EngineTickWorkers::new(shards));
    }

    fn tick_engines_parallel(&mut self) {
        if self.engines.is_empty() {
            return;
        }

        let worker_count = self.engines.prepare_fixed_shards(self.engine_worker_limit);
        self.ensure_engine_tick_workers(worker_count);
        self.engine_tick_workers
            .as_ref()
            .expect("engine tick workers must exist when engines are present")
            .tick();
    }

    fn enqueue_regular_memory(&mut self, mut req: dram_req, payload_sz_bytes: u32) {
        if let SimMode::Pim = self.sim_mode {
            if let Some(cfg) = self.get_engine_cfg(req.get_addr()) {
                let ignored = {
                    let mut engine = self
                        .engines
                        .get_mut(&cfg)
                        .expect("Cannot detect available engine");
                    if !engine.accepts_host_mem_requests() {
                        true
                    } else {
                        if !req.is_read()
                            && payload_sz_bytes as usize == std::mem::size_of::<cacheline_payload>()
                        {
                            engine.mirror_host_write(req.get_addr(), req.get_payload());
                        }
                        engine.enqueue_host_mem_request(req.clone());
                        false
                    }
                };
                if ignored {
                    eprintln!("warning: ignoring host memory request to a PIM-only engine");
                    self.enqueue_next_cycle_completion(req);
                    return;
                }
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
