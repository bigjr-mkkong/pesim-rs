use crate::cpu::pimcpu_types::{CPU_stages, DMAop, WBop, arch_action, fatptr_rf};
use crate::cpu::pipeline::CPU;
use crate::cpu::signal_scoreboard::{SigFSM, pipeline_action, signal_reason, signal_req};

use crate::cpu::AGU::AGU_MEM_rf;
use std::collections::{HashMap, HashSet};

use crate::memory::flat_memory::flat_mem;

pub struct MEM_WB_RF {
    valid: bool,

    arith_result: Option<[u32; 4]>,
    ptr_result: Option<fatptr_rf>,

    wb_op: WBop,
}

impl MEM_WB_RF {
    pub const fn new() -> Self {
        Self {
            valid: false,

            arith_result: None,
            ptr_result: None,
            wb_op: WBop::NOP,
        }
    }

    pub fn is_valid(&self) -> bool {
        self.valid
    }

    pub fn invalidate(&mut self) {
        self.valid = false;
    }

    pub fn get_arith_result(&self) -> Option<[u32; 4]> {
        self.arith_result
    }

    pub fn get_ptr_result(&self) -> Option<fatptr_rf> {
        self.ptr_result
    }

    pub fn get_wb_op(&self) -> WBop {
        self.wb_op
    }
}

impl CPU {
    pub fn eval_MEM(
        &self,
        agu_mem_rf: &AGU_MEM_rf,
        fmem: &flat_mem,
    ) -> (MEM_WB_RF, signal_req, Vec<arch_action>) {
        if !agu_mem_rf.is_valid() {
            (
                MEM_WB_RF {
                    valid: false,
                    arith_result: None,
                    ptr_result: None,
                    wb_op: WBop::NOP,
                },
                signal_req::new(signal_reason::no_reason, CPU_stages::MEM, None),
                [arch_action::DoNothing].to_vec(),
            )
        } else {
            match agu_mem_rf.get_dma_op() {
                DMAop::NOP => (
                    MEM_WB_RF {
                        valid: true,
                        arith_result: agu_mem_rf.get_arith_result(),
                        ptr_result: agu_mem_rf.get_ptr_result(),
                        wb_op: agu_mem_rf.get_wb_op(),
                    },
                    signal_req::new(signal_reason::no_reason, CPU_stages::MEM, None),
                    [arch_action::DoNothing].to_vec(),
                ),
                DMAop::READ_VEC { .. } => {
                    if let Some(paddr) = agu_mem_rf.get_phys_addr() {
                        (
                            MEM_WB_RF {
                                valid: true,
                                arith_result: fmem.mem_read_data(paddr),
                                ptr_result: None,
                                wb_op: agu_mem_rf.get_wb_op(),
                            },
                            signal_req::new(
                                signal_reason::MEM_block,
                                CPU_stages::MEM,
                                Some(HashSet::<CPU_stages>::from([
                                    CPU_stages::IF,
                                    CPU_stages::ID,
                                    CPU_stages::EX,
                                    CPU_stages::AGU,
                                ])),
                            ),
                            [arch_action::DoNothing].to_vec(),
                        )
                    } else {
                        (
                            MEM_WB_RF {
                                valid: true,
                                arith_result: None,
                                ptr_result: None,
                                wb_op: WBop::NOP,
                            },
                            signal_req::new(signal_reason::no_reason, CPU_stages::MEM, None),
                            [arch_action::DoNothing].to_vec(),
                        )
                    }
                }
                DMAop::WRITE_VEC { data_lit, .. } => {
                    if let Some(paddr) = agu_mem_rf.get_phys_addr() {
                        (
                            MEM_WB_RF {
                                valid: true,
                                arith_result: None,
                                ptr_result: None,
                                wb_op: WBop::NOP,
                            },
                            signal_req::new(signal_reason::no_reason, CPU_stages::MEM, None),
                            [arch_action::WriteMEM_DATA {
                                addr: paddr,
                                content: data_lit,
                            }]
                            .to_vec(),
                        )
                    } else {
                        (
                            MEM_WB_RF {
                                valid: true,
                                arith_result: None,
                                ptr_result: None,
                                wb_op: WBop::NOP,
                            },
                            signal_req::new(signal_reason::no_reason, CPU_stages::MEM, None),
                            [arch_action::DoNothing].to_vec(),
                        )
                    }
                }
                DMAop::READ_FPTR { .. } => {
                    if let Some(paddr) = agu_mem_rf.get_phys_addr() {
                        (
                            MEM_WB_RF {
                                valid: true,
                                arith_result: None,
                                ptr_result: fmem.mem_read_fptr(paddr),
                                wb_op: agu_mem_rf.get_wb_op(),
                            },
                            signal_req::new(
                                signal_reason::MEM_block,
                                CPU_stages::MEM,
                                Some(HashSet::<CPU_stages>::from([
                                    CPU_stages::IF,
                                    CPU_stages::ID,
                                    CPU_stages::EX,
                                    CPU_stages::AGU,
                                ])),
                            ),
                            [arch_action::DoNothing].to_vec(),
                        )
                    } else {
                        (
                            MEM_WB_RF {
                                valid: true,
                                arith_result: None,
                                ptr_result: None,
                                wb_op: WBop::NOP,
                            },
                            signal_req::new(signal_reason::no_reason, CPU_stages::MEM, None),
                            [arch_action::DoNothing].to_vec(),
                        )
                    }
                }
                DMAop::WRITE_FPTR { fptr_data_lit, .. } => {
                    if let Some(paddr) = agu_mem_rf.get_phys_addr() {
                        (
                            MEM_WB_RF {
                                valid: true,
                                arith_result: None,
                                ptr_result: None,
                                wb_op: WBop::NOP,
                            },
                            signal_req::new(signal_reason::no_reason, CPU_stages::MEM, None),
                            [arch_action::WriteMEM_FPTR {
                                addr: paddr,
                                content: fptr_data_lit,
                            }]
                            .to_vec(),
                        )
                    } else {
                        (
                            MEM_WB_RF {
                                valid: true,
                                arith_result: None,
                                ptr_result: None,
                                wb_op: WBop::NOP,
                            },
                            signal_req::new(signal_reason::no_reason, CPU_stages::MEM, None),
                            [arch_action::DoNothing].to_vec(),
                        )
                    }
                }
            }
        }
    }
}

#[derive(Clone, Copy)]
pub enum MEM_stop_FSM_states {
    STALL,
    Idle,
}

#[derive(Clone, Copy)]
pub struct MEM_stop_FSM {
    state: MEM_stop_FSM_states,
    state_next: MEM_stop_FSM_states,
}

impl SigFSM for MEM_stop_FSM {
    fn reason(&self) -> signal_reason {
        signal_reason::MEM_block
    }

    //action should return Normal when reaching the finish state
    fn action(&self) -> pipeline_action {
        match self.state {
            MEM_stop_FSM_states::STALL => pipeline_action::Stall,
            MEM_stop_FSM_states::Idle => pipeline_action::Normal,
        }
    }

    fn get_ops(&self) -> HashMap<CPU_stages, pipeline_action> {
        HashMap::<CPU_stages, pipeline_action>::from([
            (CPU_stages::IF, pipeline_action::Stall),  //stall ifid
            (CPU_stages::ID, pipeline_action::Stall),  //stall idex
            (CPU_stages::EX, pipeline_action::Stall),  //stall exagu
            (CPU_stages::AGU, pipeline_action::Stall), //stall agumem
        ])
    }

    fn advance_winner(&mut self) -> bool {
        let op_finished = true; //For ideal MEM operation always finished immediately
        self.state_next = match self.state {
            MEM_stop_FSM_states::STALL => {
                if op_finished {
                    MEM_stop_FSM_states::Idle
                } else {
                    MEM_stop_FSM_states::STALL
                }
            }
            MEM_stop_FSM_states::Idle => MEM_stop_FSM_states::Idle,
        };

        self.state = self.state_next;
        return true;
    }

    fn handle_blocked(&mut self) {}
}

impl MEM_stop_FSM {
    pub const fn new() -> Self {
        Self {
            state: MEM_stop_FSM_states::STALL,
            state_next: MEM_stop_FSM_states::STALL,
        }
    }
}
