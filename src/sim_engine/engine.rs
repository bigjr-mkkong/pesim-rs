use crate::CPU;
use crate::PE::pe_top::PE;
use crate::cpu::boot_fsm::CPU_boot_FSM;
use crate::memory::dramsim3_wrapper::dramsim3_wrapper;
use crate::memory::flat_memory::PIM_ENTRIES_PER_CACHELINE;
use crate::memory::mem_portal::{cacheline_payload, dram_portal, dram_req, portal_mode};
use crate::sim_engine::engine_alloc::{
    PSEUDO_BANK_CACHELINES, PSEUDO_BANK_ENTRIES, PSEUDO_BANKS_PER_LOGICAL_BANK,
};
use crate::sim_engine::request_router::{pim_cmd, validate_pim_cmd_access};
use std::collections::{HashSet, VecDeque};
use std::path::Path;
use std::sync::OnceLock;
#[cfg(test)]
use std::sync::atomic::{AtomicU8, Ordering};

const BATCH_SZ: u64 = 0;

fn verbose_engine_trace() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var("PIM_VERBOSE_TRACE").as_deref() == Ok("1"))
}

#[cfg(test)]
const SCHED_PROBE_INVOKED: u8 = 1 << 0;
#[cfg(test)]
const SCHED_PROBE_ENTERED_HOST: u8 = 1 << 1;
#[cfg(test)]
const SCHED_PROBE_ENTERED_PIM: u8 = 1 << 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EngineMode {
    PIM,
    HOST,
    switch_delay,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SwitchPhase {
    Idle,
    SourceQuiescing,
    MemoryPausing,
    NearRowSwitch,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct CgoSwitchDirectionStats {
    pub requests: u64,
    pub commits: u64,
    pub cancellations: u64,
    pub source_quiesce_cycles: u64,
    pub promoted_drain_cycles: u64,
    pub fixed_delay_cycles: u64,
    pub commit_guard_cycles: u64,
    pub parked_transactions_total: u64,
    pub parked_transactions_max: u64,
    pub promoted_transactions_total: u64,
    pub promoted_transactions_max: u64,
}

impl CgoSwitchDirectionStats {
    pub(crate) fn total_cycles(&self) -> u64 {
        self.source_quiesce_cycles
            + self.promoted_drain_cycles
            + self.fixed_delay_cycles
            + self.commit_guard_cycles
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct CgoSwitchStats {
    pub pim_to_host: CgoSwitchDirectionStats,
    pub host_to_pim: CgoSwitchDirectionStats,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CgoLifecycle {
    Idle,
    Booting,
    Running,
    Finished,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EngineSchedulingMode {
    Unconfigured,
    CGO_only,
    Host_CGO_share,
    Host_FGO_share,
    HostOnly,
    Sequential,
}

enum EngineProcessor {
    CGO(CPU),
    FGO(PE),
}

#[derive(Clone, Copy)]
enum FGO_RequestState {
    Idle,
    PimInFlight,
    HostInFlight,
}

#[derive(Clone, Copy)]
enum CGO_Cmd {
    Start,
}

#[derive(Clone, Copy)]
pub(crate) struct EngineRequest {
    pub addr: u64,
    pub is_write: bool,
    pub decoded_cmd: Result<Option<pim_cmd>, &'static str>,
}

// CGO scheduling is cycle/batch based because the CPU runs autonomously. FGO scheduling is
// request based: the engine admits one PE instruction or one host request, waits for completion,
// and then gives the other source priority.

pub struct Engine {
    controller_id: u32,
    ch: u64,
    ra: u64,
    bg: u64,
    ba: u64,
    pseudo_bank: u64,
    pseudo_bank_base_cacheline: u64,
    processor: EngineProcessor,
    cgo_boot: Option<CPU_boot_FSM>,
    host_pool: VecDeque<dram_req>,
    host_complete_queue: VecDeque<dram_req>,
    cgo_cmd_queue: VecDeque<(CGO_Cmd, dram_req)>,
    cgo_cmd_complete_queue: VecDeque<dram_req>,
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
    cgo_host_quantum_req_ids: HashSet<u64>,
    cgo_switch_stats: CgoSwitchStats,
    //Following are F3FS scheduler internal variables
    fgo_request_state: FGO_RequestState,
    fgo_next_service: EngineMode,
    fgo_commands_enqueued: u64,
    fgo_commands_retired: u64,
    fgo_barrier_sequence: Option<u64>,
    switch_phase: SwitchPhase,
    switch_delay_remaining: u64,
    near_switch_cycles: u64,
    init_mirroring_enabled: bool,
    sequential_pim_requested: bool,
    // Test-only pin-out. This field and all writes to it are absent from production builds.
    #[cfg(test)]
    scheduler_probe: AtomicU8,
}

impl Engine {
    #[cfg(test)]
    pub fn new_cgo() -> Self {
        Self::new_cgo_at(0, 0, 0, 0, 0)
    }

    #[cfg(test)]
    pub fn new_fgo() -> Self {
        Self::new_fgo_at(0, 0, 0, 0, 0)
    }

    pub(crate) fn new_cgo_at(ch: u64, ra: u64, bg: u64, ba: u64, pb: u64) -> Self {
        let (cfg_path, out_dir) =
            crate::dsim3_paths(crate::PIM_DSIM3_CFG_PATH, crate::DSIM3_OUT_DIR);
        Self::new_cgo_configured(0, 0, cfg_path, out_dir, ch, ra, bg, ba, pb)
    }

    pub(crate) fn new_cgo_configured(
        controller_id: u32,
        controller_base: u64,
        cfg_path: impl AsRef<Path>,
        out_dir: impl AsRef<Path>,
        ch: u64,
        ra: u64,
        bg: u64,
        ba: u64,
        pb: u64,
    ) -> Self {
        Self::build(
            |dram_port| EngineProcessor::CGO(CPU::new_with_dram_port(dram_port)),
            controller_id,
            controller_base,
            cfg_path,
            out_dir,
            ch,
            ra,
            bg,
            ba,
            pb,
        )
    }

    pub(crate) fn new_fgo_at(ch: u64, ra: u64, bg: u64, ba: u64, pb: u64) -> Self {
        let (cfg_path, out_dir) =
            crate::dsim3_paths(crate::PIM_DSIM3_CFG_PATH, crate::DSIM3_OUT_DIR);
        Self::new_fgo_configured(0, 0, cfg_path, out_dir, ch, ra, bg, ba, pb)
    }

    pub(crate) fn new_fgo_configured(
        controller_id: u32,
        controller_base: u64,
        cfg_path: impl AsRef<Path>,
        out_dir: impl AsRef<Path>,
        ch: u64,
        ra: u64,
        bg: u64,
        ba: u64,
        pb: u64,
    ) -> Self {
        Self::build(
            |dram_port| EngineProcessor::FGO(PE::new_with_dram_port(dram_port)),
            controller_id,
            controller_base,
            cfg_path,
            out_dir,
            ch,
            ra,
            bg,
            ba,
            pb,
        )
    }

    fn build(
        make_processor: impl FnOnce(dram_portal) -> EngineProcessor,
        controller_id: u32,
        controller_base: u64,
        cfg_path: impl AsRef<Path>,
        out_dir: impl AsRef<Path>,
        ch: u64,
        ra: u64,
        bg: u64,
        ba: u64,
        pb: u64,
    ) -> Self {
        assert!(
            pb < PSEUDO_BANKS_PER_LOGICAL_BANK,
            "invalid pseudo-bank index"
        );
        let pseudo_bank_base_cacheline = pb
            .checked_mul(PSEUDO_BANK_CACHELINES)
            .expect("pseudo-bank base overflow");
        let mut dram_port = dram_portal::new();
        dram_port.set_mode(portal_mode::PIM);
        let processor = make_processor(dram_port.clone());
        let cgo_boot = matches!(&processor, EngineProcessor::CGO(_))
            .then(|| CPU_boot_FSM::new(dram_port.clone(), ch, ra, bg, ba, pb));

        let mut dsim3 = dramsim3_wrapper::new_for_pseudo_bank(
            cfg_path,
            out_dir,
            ch,
            ra,
            bg,
            ba,
            pb,
            pseudo_bank_base_cacheline,
            PSEUDO_BANK_CACHELINES,
            controller_base,
        );
        dsim3.SetPimMode(true);
        let near_switch_cycles = u64::try_from(dsim3.get_near_switch_latency())
            .expect("near-row switch latency must be non-negative");

        Self {
            controller_id,
            ch,
            ra,
            bg,
            ba,
            pseudo_bank: pb,
            pseudo_bank_base_cacheline,
            processor,
            dram_port,
            cgo_boot,
            host_pool: VecDeque::new(),
            host_complete_queue: VecDeque::new(),
            cgo_cmd_queue: VecDeque::new(),
            cgo_cmd_complete_queue: VecDeque::new(),
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
            cgo_host_quantum_req_ids: HashSet::new(),
            cgo_switch_stats: CgoSwitchStats::default(),
            clock_cycle: 0,
            fgo_request_state: FGO_RequestState::Idle,
            fgo_next_service: EngineMode::PIM,
            fgo_commands_enqueued: 0,
            fgo_commands_retired: 0,
            fgo_barrier_sequence: None,
            switch_phase: SwitchPhase::Idle,
            switch_delay_remaining: 0,
            near_switch_cycles,
            init_mirroring_enabled: true,
            sequential_pim_requested: false,
            #[cfg(test)]
            scheduler_probe: AtomicU8::new(0),
        }
    }

    pub fn set_scheduling_mode(
        &mut self,
        scheduling_mode: EngineSchedulingMode,
    ) -> Result<(), &'static str> {
        if self.scheduling_mode != EngineSchedulingMode::Unconfigured {
            // NOTE: Support live scheduler reconfiguration by defining how active processor and
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
                | (EngineProcessor::CGO(_), EngineSchedulingMode::Sequential)
                | (EngineProcessor::FGO(_), EngineSchedulingMode::Sequential)
        );
        if !compatible {
            return Err("scheduling mode is incompatible with the engine processor");
        }

        self.scheduling_mode = scheduling_mode;
        if matches!(
            scheduling_mode,
            EngineSchedulingMode::HostOnly
                | EngineSchedulingMode::Host_CGO_share
                | EngineSchedulingMode::Sequential
        ) {
            self.force_host_mode();
            self.first_host_switch_started = true;
        } else {
            self.force_pim_mode();
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn get_cpu(&mut self) -> &mut CPU {
        match &mut self.processor {
            EngineProcessor::CGO(cpu) => cpu,
            EngineProcessor::FGO(_) => panic!("cannot access CPU on an FGO engine"),
        }
    }

    #[cfg(test)]
    pub(crate) fn get_pe(&mut self) -> &mut PE {
        match &mut self.processor {
            EngineProcessor::FGO(pe) => pe,
            EngineProcessor::CGO(_) => panic!("cannot access PE on a CGO engine"),
        }
    }

    pub fn set_external_signal_delays(&mut self, pause_cycles: u64, resume_cycles: u64) {
        if let EngineProcessor::CGO(cpu) = &mut self.processor {
            cpu.set_external_signal_delays(pause_cycles, resume_cycles);
        }
    }

    #[cfg(test)]
    fn set_near_switch_cycles_for_test(&mut self, cycles: u64) {
        self.near_switch_cycles = cycles;
    }

    pub(crate) fn clock_cycle(&self) -> u64 {
        self.clock_cycle
    }

    pub(crate) fn cgo_switch_stats(&self) -> Option<CgoSwitchStats> {
        (self.scheduling_mode == EngineSchedulingMode::Host_CGO_share)
            .then_some(self.cgo_switch_stats)
    }

    pub(crate) fn harness_read_FGO_vector(&self, addr: u32) -> Option<[i16; 8]> {
        match &self.processor {
            EngineProcessor::FGO(pe) => pe.harness_read_vector(addr),
            EngineProcessor::CGO(_) => None,
        }
    }

    pub(crate) fn harness_CGO_finished(&self) -> bool {
        matches!(&self.processor, EngineProcessor::CGO(cpu) if cpu.is_finished())
    }

    pub(crate) fn reached_final_barrier(&self) -> bool {
        match &self.processor {
            EngineProcessor::CGO(cpu) => cpu.is_finished(),
            EngineProcessor::FGO(_) => {
                self.fgo_barrier_sequence == Some(self.fgo_commands_enqueued)
                    && self.fgo_commands_retired >= self.fgo_commands_enqueued
            }
        }
    }

    pub(crate) fn harness_read_CGO_outputs(&self) -> Option<Vec<[u32; 4]>> {
        let EngineProcessor::CGO(cpu) = &self.processor else {
            return None;
        };
        let (base, bound) = cpu.agu.get_entry(3)?;

        (0..bound)
            .map(|offset| cpu.fmem.mem_read_data(base + offset))
            .collect()
    }

    fn switch_drain_done(&mut self) -> bool {
        match self.last_service_mode {
            EngineMode::PIM => {
                let processor_ready = match &self.processor {
                    // EqualExit halts the CGO pipeline permanently, so there
                    // is no pause handshake left to acknowledge before host
                    // traffic can reclaim the DRAM port.
                    EngineProcessor::CGO(cpu) => cpu.is_finished() || cpu.ready4signal(),
                    EngineProcessor::FGO(_) => true,
                };
                processor_ready
                    && self.dram_port.req_drained_for_mode(portal_mode::PIM)
                    && self.dsim3.is_drained()
            }
            EngineMode::HOST => {
                self.dram_port.req_drained_for_mode(portal_mode::HOST) && self.dsim3.is_drained()
            }
            EngineMode::switch_delay => false,
        }
    }

    /*
     * switch() simulates the following automata:
     * Host -> switch_delay(self-looping) -> PIM -> switch_delay(self-looping) -> Host
     */
    fn switch(&mut self, from: EngineMode) {
        if self.scheduling_mode == EngineSchedulingMode::Host_CGO_share {
            self.switch_cgo(from);
        } else {
            self.switch_legacy(from);
        }
    }

    fn switch_legacy(&mut self, from: EngineMode) {
        match from {
            EngineMode::PIM => {
                self.next_mode = EngineMode::switch_delay;
                self.last_service_mode = EngineMode::PIM;
                self.switch_phase = SwitchPhase::SourceQuiescing;
                self.switch_delay_remaining = 0;
            }
            EngineMode::HOST => {
                self.next_mode = EngineMode::switch_delay;
                self.last_service_mode = EngineMode::HOST;
                self.switch_phase = SwitchPhase::SourceQuiescing;
                self.switch_delay_remaining = 0;
            }
            EngineMode::switch_delay => {
                if self.switch_phase == SwitchPhase::SourceQuiescing {
                    if !self.switch_drain_done() {
                        self.next_mode = EngineMode::switch_delay;
                        return;
                    }
                    self.switch_phase = SwitchPhase::NearRowSwitch;
                    self.switch_delay_remaining = self.near_switch_cycles;
                }

                if self.switch_delay_remaining > 0 {
                    self.switch_delay_remaining -= 1;
                    self.next_mode = EngineMode::switch_delay;
                    return;
                }

                // Refresh or a late portal request can make the timing model
                // busy again during the modeled near-row switching delay.
                // Recheck at the exact mode-change boundary before asking
                // DRAMSim3 to swap its saved CPU/PIM row context.
                if !self.dsim3.is_drained() {
                    self.next_mode = EngineMode::switch_delay;
                    return;
                }

                assert_eq!(self.switch_phase, SwitchPhase::NearRowSwitch);
                self.switch_phase = SwitchPhase::Idle;

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

    fn switch_cgo(&mut self, from: EngineMode) {
        match from {
            EngineMode::PIM | EngineMode::HOST => {
                self.next_mode = EngineMode::switch_delay;
                self.last_service_mode = from;
                self.switch_phase = SwitchPhase::SourceQuiescing;
                self.switch_delay_remaining = 0;
                self.active_cgo_switch_stats_mut().requests += 1;
            }
            EngineMode::switch_delay => {
                if self.switch_phase == SwitchPhase::SourceQuiescing {
                    if !self.cgo_source_quiesced() {
                        self.active_cgo_switch_stats_mut().source_quiesce_cycles += 1;
                        self.next_mode = EngineMode::switch_delay;
                        return;
                    }

                    self.dsim3.request_pause();
                    let parked = self.dsim3.pause_parked_transactions();
                    let promoted = self.dsim3.pause_promoted_transactions();
                    let stats = self.active_cgo_switch_stats_mut();
                    stats.parked_transactions_total += parked;
                    stats.parked_transactions_max = stats.parked_transactions_max.max(parked);
                    stats.promoted_transactions_total += promoted;
                    stats.promoted_transactions_max = stats.promoted_transactions_max.max(promoted);
                    self.switch_phase = SwitchPhase::MemoryPausing;
                }

                if self.switch_phase == SwitchPhase::MemoryPausing {
                    if !self.dsim3.is_pause_ready() {
                        self.active_cgo_switch_stats_mut().promoted_drain_cycles += 1;
                        self.next_mode = EngineMode::switch_delay;
                        return;
                    }
                    self.switch_phase = SwitchPhase::NearRowSwitch;
                    self.switch_delay_remaining = self.near_switch_cycles;
                }

                if self.switch_delay_remaining > 0 {
                    self.switch_delay_remaining -= 1;
                    self.active_cgo_switch_stats_mut().fixed_delay_cycles += 1;
                    self.next_mode = EngineMode::switch_delay;
                    return;
                }

                if !self.dsim3.is_pause_ready() {
                    self.active_cgo_switch_stats_mut().commit_guard_cycles += 1;
                    self.next_mode = EngineMode::switch_delay;
                    return;
                }

                assert_eq!(self.switch_phase, SwitchPhase::NearRowSwitch);
                self.switch_phase = SwitchPhase::Idle;
                self.active_cgo_switch_stats_mut().commits += 1;
                match self.last_service_mode {
                    EngineMode::PIM => {
                        self.next_mode = EngineMode::HOST;
                        self.dram_port.set_mode(portal_mode::HOST);
                        self.dsim3.commit_paused_mode(false);
                    }
                    EngineMode::HOST => {
                        self.next_mode = EngineMode::PIM;
                        self.dram_port.set_mode(portal_mode::PIM);
                        self.dsim3.commit_paused_mode(true);
                    }
                    EngineMode::switch_delay => unreachable!(),
                }
            }
        }
    }

    fn cgo_source_quiesced(&mut self) -> bool {
        match self.last_service_mode {
            EngineMode::PIM => {
                let EngineProcessor::CGO(cpu) = &self.processor else {
                    unreachable!("CGO switch requires a CGO processor");
                };
                (cpu.is_finished() || cpu.ready4signal())
                    && self.dram_port.req_drained_for_mode(portal_mode::PIM)
            }
            EngineMode::HOST => {
                self.cgo_host_quantum_req_ids.is_empty()
                    && self.dram_port.req_drained_for_mode(portal_mode::HOST)
            }
            EngineMode::switch_delay => false,
        }
    }

    fn active_cgo_switch_stats_mut(&mut self) -> &mut CgoSwitchDirectionStats {
        match self.last_service_mode {
            EngineMode::PIM => &mut self.cgo_switch_stats.pim_to_host,
            EngineMode::HOST => &mut self.cgo_switch_stats.host_to_pim,
            EngineMode::switch_delay => {
                unreachable!("switch delay cannot be an outgoing service mode")
            }
        }
    }

    fn cancel_cgo_switch(&mut self) {
        if self.switch_phase == SwitchPhase::Idle {
            return;
        }
        if self.dsim3.is_pause_requested() {
            self.dsim3.cancel_pause();
        }
        self.active_cgo_switch_stats_mut().cancellations += 1;
        self.switch_phase = SwitchPhase::Idle;
        self.switch_delay_remaining = 0;
    }
    fn force_pim_mode(&mut self) {
        self.cancel_cgo_switch();
        self.mode = EngineMode::PIM;
        self.next_mode = EngineMode::PIM;
        self.switch_phase = SwitchPhase::Idle;
        self.switch_delay_remaining = 0;
        self.dram_port.set_mode(portal_mode::PIM);
        self.dsim3.SetPimMode(true);
    }

    fn force_host_mode(&mut self) {
        self.cancel_cgo_switch();
        self.mode = EngineMode::HOST;
        self.next_mode = EngineMode::HOST;
        self.switch_phase = SwitchPhase::Idle;
        self.switch_delay_remaining = 0;
        self.dram_port.set_mode(portal_mode::HOST);
        self.dsim3.SetPimMode(false);
    }

    pub fn schedule(&mut self) {
        #[cfg(test)]
        let mode_before = {
            self.scheduler_probe
                .fetch_or(SCHED_PROBE_INVOKED, Ordering::Relaxed);
            self.mode
        };

        match self.scheduling_mode {
            EngineSchedulingMode::Unconfigured => {
                panic!("cannot schedule an engine before configuring its scheduling mode")
            }
            EngineSchedulingMode::CGO_only => {
                self.force_pim_mode();
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
            EngineSchedulingMode::Sequential => self.schedule_sequential(),
        }

        #[cfg(test)]
        match (mode_before, self.next_mode) {
            (EngineMode::switch_delay, EngineMode::HOST) => {
                self.scheduler_probe
                    .fetch_or(SCHED_PROBE_ENTERED_HOST, Ordering::Relaxed);
            }
            (EngineMode::switch_delay, EngineMode::PIM) => {
                self.scheduler_probe
                    .fetch_or(SCHED_PROBE_ENTERED_PIM, Ordering::Relaxed);
            }
            _ => {}
        }
    }

    /*
     * This implements a batch-based CFS for CGO and host
     */
    fn schedule_host_cgo_share(&mut self) {
        match self.cgo_lifecycle() {
            CgoLifecycle::Idle | CgoLifecycle::Finished => {
                self.schedule_cgo_host_exclusive();
                return;
            }
            CgoLifecycle::Booting => {
                self.schedule_cgo_boot_exclusive();
                return;
            }
            CgoLifecycle::Running => {}
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
                    self.first_host_switch_started = true;
                    self.PIM_tick_rec = 0;
                    match &mut self.processor {
                        EngineProcessor::CGO(cpu) if cpu.is_started() => cpu.signal_pause(),
                        EngineProcessor::CGO(_) => {}
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
                for mut req in batch.into_iter().rev() {
                    if req.get_id().is_none() {
                        req.set_id(self.dsim3.get_req_id());
                    }
                    self.cgo_host_quantum_req_ids.insert(
                        req.get_id()
                            .expect("CGO host-quantum request must carry an ID"),
                    );
                    self.dram_port.submit(req);
                }

                if self.MEM_tick_rec > self.MEM_req_watermarkL || self.host_pool.is_empty() {
                    self.PIM_tick_watermark = self.MEM_tick_rec;
                    self.MEM_tick_rec = 0;
                    match &mut self.processor {
                        EngineProcessor::CGO(cpu) if cpu.is_started() => cpu.signal_resume(),
                        EngineProcessor::CGO(_) => {}
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

    fn cgo_lifecycle(&self) -> CgoLifecycle {
        let EngineProcessor::CGO(cpu) = &self.processor else {
            unreachable!("CGO lifecycle requires a CGO processor");
        };

        if cpu.is_finished() {
            CgoLifecycle::Finished
        } else if cpu.is_started() {
            CgoLifecycle::Running
        } else if self
            .cgo_boot
            .as_ref()
            .expect("CGO engine must own a boot controller")
            .is_idle()
        {
            CgoLifecycle::Idle
        } else {
            CgoLifecycle::Booting
        }
    }

    fn submit_all_host_requests(&mut self) {
        // The portal is stack-backed, so reverse submission preserves FIFO.
        while let Some(req) = self.host_pool.pop_back() {
            self.dram_port.submit(req);
        }
    }

    fn schedule_cgo_host_exclusive(&mut self) {
        match self.mode {
            EngineMode::HOST => {
                self.next_mode = EngineMode::HOST;
                self.submit_all_host_requests();
            }
            EngineMode::PIM => self.switch(EngineMode::PIM),
            EngineMode::switch_delay if self.last_service_mode == EngineMode::HOST => {
                // CGO can reach EqualExit after resume but before a pending
                // HOST->PIM handoff completes.  No PIM work remains, and the
                // DRAM timing context has not changed yet, so cancel that
                // unnecessary handoff and keep serving the host.
                self.force_host_mode();
                self.submit_all_host_requests();
            }
            EngineMode::switch_delay => self.switch(EngineMode::switch_delay),
        }
    }

    fn schedule_cgo_boot_exclusive(&mut self) {
        match self.mode {
            EngineMode::PIM => self.next_mode = EngineMode::PIM,
            EngineMode::HOST => self.switch(EngineMode::HOST),
            EngineMode::switch_delay => self.switch(EngineMode::switch_delay),
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
        self.fgo_request_state = FGO_RequestState::PimInFlight;
    }

    fn fgo_pim_finished(&mut self) -> bool {
        let finished = match &mut self.processor {
            EngineProcessor::FGO(pe) => pe.has_finished(),
            EngineProcessor::CGO(_) => unreachable!(),
        };
        if finished {
            self.fgo_commands_retired = self
                .fgo_commands_retired
                .checked_add(1)
                .expect("FGO retired-command counter overflow");
        }
        finished
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
        self.fgo_request_state = FGO_RequestState::HostInFlight;
    }

    /*
     * TODO
     * This is the current FGO schedule algorithm
     * It's now a basic round-robin, act like a stub for future F3FS implementation
     */
    fn schedule_host_fgo_share(&mut self) {
        match self.mode {
            EngineMode::switch_delay => {
                self.switch(EngineMode::switch_delay);
            }
            EngineMode::PIM => {
                if matches!(self.fgo_request_state, FGO_RequestState::PimInFlight)
                    && self.fgo_pim_finished()
                {
                    self.fgo_request_state = FGO_RequestState::Idle;
                    self.fgo_next_service = EngineMode::HOST;
                }

                if matches!(self.fgo_request_state, FGO_RequestState::Idle) {
                    self.fgo_select_in_pim_mode();
                }
            }
            EngineMode::HOST => {
                if matches!(self.fgo_request_state, FGO_RequestState::HostInFlight)
                    && self.dram_port.req_drained_for_mode(portal_mode::HOST)
                    && self.dsim3.is_drained()
                {
                    self.fgo_request_state = FGO_RequestState::Idle;
                    self.fgo_next_service = EngineMode::PIM;
                }

                if matches!(self.fgo_request_state, FGO_RequestState::Idle) {
                    self.fgo_select_in_host_mode();
                }
            }
        }
    }

    /*
     * Lock-free profiling baseline.  Host traffic drains in a host-only
     * phase.  The first PIM command requests a direct, zero-delay handoff to
     * PIM-only execution; no sharing scheduler or modeled switch delay is
     * involved.  Post-measurement verification may hand back to host after
     * the final PIM barrier.
     */
    fn schedule_sequential(&mut self) {
        match self.mode {
            EngineMode::HOST => {
                while let Some(req) = self.host_pool.pop_back() {
                    self.dram_port.submit(req);
                }
                if self.sequential_pim_requested
                    && self.host_pool.is_empty()
                    && self.dram_port.req_drained_for_mode(portal_mode::HOST)
                    && self.dsim3.is_drained()
                {
                    self.force_pim_mode();
                }
            }
            EngineMode::PIM => {
                // The guest PIM submission thread can still miss in its
                // instruction cache (notably in pseudo-bank 0).  This is
                // orchestration traffic, not the finished CPU benchmark.
                // Admit it directly to the same timing model without taking
                // the host/PIM scheduler lock or changing row contexts.
                self.issue_sequential_host_in_pim_phase();

                if matches!(self.processor, EngineProcessor::FGO(_)) {
                    if matches!(self.fgo_request_state, FGO_RequestState::PimInFlight)
                        && self.fgo_pim_finished()
                    {
                        self.fgo_request_state = FGO_RequestState::Idle;
                    }
                    if matches!(self.fgo_request_state, FGO_RequestState::Idle)
                        && self.fgo_has_buffered_inst()
                    {
                        self.fgo_issue_pim();
                    }
                }
            }
            EngineMode::switch_delay => {
                panic!("sequential scheduling must never enter switch_delay")
            }
        }
    }

    fn issue_sequential_host_in_pim_phase(&mut self) {
        // dram_portal is stack-backed, so reverse submission preserves FIFO.
        while let Some(req) = self.host_pool.pop_back() {
            if verbose_engine_trace() && self.pseudo_bank == 0 {
                eprintln!(
                    "SEQUENTIAL_TRACE event=portal-submit cycle={} addr={:#x}",
                    self.clock_cycle,
                    req.get_addr()
                );
            }
            self.dram_port.submit_host_in_pim_phase(req);
        }
    }

    pub fn enqueue_host_mem_request(&mut self, req: dram_req) {
        if req.is_pim() {
            panic!("host memory request must be a non-PIM memory access");
        }
        if verbose_engine_trace() && self.pseudo_bank == 0 {
            eprintln!(
                "SEQUENTIAL_TRACE event=host-enqueue cycle={} mode={:?} addr={:#x}",
                self.clock_cycle,
                self.mode,
                req.get_addr()
            );
        }
        self.host_pool.push_back(req);
    }

    pub(crate) fn mirror_host_write(&mut self, global_addr: u64, payload: &cacheline_payload) {
        if !self.init_mirroring_enabled {
            return;
        }

        if matches!(&self.processor, EngineProcessor::CGO(cpu) if cpu.is_started()) {
            return;
        }

        let local_addr = self.dsim3.global_addr_to_local_components(global_addr);
        assert_eq!(
            (
                local_addr.channel,
                local_addr.rank,
                local_addr.bank_group,
                local_addr.bank
            ),
            (self.ch, self.ra, self.bg, self.ba),
            "host write was routed to the wrong logical bank"
        );
        let pseudo_local_cacheline = local_addr
            .bank_local_addr
            .checked_sub(self.pseudo_bank_base_cacheline)
            .filter(|offset| *offset < PSEUDO_BANK_CACHELINES)
            .expect("host write was routed to the wrong pseudo bank");
        let first_entry = pseudo_local_cacheline
            .checked_mul(PIM_ENTRIES_PER_CACHELINE)
            .and_then(|addr| u32::try_from(addr).ok())
            .expect("bank-local address exceeds PIM flat-memory address space");

        match &mut self.processor {
            EngineProcessor::CGO(cpu) => {
                cpu.get_fmem().mirror_host_write(first_entry, payload);
            }
            EngineProcessor::FGO(pe) => pe.mirror_host_write(first_entry, payload),
        }
    }

    pub fn enqueue_host_pim_request(&mut self, req: dram_req, cmd: pim_cmd) {
        if req.is_pim() {
            panic!("host PIM command must use the host-command path");
        }
        validate_pim_cmd_access(cmd, !req.is_read())
            .unwrap_or_else(|err| panic!("cannot accept PIM command request: {err}"));
        if !self.can_accept_pim_cmd(cmd, !req.is_read()) {
            panic!("cannot route PIM command to this engine");
        }
        self.validate_pim_command_bounds(cmd);

        match (&mut self.processor, cmd) {
            (EngineProcessor::FGO(pe), pim_cmd::FGO(instruction)) => {
                if self.scheduling_mode == EngineSchedulingMode::Sequential {
                    self.sequential_pim_requested = true;
                }
                self.init_mirroring_enabled = false;
                self.fgo_commands_enqueued = self
                    .fgo_commands_enqueued
                    .checked_add(1)
                    .expect("FGO enqueued-command counter overflow");
                if matches!(instruction, crate::PE::types::inst::NOP) {
                    self.fgo_barrier_sequence = Some(self.fgo_commands_enqueued);
                }
                pe.push_host_req(req, instruction)
            }
            (EngineProcessor::CGO(_), pim_cmd::CGO_Start) => {
                if self.scheduling_mode == EngineSchedulingMode::Sequential {
                    self.sequential_pim_requested = true;
                }
                if self.scheduling_mode != EngineSchedulingMode::HostOnly {
                    self.init_mirroring_enabled = false;
                }
                self.cgo_cmd_queue.push_back((CGO_Cmd::Start, req));
            }
            _ => unreachable!("PIM command compatibility was checked before enqueue"),
        }
    }

    fn validate_pim_command_bounds(&self, cmd: pim_cmd) {
        let pim_cmd::FGO(instruction) = cmd else {
            return;
        };
        let (operation, addr) = match instruction {
            crate::PE::types::inst::LD128 { addr, .. } => ("LD128", addr),
            crate::PE::types::inst::ST128 { addr, .. } => ("ST128", addr),
            crate::PE::types::inst::LD32 { addr, .. } => ("LD32", addr),
            crate::PE::types::inst::ST32 { addr, .. } => ("ST32", addr),
            _ => return,
        };

        if u64::from(addr) >= PSEUDO_BANK_ENTRIES {
            self.fatal_pim_oob(operation, u64::from(addr));
        }
    }

    #[cold]
    fn fatal_pim_oob(&self, operation: &str, addr: u64) -> ! {
        eprintln!(
            "PIM_FATAL reason=address_out_of_bounds operation={operation} controller={} dram_channel={} rank={} bank_group={} bank={} pseudo_bank={} address={} valid_entries=0..{}",
            self.controller_id,
            self.ch,
            self.ra,
            self.bg,
            self.ba,
            self.pseudo_bank,
            addr,
            PSEUDO_BANK_ENTRIES
        );

        #[cfg(test)]
        panic!("PIM address is outside its pseudo bank");

        #[cfg(not(test))]
        std::process::abort();
    }

    pub(crate) fn canAccept(&mut self, request: EngineRequest) -> bool {
        match request.decoded_cmd {
            Ok(Some(cmd)) => self.can_accept_pim_cmd(cmd, request.is_write),
            Ok(None) => self
                .dsim3
                .WillAcceptTransaction(request.addr, request.is_write),
            Err(_) => false,
        }
    }

    pub(crate) fn can_accept_pim_cmd(&self, cmd: pim_cmd, is_write: bool) -> bool {
        if validate_pim_cmd_access(cmd, is_write).is_err() {
            return false;
        }

        matches!(
            (&self.processor, cmd),
            (EngineProcessor::FGO(_), pim_cmd::FGO(_))
                | (EngineProcessor::CGO(_), pim_cmd::CGO_Start)
        )
    }

    pub(crate) fn accepts_host_mem_requests(&self) -> bool {
        matches!(
            self.scheduling_mode,
            EngineSchedulingMode::Host_CGO_share
                | EngineSchedulingMode::Host_FGO_share
                | EngineSchedulingMode::HostOnly
                | EngineSchedulingMode::Sequential
        )
    }

    fn process_cgo_cmds(&mut self) {
        if !matches!(self.processor, EngineProcessor::CGO(_)) {
            return;
        }

        while let Some((cmd, req)) = self.cgo_cmd_queue.pop_front() {
            match cmd {
                CGO_Cmd::Start => {
                    let accepted = self.scheduling_mode != EngineSchedulingMode::HostOnly
                        && self
                            .cgo_boot
                            .as_mut()
                            .expect("CGO engine must own a boot controller")
                            .set_on();
                    if !accepted {
                        eprintln!(
                            "PIM_ERROR reason=irregular_cgo_start controller={} dram_channel={} rank={} bank_group={} bank={} pseudo_bank={}",
                            self.controller_id,
                            self.ch,
                            self.ra,
                            self.bg,
                            self.ba,
                            self.pseudo_bank
                        );
                    }
                }
            }

            self.cgo_cmd_complete_queue.push_back(req);
        }
    }

    fn tick_cgo_processor(&mut self) {
        let EngineProcessor::CGO(cpu) = &mut self.processor else {
            unreachable!("CGO scheduling requires a CGO processor");
        };

        if cpu.is_started() {
            cpu.tick();
            return;
        }

        if self.mode != EngineMode::PIM {
            return;
        }

        let boot = self
            .cgo_boot
            .as_mut()
            .expect("CGO engine must own a boot controller");
        boot.tick(&cpu.fmem, &mut cpu.agu, &mut cpu.imem, &mut cpu.RF);
        if boot.has_finished() {
            cpu.start();
        }
    }

    fn drain_current_port_to_dram(&mut self) {
        loop {
            let Some(mut req) = self.dram_port.get_one_req() else {
                break;
            };

            if self.dsim3.WillAcceptTransactionReq(&req) {
                if verbose_engine_trace()
                    && self.scheduling_mode == EngineSchedulingMode::Sequential
                    && self.pseudo_bank == 0
                    && !req.is_pim()
                {
                    eprintln!(
                        "SEQUENTIAL_TRACE event=dram-accept cycle={} addr={:#x}",
                        self.clock_cycle,
                        req.get_addr()
                    );
                }
                if req.get_id().is_none() {
                    req.set_id(self.dsim3.get_req_id());
                }
                if req.get_issue_time().is_none() {
                    req.set_issue_time(self.clock_cycle);
                }
                self.dsim3.AddTransactionReq(req);
            } else {
                if self.scheduling_mode == EngineSchedulingMode::Sequential
                    && self.mode == EngineMode::PIM
                    && !req.is_pim()
                {
                    if verbose_engine_trace() && self.pseudo_bank == 0 {
                        eprintln!(
                            "SEQUENTIAL_TRACE event=dram-retry cycle={} addr={:#x}",
                            self.clock_cycle,
                            req.get_addr()
                        );
                    }
                    self.dram_port.submit_host_in_pim_phase(req);
                } else {
                    self.dram_port.submit(req);
                }
                break;
            }
        }
    }

    fn drain_host_completions(&mut self) {
        while let Some(req) = self.cgo_cmd_complete_queue.pop_front() {
            self.host_complete_queue.push_back(req);
        }

        if let EngineProcessor::FGO(pe) = &mut self.processor {
            while pe.has_complete() {
                self.host_complete_queue.push_back(
                    pe.take_completed()
                        .expect("PE completion queue changed while being drained"),
                );
            }
        }

        while let Some(req) = self.dram_port.take_host_completed() {
            if let Some(req_id) = req.get_id() {
                self.cgo_host_quantum_req_ids.remove(&req_id);
            }
            if verbose_engine_trace()
                && self.scheduling_mode == EngineSchedulingMode::Sequential
                && self.pseudo_bank == 0
            {
                eprintln!(
                    "SEQUENTIAL_TRACE event=host-complete cycle={} addr={:#x}",
                    self.clock_cycle,
                    req.get_addr()
                );
            }
            self.host_complete_queue.push_back(req);
        }
    }

    pub fn get_host_complete(&mut self) -> Option<dram_req> {
        self.host_complete_queue.pop_front()
    }

    pub fn tick(&mut self) {
        match self.scheduling_mode {
            EngineSchedulingMode::Unconfigured => {
                panic!("cannot tick an engine before configuring its scheduling mode")
            }
            EngineSchedulingMode::CGO_only
            | EngineSchedulingMode::Host_CGO_share
            | EngineSchedulingMode::Sequential
                if matches!(self.processor, EngineProcessor::CGO(_)) =>
            {
                self.tick_cgo_processor();
            }
            EngineSchedulingMode::Host_FGO_share | EngineSchedulingMode::Sequential => {
                if matches!(self.mode, EngineMode::PIM)
                    && matches!(self.fgo_request_state, FGO_RequestState::PimInFlight)
                {
                    match &mut self.processor {
                        EngineProcessor::FGO(pe) => pe.tick(),
                        EngineProcessor::CGO(_) => unreachable!(),
                    }
                }
            }
            EngineSchedulingMode::HostOnly => {}
            EngineSchedulingMode::CGO_only | EngineSchedulingMode::Host_CGO_share => {
                unreachable!("CGO scheduling mode configured on an FGO engine")
            }
        }
        self.process_cgo_cmds();
        self.drain_current_port_to_dram();

        for req in self.dsim3.ClockTick() {
            self.dram_port.complete(req);
        }
        self.drain_host_completions();

        self.schedule();
        self.mode = self.next_mode;
        self.clock_cycle += 1;
    }
}

#[cfg(test)]
#[path = "engine_test.rs"]
mod engine_test;
