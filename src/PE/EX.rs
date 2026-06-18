/*
 * This is the PE execute stage.  PE is a two-stage pipeline, so EX performs
 * ALU, memory, and writeback work and produces architectural operations for
 * pe_top.rs to commit after pipeline-control arbitration.
 */

use crate::PE::ALU::{ALU_comp, ALU_out};
use crate::PE::ISSUE::ISSUE_EX_RF;
use crate::PE::flat_mem::flat_mem;
use crate::PE::pe_top::PE;
use crate::PE::types::{ALUop, MEMop, PE_stages, WBop, arch_action};
use crate::cpu::signal_scoreboard::{pipeline_action, signal_reason};
use std::collections::HashMap;

#[derive(Clone, Copy)]
pub struct EX_WB_RF {
    valid: bool,
    v_result: Option<[i16; 8]>,
    s_result: Option<i32>,
    wbop: WBop,
}

impl EX_WB_RF {
    pub const fn new() -> Self {
        Self {
            valid: false,
            v_result: None,
            s_result: None,
            wbop: WBop::NOP,
        }
    }

    pub fn is_valid(&self) -> bool {
        self.valid
    }

    pub fn invalidate(&mut self) {
        self.valid = false;
    }

    pub fn get_v_result(&self) -> Option<[i16; 8]> {
        self.v_result
    }

    pub fn get_s_result(&self) -> Option<i32> {
        self.s_result
    }

    pub fn get_wbop(&self) -> WBop {
        self.wbop
    }
}

impl PE {
    pub fn eval_EX(
        &self,
        issue_ex_rf: &ISSUE_EX_RF,
        fmem: &flat_mem,
    ) -> (EX_WB_RF, signal_reason, Vec<arch_action>) {
        let bypassed_aluop = self.ex_bypass_aluop(issue_ex_rf.get_aluop());
        let mut ex_wb_next = EX_WB_RF {
            valid: true,
            v_result: None,
            s_result: None,
            wbop: issue_ex_rf.get_wbop(),
        };

        match ALU_comp(bypassed_aluop) {
            ALU_out::vec_out { lit } => ex_wb_next.v_result = Some(lit),
            ALU_out::scalar_out { lit } => ex_wb_next.s_result = Some(lit),
            ALU_out::NA => {}
        }

        let mut arch_ops = Vec::new();
        let mut sig_reason = signal_reason::no_reason;

        match issue_ex_rf.get_memop() {
            MEMop::NOP => {}
            MEMop::ReadV { addr } => {
                ex_wb_next.v_result = fmem.mem_read_v(addr);
                sig_reason = signal_reason::MEM_block {
                    addr: addr as u64,
                    is_read: true,
                };
            }
            MEMop::WriteV { addr, vRS, data } => {
                let data = self.ex_bypass_get_vreg(vRS, data);
                arch_ops.push(arch_action::WriteMEM_V {
                    addr,
                    content: data,
                });
                sig_reason = signal_reason::MEM_block {
                    addr: addr as u64,
                    is_read: false,
                };
            }
            MEMop::ReadS { addr } => {
                ex_wb_next.s_result = fmem.mem_read_s(addr);
                sig_reason = signal_reason::MEM_block {
                    addr: addr as u64,
                    is_read: true,
                };
            }
            MEMop::WriteS { addr, sRS, data } => {
                let data = self.ex_bypass_get_sreg(sRS, data);
                arch_ops.push(arch_action::WriteMEM_S {
                    addr,
                    content: data,
                });
                sig_reason = signal_reason::MEM_block {
                    addr: addr as u64,
                    is_read: false,
                };
            }
        }

        match ex_wb_next.get_wbop() {
            WBop::NOP => {}
            WBop::VWrite { vRD } => {
                if let Some(content) = ex_wb_next.get_v_result() {
                    arch_ops.push(arch_action::WriteVRF { vRD, content });
                }
            }
            WBop::SWrite { sRD } => {
                if let Some(content) = ex_wb_next.get_s_result() {
                    arch_ops.push(arch_action::WriteSRF { sRD, content });
                }
            }
        }

        if arch_ops.is_empty() {
            arch_ops.push(arch_action::DoNothing);
        }

        (ex_wb_next, sig_reason, arch_ops)
    }

    fn ex_bypass_aluop(&self, aluop: ALUop) -> ALUop {
        match aluop {
            ALUop::ADD {
                vRS0,
                vRS1,
                vRS0_lit,
                vRS1_lit,
            } => ALUop::ADD {
                vRS0,
                vRS1,
                vRS0_lit: self.ex_bypass_get_vreg(vRS0, vRS0_lit),
                vRS1_lit: self.ex_bypass_get_vreg(vRS1, vRS1_lit),
            },
            ALUop::SUB {
                vRS0,
                vRS1,
                vRS0_lit,
                vRS1_lit,
            } => ALUop::SUB {
                vRS0,
                vRS1,
                vRS0_lit: self.ex_bypass_get_vreg(vRS0, vRS0_lit),
                vRS1_lit: self.ex_bypass_get_vreg(vRS1, vRS1_lit),
            },
            ALUop::MUL {
                vRS0,
                vRS1,
                vRS0_lit,
                vRS1_lit,
            } => ALUop::MUL {
                vRS0,
                vRS1,
                vRS0_lit: self.ex_bypass_get_vreg(vRS0, vRS0_lit),
                vRS1_lit: self.ex_bypass_get_vreg(vRS1, vRS1_lit),
            },
            ALUop::MAC {
                sRS0,
                vRS0,
                vRS1,
                sRS0_lit,
                vRS0_lit,
                vRS1_lit,
            } => ALUop::MAC {
                sRS0,
                vRS0,
                vRS1,
                sRS0_lit: self.ex_bypass_get_sreg(sRS0, sRS0_lit),
                vRS0_lit: self.ex_bypass_get_vreg(vRS0, vRS0_lit),
                vRS1_lit: self.ex_bypass_get_vreg(vRS1, vRS1_lit),
            },
            ALUop::ReLU { vRS0, vRS0_lit } => ALUop::ReLU {
                vRS0,
                vRS0_lit: self.ex_bypass_get_vreg(vRS0, vRS0_lit),
            },
            ALUop::NOP => ALUop::NOP,
        }
    }
}

#[derive(Clone, Copy)]
enum MEM_stop_FSM_states {
    Submit,
    Stall,
    WriteBack,
    Release,
    Idle,
}

pub struct MEM_stop_FSM {
    state: MEM_stop_FSM_states,
    // Should also contain a dram_port copy for issue
}

impl MEM_stop_FSM {
    pub fn new() -> Self {
        Self {
            state: MEM_stop_FSM_states::Idle,
        }
    }

    pub fn get_decision(
        &mut self,
        sig_reason: signal_reason,
    ) -> HashMap<PE_stages, pipeline_action> {
        if self.is_idle() && matches!(sig_reason, signal_reason::MEM_block { .. }) {
            self.state = MEM_stop_FSM_states::Submit;
        }

        let ops = self.get_ops();
        self.advance();
        ops
    }

    fn is_idle(&self) -> bool {
        matches!(self.state, MEM_stop_FSM_states::Idle)
    }

    fn get_ops(&self) -> HashMap<PE_stages, pipeline_action> {
        match self.state {
            MEM_stop_FSM_states::Submit | MEM_stop_FSM_states::Stall => HashMap::from([
                (PE_stages::ISSUE, pipeline_action::Stall),
                (PE_stages::EX, pipeline_action::Stall),
            ]),
            MEM_stop_FSM_states::WriteBack => {
                HashMap::from([(PE_stages::ISSUE, pipeline_action::Stall)])
            }
            MEM_stop_FSM_states::Release => {
                HashMap::from([(PE_stages::ISSUE, pipeline_action::Stall)])
            }
            MEM_stop_FSM_states::Idle => HashMap::new(),
        }
    }

    fn advance(&mut self) {
        self.state = match self.state {
            MEM_stop_FSM_states::Submit | MEM_stop_FSM_states::Stall => {
                if rand::random_bool(0.7) {
                    MEM_stop_FSM_states::Stall
                } else {
                    MEM_stop_FSM_states::WriteBack
                }
            }
            MEM_stop_FSM_states::WriteBack => MEM_stop_FSM_states::Release,
            MEM_stop_FSM_states::Release => MEM_stop_FSM_states::Idle,
            MEM_stop_FSM_states::Idle => MEM_stop_FSM_states::Idle,
        };
    }
}
