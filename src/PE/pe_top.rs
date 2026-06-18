/*
 * This directory describe the PE architecture for HBM-PIM liked PIM
 * A two cycle PE with no IF(directly receive instruction from host)
 *
 */

use crate::PE::EX::{EX_WB_RF, MEM_stop_FSM};
use crate::PE::ISSUE::ISSUE_EX_RF;
use crate::PE::RF::arch_rf;
use crate::PE::flat_mem::flat_mem;
use crate::PE::types::{PE_stages, arch_action, inst};
use crate::cpu::signal_scoreboard::pipeline_action;
use std::collections::HashSet;

pub struct PE {
    host_inst: inst,
    issue_ex_rf: ISSUE_EX_RF,
    ex_wb_forward_rf: EX_WB_RF,
    Arf: arch_rf,
    fmem: flat_mem,
    mem_stop_fsm: MEM_stop_FSM,
}

impl PE {
    pub fn new() -> Self {
        Self {
            host_inst: inst::NOP,
            issue_ex_rf: ISSUE_EX_RF::new(),
            ex_wb_forward_rf: EX_WB_RF::new(),
            Arf: arch_rf::new(),
            fmem: flat_mem::new(),
            mem_stop_fsm: MEM_stop_FSM::new(),
        }
    }

    pub fn set_host_inst(&mut self, host_inst: inst) {
        self.host_inst = host_inst;
    }

    pub fn get_Arf(&mut self) -> &mut arch_rf {
        &mut self.Arf
    }

    pub fn get_fmem(&mut self) -> &mut flat_mem {
        &mut self.fmem
    }

    pub fn tick(&mut self) {
        let issue_ex_snapshot = self.issue_ex_rf;
        let (ex_wb_next, ex_sigreq, ex_archop) = self.eval_EX(&issue_ex_snapshot, &self.fmem);
        let issue_ex_next = Self::eval_ISSUE(self.host_inst, &self.Arf);

        let pipeline_op = self.mem_stop_fsm.get_decision(ex_sigreq);
        let stage_action = |stage| {
            pipeline_op
                .get(&stage)
                .copied()
                .unwrap_or(pipeline_action::Normal)
        };

        if stage_action(PE_stages::EX) == pipeline_action::Normal {
            self.arch_update(ex_archop);
        }

        if stage_action(PE_stages::EX) == pipeline_action::Normal && ex_wb_next.is_valid() {
            self.ex_wb_forward_rf = ex_wb_next;
        }

        let stage_op = |producer_act, consumer_act| match (producer_act, consumer_act) {
            (_, pipeline_action::Stall) => pipeline_action::Stall,
            (pipeline_action::Normal, pipeline_action::Normal) => pipeline_action::Normal,
            (pipeline_action::Stall, pipeline_action::Normal) => pipeline_action::Flush,
            (_, pipeline_action::Flush | pipeline_action::END) => pipeline_action::Flush,
            (pipeline_action::Flush | pipeline_action::END, _) => pipeline_action::Flush,
        };

        match stage_op(stage_action(PE_stages::ISSUE), stage_action(PE_stages::EX)) {
            pipeline_action::Normal => self.issue_ex_rf = issue_ex_next,
            pipeline_action::Stall => {}
            pipeline_action::Flush | pipeline_action::END => self.issue_ex_rf = ISSUE_EX_RF::new(),
        }
    }

    fn arch_update(&mut self, op_vec: Vec<arch_action>) {
        let mut seen_dest = HashSet::new();
        let mut real_ops = Vec::new();

        for op in op_vec {
            let Some(dest) = op.dest() else {
                continue;
            };

            if !seen_dest.insert(dest) {
                panic!(
                    "PE arch update failed: duplicated architectural destination: {:?}",
                    dest
                );
            }

            real_ops.push(op);
        }

        for op in real_ops {
            match op {
                arch_action::WriteVRF { vRD, content } => self.Arf.write_vRF(vRD, content),
                arch_action::WriteSRF { sRD, content } => self.Arf.write_sRF(sRD, content),
                arch_action::WriteMEM_V { addr, content } => {
                    self.fmem.mem_write_v(addr, &content);
                }
                arch_action::WriteMEM_S { addr, content } => {
                    self.fmem.mem_write_s(addr, content);
                }
                arch_action::DoNothing => unreachable!("DoNothing was filtered out"),
            }
        }
    }
}
