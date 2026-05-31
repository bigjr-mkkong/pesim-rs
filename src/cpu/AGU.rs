use crate::cpu::pimcpu_types::{AGUop, ALUop, CPU_stages, DMAop, WBop, arch_action, fatptr_rf};
use crate::cpu::pipeline::CPU;

use crate::cpu::EX::EX_AGU_rf;
use crate::cpu::signal_scoreboard::{SigFSM, pipeline_action, signal_reason, signal_req};
use crate::memory::AGU_unit::AGU_unit;

use std::collections::{HashMap, HashSet};

pub struct AGU_MEM_rf {
    valid: bool,

    phys_addr: Option<u32>,
    arith_in: Option<[u32; 4]>,
    ptr_result: Option<fatptr_rf>,

    dma_op: DMAop,
    wb_op: WBop,
}

impl AGU_MEM_rf {
    pub const fn new() -> Self {
        Self {
            valid: false,
            phys_addr: None,
            arith_in: None,
            ptr_result: None,

            dma_op: DMAop::NOP,
            wb_op: WBop::NOP,
        }
    }

    pub fn is_valid(&self) -> bool {
        self.valid
    }

    pub fn invalidate(&mut self) {
        self.valid = false;
    }

    pub fn get_phys_addr(&self) -> Option<u32> {
        self.phys_addr
    }

    pub fn get_arith_in(&self) -> Option<[u32; 4]> {
        self.arith_in
    }

    pub fn get_ptr_result(&self) -> Option<fatptr_rf> {
        self.ptr_result
    }

    pub fn get_dma_op(&self) -> DMAop {
        self.dma_op.clone()
    }

    pub fn get_wb_op(&self) -> WBop {
        self.wb_op.clone()
    }
}

impl CPU {
    pub fn eval_AGU(
        &self,
        ex_agu_rf: &EX_AGU_rf,
        agu: &AGU_unit,
    ) -> (AGU_MEM_rf, signal_req, Vec<arch_action>) {
        if !ex_agu_rf.is_valid() {
            (
                AGU_MEM_rf {
                    valid: false,
                    phys_addr: None,
                    arith_in: None,
                    ptr_result: None,
                    dma_op: DMAop::NOP,
                    wb_op: WBop::NOP,
                },
                signal_req::new(signal_reason::no_reason, CPU_stages::AGU, None),
                [arch_action::DoNothing].to_vec(),
            )
        } else {
            match ex_agu_rf.get_agu_op() {
                AGUop::NOP => (
                    AGU_MEM_rf {
                        valid: true,
                        phys_addr: None,
                        arith_in: None,
                        ptr_result: None,
                        dma_op: DMAop::NOP,
                        wb_op: WBop::NOP,
                    },
                    signal_req::new(signal_reason::no_reason, CPU_stages::AGU, None),
                    [arch_action::DoNothing].to_vec(),
                ),
                AGUop::CHK { fptr_lit } => {
                    if agu.accept(fptr_lit) {
                        (
                            AGU_MEM_rf {
                                valid: true,
                                phys_addr: agu.translate(fptr_lit),
                                arith_in: ex_agu_rf.get_arith_result(),
                                ptr_result: None,
                                dma_op: ex_agu_rf.get_dma_op(),
                                wb_op: ex_agu_rf.get_wb_op(),
                            },
                            signal_req::new(signal_reason::no_reason, CPU_stages::AGU, None),
                            [arch_action::DoNothing].to_vec(),
                        )
                    } else {
                        (
                            AGU_MEM_rf {
                                valid: false,
                                phys_addr: None,
                                arith_in: None,
                                ptr_result: None,
                                dma_op: DMAop::NOP,
                                wb_op: WBop::NOP,
                            },
                            signal_req::new(
                                signal_reason::exception,
                                CPU_stages::AGU,
                                Some(HashSet::<CPU_stages>::from([
                                    CPU_stages::IF,
                                    CPU_stages::ID,
                                    CPU_stages::EX,
                                ])),
                            ),
                            [arch_action::HoldPC].to_vec(),
                        )
                    }
                }
                AGUop::ADD {
                    fptr_lit,
                    rs1_lit,
                    idx_imm,
                } => {
                    let fptr_ = agu.addition(fptr_lit, rs1_lit, idx_imm);
                    if let Some(new_fptr) = fptr_ {
                        (
                            AGU_MEM_rf {
                                valid: true,
                                phys_addr: None,
                                arith_in: ex_agu_rf.get_arith_result(),
                                ptr_result: Some(new_fptr),
                                dma_op: ex_agu_rf.get_dma_op(),
                                wb_op: ex_agu_rf.get_wb_op(),
                            },
                            signal_req::new(signal_reason::no_reason, CPU_stages::AGU, None),
                            [arch_action::DoNothing].to_vec(),
                        )
                    } else {
                        (
                            AGU_MEM_rf {
                                valid: false,
                                phys_addr: None,
                                arith_in: None,
                                ptr_result: None,
                                dma_op: DMAop::NOP,
                                wb_op: WBop::NOP,
                            },
                            signal_req::new(
                                signal_reason::exception,
                                CPU_stages::AGU,
                                Some(HashSet::<CPU_stages>::from([
                                    CPU_stages::IF,
                                    CPU_stages::ID,
                                    CPU_stages::EX,
                                ])),
                            ),
                            [arch_action::HoldPC].to_vec(),
                        )
                    }
                }
                AGUop::SUB {
                    fptr_lit,
                    rs1_lit,
                    idx_imm,
                } => {
                    let fptr_ = agu.subtraction(fptr_lit, rs1_lit, idx_imm);
                    if let Some(new_fptr) = fptr_ {
                        (
                            AGU_MEM_rf {
                                valid: true,
                                phys_addr: None,
                                arith_in: ex_agu_rf.get_arith_result(),
                                ptr_result: Some(new_fptr),
                                dma_op: ex_agu_rf.get_dma_op(),
                                wb_op: ex_agu_rf.get_wb_op(),
                            },
                            signal_req::new(signal_reason::no_reason, CPU_stages::AGU, None),
                            [arch_action::DoNothing].to_vec(),
                        )
                    } else {
                        (
                            AGU_MEM_rf {
                                valid: false,
                                phys_addr: None,
                                arith_in: None,
                                ptr_result: None,
                                dma_op: DMAop::NOP,
                                wb_op: WBop::NOP,
                            },
                            signal_req::new(
                                signal_reason::exception,
                                CPU_stages::AGU,
                                Some(HashSet::<CPU_stages>::from([
                                    CPU_stages::IF,
                                    CPU_stages::ID,
                                    CPU_stages::EX,
                                ])),
                            ),
                            [arch_action::HoldPC].to_vec(),
                        )
                    }
                }
            }
        }
    }
}

#[derive(Clone, Copy)]
pub enum AGU_stop_FSM_states {
    Drain_WB,
    Drain_MEM,
    Idle,
}

#[derive(Clone, Copy)]
pub struct AGU_stop_FSM {
    state: AGU_stop_FSM_states,
    state_next: AGU_stop_FSM_states,
}

impl SigFSM for AGU_stop_FSM {
    fn reason(&self) -> signal_reason {
        signal_reason::exception
    }

    //action should return Normal when reaching the finish state
    fn action(&self) -> pipeline_action {
        match self.state {
            AGU_stop_FSM_states::Drain_WB => pipeline_action::Flush,
            AGU_stop_FSM_states::Drain_MEM => pipeline_action::Flush,
            AGU_stop_FSM_states::Idle => pipeline_action::Normal,
        }
    }

    fn get_ops(&self) -> HashMap<CPU_stages, pipeline_action> {
        HashMap::<CPU_stages, pipeline_action>::from([
            (CPU_stages::IF, pipeline_action::Flush), //flush ifid
            (CPU_stages::ID, pipeline_action::Flush), //flush idex
            (CPU_stages::EX, pipeline_action::Flush), //flush exagu
        ])
    }

    fn advance_winner(&mut self) -> bool {
        self.state_next = match self.state {
            AGU_stop_FSM_states::Drain_WB => AGU_stop_FSM_states::Drain_MEM,
            AGU_stop_FSM_states::Drain_MEM => AGU_stop_FSM_states::Idle,
            AGU_stop_FSM_states::Idle => AGU_stop_FSM_states::Idle,
        };

        self.state = self.state_next;
        return true;
    }

    fn handle_blocked(&mut self) {}
}

impl AGU_stop_FSM {
    pub const fn new() -> Self {
        Self {
            state: AGU_stop_FSM_states::Drain_WB,
            state_next: AGU_stop_FSM_states::Drain_MEM,
        }
    }
}
