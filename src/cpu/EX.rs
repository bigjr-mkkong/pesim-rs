use crate::cpu::pimcpu_types::{AGUop, ALUop, CPU_stages, DMAop, WBop, arch_action};
use crate::cpu::pipeline::CPU;

use crate::cpu::ID::ID_EX_rf;
use crate::cpu::signal_scoreboard::{SigFSM, pipeline_action, signal_reason, signal_req};

use std::collections::{HashMap, HashSet};
pub struct EX_AGU_rf {
    valid: bool,

    arith_result: Option<[u32; 4]>,

    agu_op: AGUop,
    dma_op: DMAop,
    wb_op: WBop,
}

impl EX_AGU_rf {
    pub const fn new() -> Self {
        Self {
            valid: false,
            arith_result: None,
            agu_op: AGUop::NOP,
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

    pub fn get_arith_result(&self) -> Option<[u32; 4]> {
        self.arith_result
    }

    pub fn get_agu_op(&self) -> AGUop {
        self.agu_op
    }

    pub fn get_dma_op(&self) -> DMAop {
        self.dma_op
    }

    pub fn get_wb_op(&self) -> WBop {
        self.wb_op
    }
}

impl CPU {
    pub fn eval_EX(&self, idex_rf: &ID_EX_rf) -> (EX_AGU_rf, signal_req, Vec<arch_action>) {
        let raw_stall_from_ex = || {
            let ex_agu_next = EX_AGU_rf {
                valid: false,
                arith_result: None,
                agu_op: AGUop::NOP,
                dma_op: DMAop::NOP,
                wb_op: WBop::NOP,
            };
            (
                ex_agu_next,
                signal_req::new(
                    signal_reason::RAW_resolution,
                    CPU_stages::EX,
                    Some(HashSet::<CPU_stages>::from([
                        CPU_stages::IF,
                        CPU_stages::ID,
                        CPU_stages::EX,
                    ])),
                ),
                [arch_action::DoNothing].to_vec(),
            )
        };

        if !idex_rf.is_valid() {
            (
                EX_AGU_rf {
                    valid: false,
                    arith_result: None,
                    agu_op: AGUop::NOP,
                    dma_op: DMAop::NOP,
                    wb_op: WBop::NOP,
                },
                signal_req::new(signal_reason::no_reason, CPU_stages::EX, None),
                [arch_action::DoNothing].to_vec(),
            )
        } else {
            match idex_rf.get_alu_op() {
                ALUop::NOP => (
                    EX_AGU_rf {
                        valid: true,
                        arith_result: None,
                        agu_op: idex_rf.get_agu_op(),
                        dma_op: idex_rf.get_dma_op(),
                        wb_op: idex_rf.get_wb_op(),
                    },
                    signal_req::new(signal_reason::no_reason, CPU_stages::EX, None),
                    [arch_action::DoNothing].to_vec(),
                ),
                ALUop::ADD {
                    rs1,
                    rs2,
                    rs1_lit,
                    rs2_lit,
                } => {
                    let Some(rs1_lit) = self.ex_bypass_get_rs1(rs1, rs1_lit) else {
                        return raw_stall_from_ex();
                    };
                    let Some(rs2_lit) = self.ex_bypass_get_rs2(rs2, rs2_lit) else {
                        return raw_stall_from_ex();
                    };
                    let mut tmp_result: [u32; 4] = [0; 4];

                    for i in 0..4 {
                        tmp_result[i] = rs1_lit[i] + rs2_lit[i];
                    }

                    (
                        EX_AGU_rf {
                            valid: true,
                            arith_result: Some(tmp_result),
                            agu_op: idex_rf.get_agu_op(),
                            dma_op: idex_rf.get_dma_op(),
                            wb_op: idex_rf.get_wb_op(),
                        },
                        signal_req::new(signal_reason::no_reason, CPU_stages::EX, None),
                        [arch_action::DoNothing].to_vec(),
                    )
                }
                ALUop::SUB {
                    rs1,
                    rs2,
                    rs1_lit,
                    rs2_lit,
                } => {
                    let Some(rs1_lit) = self.ex_bypass_get_rs1(rs1, rs1_lit) else {
                        return raw_stall_from_ex();
                    };
                    let Some(rs2_lit) = self.ex_bypass_get_rs2(rs2, rs2_lit) else {
                        return raw_stall_from_ex();
                    };
                    let mut tmp_result: [u32; 4] = [0; 4];

                    for i in 0..4 {
                        tmp_result[i] = rs1_lit[i] - rs2_lit[i];
                    }

                    (
                        EX_AGU_rf {
                            valid: true,
                            arith_result: Some(tmp_result),
                            agu_op: idex_rf.get_agu_op(),
                            dma_op: idex_rf.get_dma_op(),
                            wb_op: idex_rf.get_wb_op(),
                        },
                        signal_req::new(signal_reason::no_reason, CPU_stages::EX, None),
                        [arch_action::DoNothing].to_vec(),
                    )
                }
                ALUop::AND {
                    rs1,
                    rs2,
                    rs1_lit,
                    rs2_lit,
                } => {
                    let Some(rs1_lit) = self.ex_bypass_get_rs1(rs1, rs1_lit) else {
                        return raw_stall_from_ex();
                    };
                    let Some(rs2_lit) = self.ex_bypass_get_rs2(rs2, rs2_lit) else {
                        return raw_stall_from_ex();
                    };
                    let mut tmp_result: [u32; 4] = [0; 4];

                    for i in 0..4 {
                        tmp_result[i] = rs1_lit[i] & rs2_lit[i];
                    }

                    (
                        EX_AGU_rf {
                            valid: true,
                            arith_result: Some(tmp_result),
                            agu_op: idex_rf.get_agu_op(),
                            dma_op: idex_rf.get_dma_op(),
                            wb_op: idex_rf.get_wb_op(),
                        },
                        signal_req::new(signal_reason::no_reason, CPU_stages::EX, None),
                        [arch_action::DoNothing].to_vec(),
                    )
                }
                ALUop::MUL {
                    rs1,
                    rs2,
                    rs1_lit,
                    rs2_lit,
                } => {
                    let Some(rs1_lit) = self.ex_bypass_get_rs1(rs1, rs1_lit) else {
                        return raw_stall_from_ex();
                    };
                    let Some(rs2_lit) = self.ex_bypass_get_rs2(rs2, rs2_lit) else {
                        return raw_stall_from_ex();
                    };
                    let mut tmp_result: [u32; 4] = [0; 4];

                    for i in 0..4 {
                        tmp_result[i] = rs1_lit[i] * rs2_lit[i];
                    }

                    (
                        EX_AGU_rf {
                            valid: true,
                            arith_result: Some(tmp_result),
                            agu_op: idex_rf.get_agu_op(),
                            dma_op: idex_rf.get_dma_op(),
                            wb_op: idex_rf.get_wb_op(),
                        },
                        signal_req::new(signal_reason::no_reason, CPU_stages::EX, None),
                        [arch_action::DoNothing].to_vec(),
                    )
                }
                ALUop::TEST {
                    rs1,
                    rs2,
                    rs1_lit,
                    rs2_lit,
                } => {
                    let Some(rs1_lit) = self.ex_bypass_get_rs1(rs1, rs1_lit) else {
                        return raw_stall_from_ex();
                    };
                    let Some(rs2_lit) = self.ex_bypass_get_rs2(rs2, rs2_lit) else {
                        return raw_stall_from_ex();
                    };
                    let mut equal: bool = true;

                    for i in 0..4 {
                        equal = equal & (rs1_lit[i] == rs2_lit[i]);
                    }

                    if !equal {
                        (
                            EX_AGU_rf {
                                valid: true,
                                arith_result: None,
                                agu_op: AGUop::NOP,
                                dma_op: DMAop::NOP,
                                wb_op: WBop::NOP,
                            },
                            signal_req::new(
                                signal_reason::no_reason,
                                CPU_stages::EX,
                                Some(HashSet::<CPU_stages>::from([
                                    CPU_stages::IF,
                                    CPU_stages::ID,
                                ])),
                            ),
                            [arch_action::HoldPC].to_vec(),
                        )
                    } else {
                        (
                            EX_AGU_rf {
                                valid: true,
                                arith_result: None,
                                agu_op: AGUop::NOP,
                                dma_op: DMAop::NOP,
                                wb_op: WBop::NOP,
                            },
                            signal_req::new(
                                signal_reason::prog_end,
                                CPU_stages::EX,
                                Some(HashSet::<CPU_stages>::from([
                                    CPU_stages::IF,
                                    CPU_stages::ID,
                                ])),
                            ),
                            [arch_action::DoNothing].to_vec(),
                        )
                    }
                }
            }
        }
    }
}

#[derive(Clone, Copy)]
enum EX_stop_FSM_states {
    Drain_WB,  //drain on-flying WB
    Drain_AGU, //drain on-flying AGU
    Drain_MEM, //drain on-flying MEM
    IDLE,
}

#[derive(Clone, Copy)]
pub struct EX_stop_FSM {
    state: EX_stop_FSM_states,
    state_next: EX_stop_FSM_states,
}

impl SigFSM for EX_stop_FSM {
    fn reason(&self) -> signal_reason {
        signal_reason::prog_end
    }

    //action should return Normal when reaching the finish state
    fn action(&self) -> pipeline_action {
        match self.state {
            EX_stop_FSM_states::Drain_WB => pipeline_action::Flush,
            EX_stop_FSM_states::Drain_MEM => pipeline_action::Flush,
            EX_stop_FSM_states::Drain_AGU => pipeline_action::Flush,
            EX_stop_FSM_states::IDLE => pipeline_action::Normal,
        }
    }

    fn get_ops(&self) -> HashMap<CPU_stages, pipeline_action> {
        HashMap::<CPU_stages, pipeline_action>::from([
            (CPU_stages::IF, pipeline_action::Flush), //flush ifid
            (CPU_stages::ID, pipeline_action::Flush), //flush idex
        ])
    }

    fn advance_winner(&mut self) -> bool {
        self.state_next = match self.state {
            EX_stop_FSM_states::Drain_WB => EX_stop_FSM_states::Drain_MEM,
            EX_stop_FSM_states::Drain_MEM => EX_stop_FSM_states::Drain_AGU,
            EX_stop_FSM_states::Drain_AGU => EX_stop_FSM_states::IDLE,
            EX_stop_FSM_states::IDLE => EX_stop_FSM_states::IDLE,
        };

        self.state = self.state_next;
        return true;
    }

    fn handle_blocked(&mut self) {}
}

impl EX_stop_FSM {
    pub const fn new() -> Self {
        Self {
            state: EX_stop_FSM_states::Drain_WB,
            state_next: EX_stop_FSM_states::Drain_MEM,
        }
    }
}

#[derive(Clone, Copy)]
enum RAW_resolution_FSM_state {
    PushdownAGU,
    Idle,
}

#[derive(Clone, Copy)]
pub struct RAW_resolution_FSM {
    state: RAW_resolution_FSM_state,
}

impl RAW_resolution_FSM {
    pub const fn new() -> Self {
        Self {
            state: RAW_resolution_FSM_state::PushdownAGU,
        }
    }
}

impl SigFSM for RAW_resolution_FSM {
    fn reason(&self) -> signal_reason {
        signal_reason::RAW_resolution
    }

    fn action(&self) -> pipeline_action {
        match self.state {
            RAW_resolution_FSM_state::PushdownAGU => pipeline_action::Stall,
            RAW_resolution_FSM_state::Idle => pipeline_action::Normal,
        }
    }

    fn get_ops(&self) -> HashMap<CPU_stages, pipeline_action> {
        HashMap::<CPU_stages, pipeline_action>::from([
            (CPU_stages::IF, pipeline_action::Stall),
            (CPU_stages::ID, pipeline_action::Stall),
            (CPU_stages::EX, pipeline_action::Stall),
            // (CPU_stages::AGU, pipeline_action::Stall),
        ])
    }

    fn advance_winner(&mut self) -> bool {
        self.state = RAW_resolution_FSM_state::Idle;
        true
    }

    fn handle_blocked(&mut self) {
        self.state = RAW_resolution_FSM_state::Idle;
    }
}
