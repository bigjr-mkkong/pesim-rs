use crate::cpu::ID::ID_EX_rf;
use crate::cpu::pimcpu_types::{AGUop, ALUop, CPU_stages, DMAop, WBop, arch_action, fatptr_rf};
use crate::cpu::pipeline::CPU;
use crate::cpu::signal_scoreboard::{SigFSM, pipeline_action, signal_reason, signal_req};
use std::collections::{HashMap, HashSet};

pub struct EX_MEM_rf {
    valid: bool,
    phys_addr: Option<u32>,
    arith_result: Option<[u32; 4]>,
    ptr_result: Option<fatptr_rf>,
    dma_op: DMAop,
    wb_op: WBop,
}

impl EX_MEM_rf {
    pub const fn new() -> Self {
        Self {
            valid: false,
            phys_addr: None,
            arith_result: None,
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

    pub fn get_arith_result(&self) -> Option<[u32; 4]> {
        self.arith_result
    }

    pub fn get_ptr_result(&self) -> Option<fatptr_rf> {
        self.ptr_result
    }

    pub fn get_dma_op(&self) -> DMAop {
        self.dma_op
    }

    pub fn get_wb_op(&self) -> WBop {
        self.wb_op
    }
}

impl CPU {
    pub fn eval_EX(&self, idex_rf: &ID_EX_rf) -> (EX_MEM_rf, signal_req, Vec<arch_action>) {
        let raw_stall_from_ex = || {
            (
                EX_MEM_rf::new(),
                signal_req::new(
                    signal_reason::RAW_resolution,
                    CPU_stages::EX,
                    Some(HashSet::from([
                        CPU_stages::IF,
                        CPU_stages::ID,
                        CPU_stages::EX,
                    ])),
                ),
                vec![arch_action::DoNothing],
            )
        };
        let exception_from_ex = || {
            (
                EX_MEM_rf::new(),
                signal_req::new(
                    signal_reason::exception,
                    CPU_stages::EX,
                    Some(HashSet::from([
                        CPU_stages::IF,
                        CPU_stages::ID,
                        CPU_stages::EX,
                    ])),
                ),
                vec![arch_action::HoldPC],
            )
        };

        if !idex_rf.is_valid() {
            return (
                EX_MEM_rf::new(),
                signal_req::new(signal_reason::no_reason, CPU_stages::EX, None),
                vec![arch_action::DoNothing],
            );
        }

        let arith_result = match idex_rf.get_alu_op() {
            ALUop::NOP => None,
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
                Some(std::array::from_fn(|idx| rs1_lit[idx] + rs2_lit[idx]))
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
                Some(std::array::from_fn(|idx| rs1_lit[idx] - rs2_lit[idx]))
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
                Some(std::array::from_fn(|idx| rs1_lit[idx] & rs2_lit[idx]))
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
                Some(std::array::from_fn(|idx| rs1_lit[idx] * rs2_lit[idx]))
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
                let equal = rs1_lit
                    .iter()
                    .zip(rs2_lit.iter())
                    .all(|(lhs, rhs)| lhs == rhs);

                return if equal {
                    (
                        EX_MEM_rf {
                            valid: true,
                            ..EX_MEM_rf::new()
                        },
                        signal_req::new(
                            signal_reason::prog_end,
                            CPU_stages::EX,
                            Some(HashSet::from([CPU_stages::IF, CPU_stages::ID])),
                        ),
                        vec![arch_action::DoNothing],
                    )
                } else {
                    (
                        EX_MEM_rf {
                            valid: true,
                            ..EX_MEM_rf::new()
                        },
                        signal_req::new(
                            signal_reason::no_reason,
                            CPU_stages::EX,
                            Some(HashSet::from([CPU_stages::IF, CPU_stages::ID])),
                        ),
                        vec![arch_action::HoldPC],
                    )
                };
            }
        };

        let dma_op = idex_rf.get_dma_op();
        let wb_op = idex_rf.get_wb_op();
        let (phys_addr, ptr_result, dma_op) = match idex_rf.get_agu_op() {
            AGUop::NOP => {
                let Some(dma_op) = self.agu_bypass_dma_op(dma_op) else {
                    return raw_stall_from_ex();
                };
                (None, None, dma_op)
            }
            AGUop::CHK { frs, fptr_lit } => {
                let Some(fptr_lit) = self.agu_bypass_get_frs(frs, fptr_lit) else {
                    return raw_stall_from_ex();
                };
                let Some(dma_op) = self.agu_bypass_dma_op(dma_op) else {
                    return raw_stall_from_ex();
                };
                if !self.agu.accept(fptr_lit) {
                    return exception_from_ex();
                }
                (self.agu.translate(fptr_lit), None, dma_op)
            }
            AGUop::ADD {
                frs,
                rs1,
                fptr_lit,
                rs1_lit,
                idx_imm,
            } => {
                let Some(fptr_lit) = self.agu_bypass_get_frs(frs, fptr_lit) else {
                    return raw_stall_from_ex();
                };
                let Some(rs1_lit) = self.agu_bypass_get_rs1(rs1, rs1_lit) else {
                    return raw_stall_from_ex();
                };
                let Some(dma_op) = self.agu_bypass_dma_op(dma_op) else {
                    return raw_stall_from_ex();
                };
                let Some(ptr_result) = self.agu.addition(fptr_lit, rs1_lit, idx_imm) else {
                    return exception_from_ex();
                };
                (None, Some(ptr_result), dma_op)
            }
            AGUop::SUB {
                frs,
                rs1,
                fptr_lit,
                rs1_lit,
                idx_imm,
            } => {
                let Some(fptr_lit) = self.agu_bypass_get_frs(frs, fptr_lit) else {
                    return raw_stall_from_ex();
                };
                let Some(rs1_lit) = self.agu_bypass_get_rs1(rs1, rs1_lit) else {
                    return raw_stall_from_ex();
                };
                let Some(dma_op) = self.agu_bypass_dma_op(dma_op) else {
                    return raw_stall_from_ex();
                };
                let Some(ptr_result) = self.agu.subtraction(fptr_lit, rs1_lit, idx_imm) else {
                    return exception_from_ex();
                };
                (None, Some(ptr_result), dma_op)
            }
        };

        (
            EX_MEM_rf {
                valid: true,
                phys_addr,
                arith_result,
                ptr_result,
                dma_op,
                wb_op,
            },
            signal_req::new(signal_reason::no_reason, CPU_stages::EX, None),
            vec![arch_action::DoNothing],
        )
    }
}

#[derive(Clone, Copy)]
enum EX_stop_FSM_states {
    Drain_WB,
    Drain_MEM,
    IDLE,
}

#[derive(Clone, Copy)]
pub struct EX_stop_FSM {
    state: EX_stop_FSM_states,
}

impl SigFSM for EX_stop_FSM {
    fn reason(&self) -> signal_reason {
        signal_reason::prog_end
    }

    fn action(&self) -> pipeline_action {
        match self.state {
            EX_stop_FSM_states::Drain_WB | EX_stop_FSM_states::Drain_MEM => pipeline_action::Flush,
            EX_stop_FSM_states::IDLE => pipeline_action::Normal,
        }
    }

    fn get_ops(&self) -> HashMap<CPU_stages, pipeline_action> {
        HashMap::from([
            (CPU_stages::IF, pipeline_action::Flush),
            (CPU_stages::ID, pipeline_action::Flush),
        ])
    }

    fn advance_winner(&mut self, _sig_reason: signal_reason) -> bool {
        self.state = match self.state {
            EX_stop_FSM_states::Drain_WB => EX_stop_FSM_states::Drain_MEM,
            EX_stop_FSM_states::Drain_MEM => EX_stop_FSM_states::IDLE,
            EX_stop_FSM_states::IDLE => EX_stop_FSM_states::IDLE,
        };
        true
    }
}

impl EX_stop_FSM {
    pub const fn new() -> Self {
        Self {
            state: EX_stop_FSM_states::Drain_WB,
        }
    }
}

#[derive(Clone, Copy)]
enum ExceptionDrainState {
    Drain_WB,
    Drain_MEM,
    Idle,
}

#[derive(Clone, Copy)]
pub struct ExceptionDrain_FSM {
    state: ExceptionDrainState,
}

impl SigFSM for ExceptionDrain_FSM {
    fn reason(&self) -> signal_reason {
        signal_reason::exception
    }

    fn action(&self) -> pipeline_action {
        match self.state {
            ExceptionDrainState::Drain_WB | ExceptionDrainState::Drain_MEM => {
                pipeline_action::Flush
            }
            ExceptionDrainState::Idle => pipeline_action::Normal,
        }
    }

    fn get_ops(&self) -> HashMap<CPU_stages, pipeline_action> {
        HashMap::from([
            (CPU_stages::IF, pipeline_action::Flush),
            (CPU_stages::ID, pipeline_action::Flush),
            (CPU_stages::EX, pipeline_action::Flush),
        ])
    }

    fn advance_winner(&mut self, _sig_reason: signal_reason) -> bool {
        self.state = match self.state {
            ExceptionDrainState::Drain_WB => ExceptionDrainState::Drain_MEM,
            ExceptionDrainState::Drain_MEM => ExceptionDrainState::Idle,
            ExceptionDrainState::Idle => ExceptionDrainState::Idle,
        };
        true
    }
}

impl ExceptionDrain_FSM {
    pub const fn new() -> Self {
        Self {
            state: ExceptionDrainState::Drain_WB,
        }
    }
}

#[derive(Clone, Copy)]
enum RAW_resolution_FSM_state {
    InsertBubble,
    Idle,
}

#[derive(Clone, Copy)]
pub struct RAW_resolution_FSM {
    state: RAW_resolution_FSM_state,
}

impl RAW_resolution_FSM {
    pub const fn new() -> Self {
        Self {
            state: RAW_resolution_FSM_state::InsertBubble,
        }
    }
}

impl SigFSM for RAW_resolution_FSM {
    fn reason(&self) -> signal_reason {
        signal_reason::RAW_resolution
    }

    fn action(&self) -> pipeline_action {
        match self.state {
            RAW_resolution_FSM_state::InsertBubble => pipeline_action::Stall,
            RAW_resolution_FSM_state::Idle => pipeline_action::Normal,
        }
    }

    fn get_ops(&self) -> HashMap<CPU_stages, pipeline_action> {
        HashMap::from([
            (CPU_stages::IF, pipeline_action::Stall),
            (CPU_stages::ID, pipeline_action::Stall),
            (CPU_stages::EX, pipeline_action::Stall),
            (CPU_stages::WB, pipeline_action::Stall),
        ])
    }

    fn advance_winner(&mut self, _sig_reason: signal_reason) -> bool {
        self.state = RAW_resolution_FSM_state::Idle;
        true
    }

    fn handle_blocked(&mut self) {
        self.state = RAW_resolution_FSM_state::Idle;
    }
}
