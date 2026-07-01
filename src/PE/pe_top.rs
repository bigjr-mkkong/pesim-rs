/*
 * This directory describe the PE architecture for HBM-PIM liked PIM
 * A two cycle PE with no IF(directly receive instruction from host)
 *
 */

use crate::PE::EX::{EX_WB_RF, PE_MEM_stop_FSM};
use crate::PE::ISSUE::ISSUE_EX_RF;
use crate::PE::RF::arch_rf;
use crate::PE::types::{PE_stages, arch_action, inst};
use crate::cpu::signal_scoreboard::pipeline_action;
use crate::memory::flat_memory::pe_flat_mem;
use crate::memory::mem_portal::dram_portal;
use crate::memory::mem_portal::dram_req;
use std::collections::{HashSet, VecDeque};

pub struct PE {
    imem: VecDeque<(inst, Option<dram_req>)>,
    completed_reqs: VecDeque<dram_req>,
    fetch_next_allowed: bool,
    finished: bool,
    issue_ex_rf: ISSUE_EX_RF,
    pub(crate) ex_wb_forward_rf: EX_WB_RF,
    Arf: arch_rf,
    fmem: pe_flat_mem,
    mem_stop_fsm: PE_MEM_stop_FSM,
}

impl PE {
    fn build(mem_stop_fsm: PE_MEM_stop_FSM) -> Self {
        Self {
            imem: VecDeque::new(),
            completed_reqs: VecDeque::new(),
            fetch_next_allowed: false,
            finished: false,
            issue_ex_rf: ISSUE_EX_RF::new(),
            ex_wb_forward_rf: EX_WB_RF::new(),
            Arf: arch_rf::new(),
            fmem: pe_flat_mem::new(),
            mem_stop_fsm,
        }
    }

    #[cfg(test)]
    pub fn new() -> Self {
        Self::build(PE_MEM_stop_FSM::new())
    }

    pub fn new_with_dram_port(dram_port: dram_portal) -> Self {
        Self::build(PE_MEM_stop_FSM::new_with_dram_port(dram_port))
    }

    #[cfg(test)]
    pub fn push_host_inst(&mut self, host_inst: inst) {
        self.imem.push_back((host_inst, None));
    }

    pub fn push_host_req(&mut self, req: dram_req, host_inst: inst) {
        self.imem.push_back((host_inst, Some(req)));
    }

    pub fn take_completed(&mut self) -> Option<dram_req> {
        self.completed_reqs.pop_front()
    }

    pub fn has_complete(&self) -> bool {
        !self.completed_reqs.is_empty()
    }

    pub fn allow_next(&mut self) {
        self.fetch_next_allowed = true;
    }

    pub fn has_buffered_inst(&self) -> bool {
        !self.imem.is_empty()
    }

    pub fn has_finished(&mut self) -> bool {
        let finished = self.finished;
        self.finished = false;
        finished
    }

    fn peek_fetch_inst(&self) -> Option<(inst, Option<dram_req>)> {
        if self.fetch_next_allowed {
            self.imem.front().cloned()
        } else {
            None
        }
    }

    fn consume_fetch_inst(&mut self) {
        if self.fetch_next_allowed {
            self.fetch_next_allowed = false;
            self.imem.pop_front();
        }
    }

    #[cfg(test)]
    pub(crate) fn get_Arf(&mut self) -> &mut arch_rf {
        &mut self.Arf
    }

    #[cfg(test)]
    pub(crate) fn get_fmem(&mut self) -> &mut pe_flat_mem {
        &mut self.fmem
    }

    pub fn tick(&mut self) {
        let issue_ex_next = Self::eval_ISSUE(self.peek_fetch_inst(), &self.Arf);
        let (ex_wb_next, ex_sigreq, ex_archop) = self.eval_EX(&self.issue_ex_rf, &self.fmem);

        let pipeline_op = self.mem_stop_fsm.get_decision(ex_sigreq);
        let stage_action = |stage| {
            pipeline_op
                .get(&stage)
                .copied()
                .unwrap_or(pipeline_action::Normal)
        };

        let mut arch_ops = Vec::new();
        if stage_action(PE_stages::EX) == pipeline_action::Normal {
            arch_ops.extend(ex_archop);
        }
        self.arch_update(arch_ops);

        if stage_action(PE_stages::EX) == pipeline_action::Normal {
            if self.issue_ex_rf.is_valid() {
                self.finished = true;
            }

            if let Some(req) = self.issue_ex_rf.get_sim_req() {
                self.completed_reqs.push_back(req);
            }

            if ex_wb_next.is_valid() {
                self.ex_wb_forward_rf = ex_wb_next;
            }
        }

        let stage_op = |producer_act, consumer_act| match (producer_act, consumer_act) {
            (_, pipeline_action::Stall) => pipeline_action::Stall,
            (pipeline_action::Normal, pipeline_action::Normal) => pipeline_action::Normal,
            (pipeline_action::Stall, pipeline_action::Normal) => pipeline_action::Flush,
            (_, pipeline_action::Flush | pipeline_action::END) => pipeline_action::Flush,
            (pipeline_action::Flush | pipeline_action::END, _) => pipeline_action::Flush,
        };

        match stage_op(stage_action(PE_stages::ISSUE), stage_action(PE_stages::EX)) {
            pipeline_action::Normal => {
                self.consume_fetch_inst();
                self.issue_ex_rf = issue_ex_next;
            }
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
