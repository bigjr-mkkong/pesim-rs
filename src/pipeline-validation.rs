use crate::cpu::pimcpu_types::{CPU_stages, fatptr_rf, inst};
use crate::cpu::pipeline::{CPU, PipelineValidationProbe};
use crate::cpu::signal_scoreboard::{SigFSM, pipeline_action, signal_reason};
use std::collections::HashMap;

const MAX_INSTRUCTION_TICKS: u64 = 32;
const DATA_ADDR: u32 = 16;
const FPTR_ADDR: u32 = 32;
const LOAD_VALUE: [u32; 4] = [0x55; 4];
const FOLLOWER_VALUE: [u32; 4] = [7; 4];

#[derive(Clone, Copy)]
enum DeterministicMemState {
    Waiting(u64),
    WriteBack,
    Idle,
}

#[derive(Clone)]
struct DeterministicMemStopFsm {
    state: DeterministicMemState,
}

impl DeterministicMemStopFsm {
    fn new(delay_cycles: u64) -> Self {
        assert!(delay_cycles > 0, "memory delay must be at least one cycle");
        Self {
            state: DeterministicMemState::Waiting(delay_cycles),
        }
    }
}

impl SigFSM for DeterministicMemStopFsm {
    fn reason(&self) -> signal_reason {
        signal_reason::mem_block_kind()
    }

    fn action(&self) -> pipeline_action {
        match self.state {
            DeterministicMemState::Waiting(_) | DeterministicMemState::WriteBack => {
                pipeline_action::Stall
            }
            DeterministicMemState::Idle => pipeline_action::Normal,
        }
    }

    fn get_ops(&self) -> HashMap<CPU_stages, pipeline_action> {
        match self.state {
            DeterministicMemState::Waiting(_) => HashMap::from([
                (CPU_stages::IF, pipeline_action::Stall),
                (CPU_stages::ID, pipeline_action::Stall),
                (CPU_stages::EX, pipeline_action::Stall),
                (CPU_stages::MEM, pipeline_action::Stall),
            ]),
            DeterministicMemState::WriteBack => HashMap::from([
                (CPU_stages::IF, pipeline_action::Stall),
                (CPU_stages::ID, pipeline_action::Stall),
                (CPU_stages::EX, pipeline_action::Stall),
            ]),
            DeterministicMemState::Idle => HashMap::new(),
        }
    }

    fn advance_winner(&mut self, _sig_reason: signal_reason) -> bool {
        self.state = match self.state {
            DeterministicMemState::Waiting(0 | 1) => DeterministicMemState::WriteBack,
            DeterministicMemState::Waiting(remaining) => {
                DeterministicMemState::Waiting(remaining - 1)
            }
            DeterministicMemState::WriteBack => DeterministicMemState::Idle,
            DeterministicMemState::Idle => DeterministicMemState::Idle,
        };
        true
    }
}

fn seed_architecture(cpu: &mut CPU) {
    cpu.get_agu().insert(1, DATA_ADDR, 16);
    cpu.get_agu().insert(2, FPTR_ADDR, 16);

    cpu.get_RF().write_vregs(1, [2; 4]);
    cpu.get_RF().write_vregs(2, [3; 4]);
    cpu.get_RF().write_vregs(3, [4; 4]);
    cpu.get_RF().write_vregs(4, [0; 4]);
    cpu.get_RF().write_vregs(5, [0; 4]);
    cpu.get_RF().write_vregs(6, [0; 4]);
    cpu.get_RF().write_vregs(7, [0; 4]);

    cpu.get_RF().write_fregs(1, fatptr_rf::new(1, 0));
    cpu.get_RF().write_fregs(2, fatptr_rf::new(2, 0));
    cpu.get_RF().write_fregs(3, fatptr_rf::new(1, 4));
    cpu.get_RF().write_fregs(4, fatptr_rf::new(1, 8));

    assert_eq!(
        cpu.get_fmem().mem_write_data(DATA_ADDR, &LOAD_VALUE),
        Some(())
    );
    assert_eq!(
        cpu.get_fmem()
            .mem_write_fptr(FPTR_ADDR, &fatptr_rf::new(1, 1)),
        Some(())
    );
}

fn cpu_with_program(program: &[inst], memory_delay: u64) -> CPU {
    let mut cpu = CPU::new_with_mem_stop_fsm(DeterministicMemStopFsm::new(memory_delay));
    seed_architecture(&mut cpu);
    cpu.get_imem().flash_in(program);
    cpu
}

#[derive(Clone, Copy)]
enum CompletionProbe {
    JumpId,
    EqualExitEx,
    StoreMem,
    WriteBack,
}

impl CompletionProbe {
    fn count(self, probe: PipelineValidationProbe) -> u64 {
        match self {
            CompletionProbe::JumpId => probe.jump_id_completions,
            CompletionProbe::EqualExitEx => probe.equal_exit_ex_completions,
            CompletionProbe::StoreMem => probe.store_mem_completions,
            CompletionProbe::WriteBack => probe.wb_completions,
        }
    }
}

fn assert_instruction_latency(
    instruction_name: &str,
    instruction: inst,
    completion_probe: CompletionProbe,
    expected_cycle: u64,
) {
    let mut cpu = cpu_with_program(&[instruction, inst::JUMP { inst_imm: 1 }], 1);
    let mut completion_cycle = None;

    for cycle in 1..=MAX_INSTRUCTION_TICKS {
        cpu.tick();
        let completions = completion_probe.count(cpu.validation_probe());
        if completion_cycle.is_none() && completions != 0 {
            assert_eq!(
                completions, 1,
                "{instruction_name} completed more than once on its first completion cycle"
            );
            assert_eq!(
                cycle, expected_cycle,
                "{instruction_name} completed on cycle {cycle}, expected cycle {expected_cycle}"
            );
            completion_cycle = Some(cycle);
        } else if completion_cycle.is_some() {
            assert_eq!(
                completions,
                1,
                "{instruction_name} completed more than once; first completion was cycle {}",
                completion_cycle.unwrap()
            );
        }
    }

    assert!(
        completion_cycle.is_some(),
        "{instruction_name} did not complete within {MAX_INSTRUCTION_TICKS} cold-pipeline ticks"
    );
}

macro_rules! instruction_latency_validation {
    ($test_name:ident, $label:literal, $instruction:expr, $probe:expr, $cycle:expr) => {
        #[test]
        fn $test_name() {
            assert_instruction_latency($label, $instruction, $probe, $cycle);
        }
    };
}

instruction_latency_validation!(
    nop_completes_at_wb_cycle_5,
    "NOP",
    inst::NOP,
    CompletionProbe::WriteBack,
    5
);
instruction_latency_validation!(
    add128_completes_at_wb_cycle_5,
    "ADD128",
    inst::ADD128 {
        rd: 4,
        rs1: 1,
        rs2: 2
    },
    CompletionProbe::WriteBack,
    5
);
instruction_latency_validation!(
    sub128_completes_at_wb_cycle_5,
    "SUB128",
    inst::SUB128 {
        rd: 4,
        rs1: 3,
        rs2: 2
    },
    CompletionProbe::WriteBack,
    5
);
instruction_latency_validation!(
    mul128_completes_at_wb_cycle_5,
    "MUL128",
    inst::MUL128 {
        rd: 4,
        rs1: 1,
        rs2: 2
    },
    CompletionProbe::WriteBack,
    5
);
instruction_latency_validation!(
    and128_completes_at_wb_cycle_5,
    "AND128",
    inst::AND128 {
        rd: 4,
        rs1: 1,
        rs2: 2
    },
    CompletionProbe::WriteBack,
    5
);
instruction_latency_validation!(
    ld128_completes_at_wb_cycle_6,
    "LD128",
    inst::LD128 { rd: 4, frs: 1 },
    CompletionProbe::WriteBack,
    6
);
instruction_latency_validation!(
    st128_completes_at_mem_cycle_5,
    "ST128",
    inst::ST128 { rs: 2, frd: 1 },
    CompletionProbe::StoreMem,
    5
);
instruction_latency_validation!(
    fatptr_ld_completes_at_wb_cycle_6,
    "FatPtrLD",
    inst::FatPtrLD { frd: 4, frs: 2 },
    CompletionProbe::WriteBack,
    6
);
instruction_latency_validation!(
    fatptr_st_completes_at_mem_cycle_5,
    "FatPtrST",
    inst::FatPtrST { frd: 2, frs: 3 },
    CompletionProbe::StoreMem,
    5
);
instruction_latency_validation!(
    fatptr_add_completes_at_wb_cycle_5,
    "FatPtrADD",
    inst::FatPtrADD {
        frd: 4,
        frs: 1,
        rs1: 1,
        imm_idx: 0
    },
    CompletionProbe::WriteBack,
    5
);
instruction_latency_validation!(
    fatptr_sub_completes_at_wb_cycle_5,
    "FatPtrSUB",
    inst::FatPtrSUB {
        frd: 4,
        frs: 3,
        rs1: 1,
        imm_idx: 0
    },
    CompletionProbe::WriteBack,
    5
);
instruction_latency_validation!(
    jump_completes_at_id_cycle_2,
    "JUMP",
    inst::JUMP { inst_imm: 7 },
    CompletionProbe::JumpId,
    2
);
instruction_latency_validation!(
    equal_exit_completes_at_ex_cycle_3,
    "EqualExit",
    inst::EqualExit { rd: 1, rs1: 1 },
    CompletionProbe::EqualExitEx,
    3
);

#[test]
fn agu_failure_is_reported_by_ex_without_an_ex_mem_result() {
    let mut cpu = CPU::new_with_mem_stop_fsm(DeterministicMemStopFsm::new(1));
    cpu.get_RF().write_fregs(1, fatptr_rf::new(15, 0));
    cpu.get_imem().flash_in(&[inst::LD128 { rd: 4, frs: 1 }]);

    cpu.tick();
    cpu.tick();

    let (ex_mem_next, signal, _) = cpu.eval_EX(&cpu.id_ex_rf);
    assert!(!ex_mem_next.is_valid());
    assert_eq!(signal.get_reason(), signal_reason::exception);
    assert!(signal.get_issuer_stage() == CPU_stages::EX);
}

#[test]
fn ex_agu_failure_drains_older_work_discards_younger_work_and_resumes() {
    let mut cpu = CPU::new_with_mem_stop_fsm(DeterministicMemStopFsm::new(1));
    cpu.get_RF().write_vregs(1, [2; 4]);
    cpu.get_RF().write_vregs(2, [3; 4]);
    cpu.get_RF().write_fregs(1, fatptr_rf::new(15, 0));
    cpu.get_imem().flash_in(&[
        inst::ADD128 {
            rd: 4,
            rs1: 1,
            rs2: 2,
        },
        inst::LD128 { rd: 6, frs: 1 },
        inst::ADD128 {
            rd: 5,
            rs1: 1,
            rs2: 2,
        },
        inst::JUMP { inst_imm: 3 },
    ]);

    for _ in 0..24 {
        cpu.tick();
    }

    assert_eq!(cpu.get_RF().read_vregs(4), [5; 4]);
    assert_eq!(cpu.get_RF().read_vregs(5), [0; 4]);
    assert_eq!(cpu.get_RF().read_vregs(6), [0; 4]);
    assert!(cpu.validation_probe().jump_id_completions > 0);
    assert!(!cpu.is_finished());
}

fn tick_and_record(cpu: &mut CPU, cycle: u64, mem_events: &mut Vec<u64>, wb_events: &mut Vec<u64>) {
    let before = cpu.validation_probe();
    cpu.tick();
    let after = cpu.validation_probe();

    for _ in before.mem_completions..after.mem_completions {
        mem_events.push(cycle);
    }
    for _ in before.wb_completions..after.wb_completions {
        wb_events.push(cycle);
    }
}

fn validate_pause_resume_case(
    memory_delay: u64,
    pause_delay: u64,
    resume_delay: u64,
) -> Option<String> {
    let program = [
        inst::LD128 { rd: 6, frs: 1 },
        inst::ADD128 {
            rd: 7,
            rs1: 2,
            rs2: 3,
        },
        inst::JUMP { inst_imm: 2 },
    ];
    let mut cpu = cpu_with_program(&program, memory_delay);
    cpu.set_external_signal_delays(pause_delay, resume_delay);

    let mut cycle = 0;
    let mut mem_events = Vec::new();
    let mut wb_events = Vec::new();
    for _ in 0..4 {
        cycle += 1;
        tick_and_record(&mut cpu, cycle, &mut mem_events, &mut wb_events);
    }

    let held_pc = cpu.get_RF().read_pc();
    cpu.signal_pause();

    let expected_mem = 4 + memory_delay;
    let expected_pause_ready = expected_mem + 1 + pause_delay;
    let expected_load_wb = expected_pause_ready + resume_delay + 1;
    let expected_follower_wb = expected_load_wb + 2;
    let mut pause_ready_cycle = None;
    let mut pc_drift_cycles = Vec::new();
    let mut pause_progress_cycles = Vec::new();
    let mut resume_progress_cycles = Vec::new();

    while cycle < expected_pause_ready + 24 {
        let memory_completed_before_tick = !mem_events.is_empty();
        let latch_updates_before = cpu.validation_probe().latch_updates;
        cycle += 1;
        tick_and_record(&mut cpu, cycle, &mut mem_events, &mut wb_events);
        if memory_completed_before_tick
            && cpu.validation_probe().latch_updates != latch_updates_before
        {
            pause_progress_cycles.push(cycle);
        }
        if cpu.get_RF().read_pc() != held_pc {
            pc_drift_cycles.push(cycle);
        }
        if cpu.ready4signal() {
            pause_ready_cycle = Some(cycle);
            break;
        }
    }

    let values_before_resume = (cpu.get_RF().read_vregs(6), cpu.get_RF().read_vregs(7));
    if pause_ready_cycle.is_some() {
        cpu.signal_resume();
    }

    for _ in 0..resume_delay {
        let latch_updates_before = cpu.validation_probe().latch_updates;
        cycle += 1;
        tick_and_record(&mut cpu, cycle, &mut mem_events, &mut wb_events);
        if cpu.validation_probe().latch_updates != latch_updates_before {
            resume_progress_cycles.push(cycle);
        }
        if cpu.get_RF().read_pc() != held_pc {
            pc_drift_cycles.push(cycle);
        }
    }

    let final_cycle = expected_follower_wb
        .max(cycle.saturating_add(16))
        .saturating_add(8);
    while cycle < final_cycle {
        cycle += 1;
        tick_and_record(&mut cpu, cycle, &mut mem_events, &mut wb_events);
    }

    let observed_wb = wb_events.clone();
    let final_load = cpu.get_RF().read_vregs(6);
    let final_follower = cpu.get_RF().read_vregs(7);
    let mut problems = Vec::new();

    if mem_events != [expected_mem] {
        problems.push(format!(
            "MEM expected [{expected_mem}], observed {mem_events:?}"
        ));
    }
    if pause_ready_cycle != Some(expected_pause_ready) {
        problems.push(format!(
            "pause-ready expected {expected_pause_ready}, observed {pause_ready_cycle:?}"
        ));
    }
    if values_before_resume != ([0; 4], [0; 4]) {
        problems.push(format!(
            "architectural update before resume: load/follower={values_before_resume:?}"
        ));
    }
    if observed_wb != [expected_load_wb, expected_follower_wb] {
        problems.push(format!(
            "WB expected [{expected_load_wb}, {expected_follower_wb}], observed {observed_wb:?}"
        ));
    }
    if final_load != LOAD_VALUE {
        problems.push(format!(
            "load result expected {LOAD_VALUE:?}, observed {final_load:?}"
        ));
    }
    if final_follower != FOLLOWER_VALUE {
        problems.push(format!(
            "follower result expected {FOLLOWER_VALUE:?}, observed {final_follower:?}"
        ));
    }
    if !pc_drift_cycles.is_empty() {
        problems.push(format!(
            "PC changed during held ticks at cycles {pc_drift_cycles:?}"
        ));
    }
    if !pause_progress_cycles.is_empty() {
        problems.push(format!(
            "pipeline latches advanced after MEM completion while waiting for pause at cycles {pause_progress_cycles:?}"
        ));
    }
    if !resume_progress_cycles.is_empty() {
        problems.push(format!(
            "pipeline latches advanced during resume hold at cycles {resume_progress_cycles:?}"
        ));
    }

    (!problems.is_empty()).then(|| {
        format!(
            "mem={memory_delay} pause={pause_delay} resume={resume_delay}: {}",
            problems.join("; ")
        )
    })
}

#[test]
fn pause_resume_timing_matches_deterministic_memory_matrix() {
    let mut failures = Vec::new();

    for memory_delay in [1, 2, 4, 8] {
        for pause_delay in [0, 1, 3] {
            for resume_delay in [0, 1, 3] {
                if let Some(failure) =
                    validate_pause_resume_case(memory_delay, pause_delay, resume_delay)
                {
                    failures.push(failure);
                }
            }
        }
    }

    assert!(
        failures.is_empty(),
        "pause/resume validation failures:\n{}",
        failures.join("\n")
    );
}
