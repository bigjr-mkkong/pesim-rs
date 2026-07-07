use crate::PE::types::inst as FGO_inst;
use crate::sim_engine::sim::engine_cfg;
use std::collections::{HashMap, HashSet};

#[derive(Default)]
struct FGO_harness_state {
    start_cycle: u64,
    vector_store_count: u64,
    passing_vector_store_count: u64,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct FGO_harness_summary {
    pub start_cycle: u64,
    pub end_cycle: u64,
    pub elapsed_cycles: u64,
    pub vector_store_count: u64,
    pub passing_vector_store_count: u64,
    pub result_passed: bool,
}

#[derive(Default)]
struct CGO_harness_state {
    start_cycle: u64,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct CGO_harness_summary {
    pub start_cycle: u64,
    pub end_cycle: u64,
    pub elapsed_cycles: u64,
    pub output_vector_count: u64,
    pub passing_output_vector_count: u64,
    pub result_passed: bool,
}

pub(crate) struct timing_harness {
    FGO_states: HashMap<engine_cfg, FGO_harness_state>,
    CGO_states: HashMap<engine_cfg, CGO_harness_state>,
    completed_CGO: HashSet<engine_cfg>,
}

impl timing_harness {
    pub fn new() -> Self {
        Self {
            FGO_states: HashMap::new(),
            CGO_states: HashMap::new(),
            completed_CGO: HashSet::new(),
        }
    }

    pub fn log_CGO_start(&mut self, cfg: engine_cfg, cycle: u64, req_id: u64) {
        let (ch, ra, bg, ba, pb) = CGO_coordinates(cfg);
        if self.completed_CGO.contains(&cfg) || self.CGO_states.contains_key(&cfg) {
            return;
        }

        self.CGO_states
            .insert(cfg, CGO_harness_state { start_cycle: cycle });
        println!(
            "CGO_TRACE event=start ch={ch} rank={ra} bank_group={bg} bank={ba} pseudo_bank={pb} cycle={cycle} req_id={req_id}"
        );
    }

    pub fn is_tracking_CGO(&self, cfg: engine_cfg) -> bool {
        self.CGO_states.contains_key(&cfg)
    }

    pub fn log_CGO_finish(
        &mut self,
        cfg: engine_cfg,
        cycle: u64,
        outputs: Option<Vec<[u32; 4]>>,
    ) -> Option<CGO_harness_summary> {
        let state = self.CGO_states.remove(&cfg)?;
        let (ch, ra, bg, ba, pb) = CGO_coordinates(cfg);
        let outputs = outputs.unwrap_or_default();
        let output_vector_count = outputs.len() as u64;
        let passing_output_vector_count = outputs
            .iter()
            .filter(|vector| vector.iter().any(|element| *element != 0))
            .count() as u64;
        let result_passed =
            output_vector_count > 0 && output_vector_count == passing_output_vector_count;
        let summary = CGO_harness_summary {
            start_cycle: state.start_cycle,
            end_cycle: cycle,
            elapsed_cycles: cycle.saturating_sub(state.start_cycle),
            output_vector_count,
            passing_output_vector_count,
            result_passed,
        };
        self.completed_CGO.insert(cfg);

        println!(
            "CGO_TIMING ch={ch} rank={ra} bank_group={bg} bank={ba} pseudo_bank={pb} start_cycle={} end_cycle={} elapsed_cycles={} output_vectors={} passing_output_vectors={} result_status={}",
            summary.start_cycle,
            summary.end_cycle,
            summary.elapsed_cycles,
            summary.output_vector_count,
            summary.passing_output_vector_count,
            if summary.result_passed {
                "PASS"
            } else {
                "FAIL"
            }
        );

        Some(summary)
    }

    pub fn log_FGO_receive(
        &mut self,
        cfg: engine_cfg,
        cycle: u64,
        req_id: u64,
        instruction: FGO_inst,
    ) {
        let (ch, ra, bg, ba, pb) = FGO_coordinates(cfg);
        self.FGO_states.entry(cfg).or_insert(FGO_harness_state {
            start_cycle: cycle,
            ..FGO_harness_state::default()
        });

        println!(
            "FGO_TRACE event=receive ch={ch} rank={ra} bank_group={bg} bank={ba} pseudo_bank={pb} cycle={cycle} req_id={req_id} instruction={}",
            describe_FGO_instruction(instruction)
        );
    }

    pub fn log_FGO_result(
        &mut self,
        cfg: engine_cfg,
        cycle: u64,
        req_id: u64,
        addr: u32,
        output: Option<[i16; 8]>,
    ) {
        let (ch, ra, bg, ba, pb) = FGO_coordinates(cfg);
        let all_zero = output
            .map(|vector| vector.iter().all(|element| *element == 0))
            .unwrap_or(true);
        let state = self.FGO_states.entry(cfg).or_insert_with(|| {
            eprintln!(
                "FGO_HARNESS_ERROR ch={ch} rank={ra} bank_group={bg} bank={ba} pseudo_bank={pb} result retired without a received command"
            );
            FGO_harness_state {
                start_cycle: cycle,
                ..FGO_harness_state::default()
            }
        });
        state.vector_store_count += 1;
        if !all_zero {
            state.passing_vector_store_count += 1;
        }

        println!(
            "FGO_RESULT ch={ch} rank={ra} bank_group={bg} bank={ba} pseudo_bank={pb} cycle={cycle} req_id={req_id} addr={addr} output={output:?} all_zero={all_zero} status={}",
            if all_zero { "FAIL" } else { "PASS" }
        );
    }

    pub fn log_FGO_retire(
        &mut self,
        cfg: engine_cfg,
        cycle: u64,
        req_id: u64,
        instruction: FGO_inst,
    ) -> Option<FGO_harness_summary> {
        let (ch, ra, bg, ba, pb) = FGO_coordinates(cfg);
        println!(
            "FGO_TRACE event=retire ch={ch} rank={ra} bank_group={bg} bank={ba} pseudo_bank={pb} cycle={cycle} req_id={req_id} instruction={}",
            describe_FGO_instruction(instruction)
        );

        if !matches!(instruction, FGO_inst::NOP) {
            return None;
        }

        let state = self.FGO_states.remove(&cfg).unwrap_or_else(|| {
            eprintln!(
                "FGO_HARNESS_ERROR ch={ch} rank={ra} bank_group={bg} bank={ba} pseudo_bank={pb} NOP retired without a received command"
            );
            FGO_harness_state {
                start_cycle: cycle,
                ..FGO_harness_state::default()
            }
        });
        let result_passed = state.vector_store_count > 0
            && state.vector_store_count == state.passing_vector_store_count;
        let summary = FGO_harness_summary {
            start_cycle: state.start_cycle,
            end_cycle: cycle,
            elapsed_cycles: cycle.saturating_sub(state.start_cycle),
            vector_store_count: state.vector_store_count,
            passing_vector_store_count: state.passing_vector_store_count,
            result_passed,
        };

        println!(
            "FGO_TIMING ch={ch} rank={ra} bank_group={bg} bank={ba} pseudo_bank={pb} start_cycle={} end_cycle={} elapsed_cycles={} vector_stores={} passing_vector_stores={} result_status={}",
            summary.start_cycle,
            summary.end_cycle,
            summary.elapsed_cycles,
            summary.vector_store_count,
            summary.passing_vector_store_count,
            if summary.result_passed {
                "PASS"
            } else {
                "FAIL"
            }
        );

        Some(summary)
    }
}

fn CGO_coordinates(cfg: engine_cfg) -> (u64, u64, u64, u64, u64) {
    match cfg {
        engine_cfg::CGO { ch, ra, bg, ba, pb } => (ch, ra, bg, ba, pb),
        engine_cfg::FGO { .. } => panic!("CGO harness received an FGO engine configuration"),
    }
}

fn FGO_coordinates(cfg: engine_cfg) -> (u64, u64, u64, u64, u64) {
    match cfg {
        engine_cfg::FGO { ch, ra, bg, ba, pb } => (ch, ra, bg, ba, pb),
        engine_cfg::CGO { .. } => panic!("FGO harness received a CGO engine configuration"),
    }
}

fn describe_FGO_instruction(instruction: FGO_inst) -> String {
    match instruction {
        FGO_inst::LD128 { vRD, addr } => format!("LD128(vRD={vRD},addr={addr})"),
        FGO_inst::ST128 { vRS, addr } => format!("ST128(vRS={vRS},addr={addr})"),
        FGO_inst::LD32 { sRD, addr } => format!("LD32(sRD={sRD},addr={addr})"),
        FGO_inst::ST32 { sRS, addr } => format!("ST32(sRS={sRS},addr={addr})"),
        FGO_inst::ADD128 { vRD, vRS0, vRS1 } => {
            format!("ADD128(vRD={vRD},vRS0={vRS0},vRS1={vRS1})")
        }
        FGO_inst::SUB128 { vRD, vRS0, vRS1 } => {
            format!("SUB128(vRD={vRD},vRS0={vRS0},vRS1={vRS1})")
        }
        FGO_inst::MUL128 { vRD, vRS0, vRS1 } => {
            format!("MUL128(vRD={vRD},vRS0={vRS0},vRS1={vRS1})")
        }
        FGO_inst::MAC128 {
            sRD,
            sRS0,
            vRS0,
            vRS1,
        } => format!("MAC128(sRD={sRD},sRS0={sRS0},vRS0={vRS0},vRS1={vRS1})"),
        FGO_inst::ReLU { vRD, vRS0 } => format!("ReLU(vRD={vRD},vRS0={vRS0})"),
        FGO_inst::NOP => "NOP".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CFG: engine_cfg = engine_cfg::FGO {
        ch: 0,
        ra: 0,
        bg: 1,
        ba: 2,
        pb: 3,
    };
    const CGO_CFG: engine_cfg = engine_cfg::CGO {
        ch: 0,
        ra: 0,
        bg: 1,
        ba: 2,
        pb: 3,
    };

    #[test]
    fn CGO_summary_uses_first_start_and_counts_nonzero_outputs() {
        let mut harness = timing_harness::new();
        harness.log_CGO_start(CGO_CFG, 10, 1);
        harness.log_CGO_start(CGO_CFG, 20, 2);

        let summary = harness
            .log_CGO_finish(CGO_CFG, 30, Some(vec![[1; 4], [0; 4], [2; 4]]))
            .expect("a tracked CGO engine should produce one summary");

        assert_eq!(
            summary,
            CGO_harness_summary {
                start_cycle: 10,
                end_cycle: 30,
                elapsed_cycles: 20,
                output_vector_count: 3,
                passing_output_vector_count: 2,
                result_passed: false,
            }
        );
        assert!(!harness.is_tracking_CGO(CGO_CFG));
        assert!(harness.log_CGO_finish(CGO_CFG, 31, None).is_none());
    }

    #[test]
    fn NOP_summary_includes_retirement_cycle_and_nonzero_result() {
        let mut harness = timing_harness::new();
        harness.log_FGO_receive(CFG, 10, 1, FGO_inst::LD128 { vRD: 0, addr: 0 });
        harness.log_FGO_receive(CFG, 11, 2, FGO_inst::ST128 { vRS: 0, addr: 2 });
        harness.log_FGO_result(CFG, 20, 2, 2, Some([1; 8]));
        harness.log_FGO_retire(CFG, 20, 2, FGO_inst::ST128 { vRS: 0, addr: 2 });

        let summary = harness
            .log_FGO_retire(CFG, 24, 3, FGO_inst::NOP)
            .expect("NOP should finish one harness interval");

        assert_eq!(
            summary,
            FGO_harness_summary {
                start_cycle: 10,
                end_cycle: 24,
                elapsed_cycles: 14,
                vector_store_count: 1,
                passing_vector_store_count: 1,
                result_passed: true,
            }
        );
    }

    #[test]
    fn all_zero_result_fails_the_NOP_summary() {
        let mut harness = timing_harness::new();
        harness.log_FGO_receive(CFG, 3, 1, FGO_inst::ST128 { vRS: 0, addr: 2 });
        harness.log_FGO_result(CFG, 8, 1, 2, Some([0; 8]));

        let summary = harness
            .log_FGO_retire(CFG, 9, 2, FGO_inst::NOP)
            .expect("NOP should finish one harness interval");

        assert!(!summary.result_passed);
        assert_eq!(summary.vector_store_count, 1);
        assert_eq!(summary.passing_vector_store_count, 0);
    }
}
