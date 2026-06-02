use crate::cpu::pimcpu_types::*;
use crate::cpu::pipeline::CPU;
use crate::cpu::signal_scoreboard::{SigFSM, pipeline_action, signal_reason, signal_req};

use crate::cpu::RF::arch_rf;

use crate::cpu::IF::IF_ID_rf;

use std::collections::{HashMap, HashSet};

pub struct ID_EX_rf {
    valid: bool,

    alu_op: ALUop,
    agu_op: AGUop,
    dma_op: DMAop,
    wb_op: WBop,
}

impl ID_EX_rf {
    pub const fn new() -> Self {
        Self {
            valid: false,

            alu_op: ALUop::NOP,
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

    pub fn get_alu_op(&self) -> ALUop {
        self.alu_op.clone()
    }

    pub fn get_agu_op(&self) -> AGUop {
        self.agu_op.clone()
    }

    pub fn get_dma_op(&self) -> DMAop {
        self.dma_op.clone()
    }

    pub fn get_wb_op(&self) -> WBop {
        self.wb_op.clone()
    }
}

impl CPU {
    pub fn eval_ID(
        &self,
        ifid_rf: &IF_ID_rf,
        arf: &arch_rf,
    ) -> (ID_EX_rf, signal_req, Vec<arch_action>) {
        if !ifid_rf.is_valid() {
            (
                ID_EX_rf {
                    valid: false,
                    alu_op: ALUop::NOP,
                    agu_op: AGUop::NOP,
                    dma_op: DMAop::NOP,
                    wb_op: WBop::NOP,
                },
                signal_req::new(signal_reason::no_reason, CPU_stages::ID, None),
                [arch_action::DoNothing].to_vec(),
            )
        } else {
            match ifid_rf.get_fetched_inst() {
                inst::NOP => (
                    ID_EX_rf {
                        valid: true,
                        alu_op: ALUop::NOP,
                        agu_op: AGUop::NOP,
                        dma_op: DMAop::NOP,
                        wb_op: WBop::NOP,
                    },
                    signal_req::new(signal_reason::no_reason, CPU_stages::ID, None),
                    [arch_action::DoNothing].to_vec(),
                ),
                inst::ADD128 { rd, rs1, rs2 } => (
                    ID_EX_rf {
                        valid: true,
                        alu_op: ALUop::ADD {
                            rs1,
                            rs2,
                            rs1_lit: arf.read_vregs(rs1),
                            rs2_lit: arf.read_vregs(rs2),
                        },

                        agu_op: AGUop::NOP,
                        dma_op: DMAop::NOP,
                        wb_op: WBop::WB_VEC { rd },
                    },
                    signal_req::new(signal_reason::no_reason, CPU_stages::ID, None),
                    [arch_action::DoNothing].to_vec(),
                ),
                inst::SUB128 { rd, rs1, rs2 } => (
                    ID_EX_rf {
                        valid: true,
                        alu_op: ALUop::SUB {
                            rs1,
                            rs2,
                            rs1_lit: arf.read_vregs(rs1),
                            rs2_lit: arf.read_vregs(rs2),
                        },

                        agu_op: AGUop::NOP,
                        dma_op: DMAop::NOP,
                        wb_op: WBop::WB_VEC { rd },
                    },
                    signal_req::new(signal_reason::no_reason, CPU_stages::ID, None),
                    [arch_action::DoNothing].to_vec(),
                ),
                inst::MUL128 { rd, rs1, rs2 } => (
                    ID_EX_rf {
                        valid: true,
                        alu_op: ALUop::MUL {
                            rs1,
                            rs2,
                            rs1_lit: arf.read_vregs(rs1),
                            rs2_lit: arf.read_vregs(rs2),
                        },

                        agu_op: AGUop::NOP,
                        dma_op: DMAop::NOP,
                        wb_op: WBop::WB_VEC { rd },
                    },
                    signal_req::new(signal_reason::no_reason, CPU_stages::ID, None),
                    [arch_action::DoNothing].to_vec(),
                ),
                inst::AND128 { rd, rs1, rs2 } => (
                    ID_EX_rf {
                        valid: true,
                        alu_op: ALUop::AND {
                            rs1,
                            rs2,
                            rs1_lit: arf.read_vregs(rs1),
                            rs2_lit: arf.read_vregs(rs2),
                        },
                        agu_op: AGUop::NOP,
                        dma_op: DMAop::NOP,
                        wb_op: WBop::WB_VEC { rd: rd },
                    },
                    signal_req::new(signal_reason::no_reason, CPU_stages::ID, None),
                    [arch_action::DoNothing].to_vec(),
                ),
                inst::LD128 { rd, frs } => (
                    ID_EX_rf {
                        valid: true,
                        alu_op: ALUop::NOP,
                        agu_op: AGUop::CHK {
                            frs,
                            fptr_lit: arf
                                .read_fregs(frs)
                                .expect("ID: Unable to load from invalid FPTR"),
                        },
                        dma_op: DMAop::READ_VEC { rd },
                        wb_op: WBop::WB_VEC { rd: rd },
                    },
                    signal_req::new(signal_reason::no_reason, CPU_stages::ID, None),
                    [arch_action::DoNothing].to_vec(),
                ),
                inst::ST128 { rs, frd } => (
                    ID_EX_rf {
                        valid: true,
                        alu_op: ALUop::NOP,
                        agu_op: AGUop::CHK {
                            frs: frd,
                            fptr_lit: arf
                                .read_fregs(frd)
                                .expect("ID: Unable to load from invalid FPTR"),
                        },

                        dma_op: DMAop::WRITE_VEC {
                            rs,
                            data_lit: arf.read_vregs(rs),
                        },
                        wb_op: WBop::NOP,
                    },
                    signal_req::new(signal_reason::no_reason, CPU_stages::ID, None),
                    [arch_action::DoNothing].to_vec(),
                ),
                inst::FatPtrLD { frd, frs } => (
                    ID_EX_rf {
                        valid: true,
                        alu_op: ALUop::NOP,
                        agu_op: AGUop::CHK {
                            frs,
                            fptr_lit: arf
                                .read_fregs(frs)
                                .expect("ID: Unable to load from invalid FPTR"),
                        },
                        dma_op: DMAop::READ_FPTR { frd },
                        wb_op: WBop::WB_FPTR { frd: frd },
                    },
                    signal_req::new(signal_reason::no_reason, CPU_stages::ID, None),
                    [arch_action::DoNothing].to_vec(),
                ),
                inst::FatPtrST { frd, frs } => (
                    ID_EX_rf {
                        valid: true,
                        alu_op: ALUop::NOP,
                        agu_op: AGUop::CHK {
                            frs: frd,
                            fptr_lit: arf
                                .read_fregs(frd)
                                .expect("ID: Unable to load from invalid FPTR"),
                        },
                        dma_op: DMAop::WRITE_FPTR {
                            frs,
                            fptr_data_lit: arf
                                .read_fregs(frs)
                                .expect("ID: Unable to load from invalid FPTR"),
                        },
                        wb_op: WBop::NOP,
                    },
                    signal_req::new(signal_reason::no_reason, CPU_stages::ID, None),
                    [arch_action::DoNothing].to_vec(),
                ),
                inst::FatPtrADD {
                    frd,
                    frs,
                    rs1,
                    imm_idx,
                } => (
                    ID_EX_rf {
                        valid: true,
                        alu_op: ALUop::NOP,
                        agu_op: AGUop::ADD {
                            frs,
                            rs1,
                            fptr_lit: arf
                                .read_fregs(frs)
                                .expect("ID: Unable to load from invalid FPTR"),
                            rs1_lit: arf.read_vregs(rs1),
                            idx_imm: imm_idx,
                        },
                        dma_op: DMAop::NOP,
                        wb_op: WBop::WB_FPTR { frd },
                    },
                    signal_req::new(signal_reason::no_reason, CPU_stages::ID, None),
                    [arch_action::DoNothing].to_vec(),
                ),
                inst::FatPtrSUB {
                    frd,
                    frs,
                    rs1,
                    imm_idx,
                } => (
                    ID_EX_rf {
                        valid: true,
                        alu_op: ALUop::NOP,
                        agu_op: AGUop::SUB {
                            frs,
                            rs1,
                            fptr_lit: arf
                                .read_fregs(frs)
                                .expect("ID: Unable to load from invalid FPTR"),
                            rs1_lit: arf.read_vregs(rs1),
                            idx_imm: imm_idx,
                        },
                        dma_op: DMAop::NOP,
                        wb_op: WBop::WB_FPTR { frd },
                    },
                    signal_req::new(signal_reason::no_reason, CPU_stages::ID, None),
                    [arch_action::DoNothing].to_vec(),
                ),
                inst::JUMP { inst_imm } => (
                    ID_EX_rf {
                        valid: false,
                        alu_op: ALUop::NOP,
                        agu_op: AGUop::NOP,
                        dma_op: DMAop::NOP,
                        wb_op: WBop::NOP,
                    },
                    signal_req::new(
                        signal_reason::jump_resolution,
                        CPU_stages::ID,
                        Some(HashSet::<CPU_stages>::from([CPU_stages::IF])),
                    ),
                    [arch_action::WritePC { new_pc: inst_imm }].to_vec(),
                ),
                inst::EqualExit { rd, rs1 } => (
                    ID_EX_rf {
                        valid: true,
                        alu_op: ALUop::TEST {
                            rs1,
                            rs2: rd,
                            rs1_lit: arf.read_vregs(rs1),
                            rs2_lit: arf.read_vregs(rd),
                        },
                        agu_op: AGUop::NOP,
                        dma_op: DMAop::NOP,
                        wb_op: WBop::NOP,
                    },
                    signal_req::new(signal_reason::no_reason, CPU_stages::ID, None),
                    [arch_action::DoNothing].to_vec(),
                ),
            }
        }
    }
}

#[derive(Clone, Copy)]
enum ID_jump_FSG_states {
    FLUSH,
    IDLE,
}

#[derive(Clone, Copy)]
pub struct ID_jump_FSM {
    //This FSM only send flush signal once
    state: ID_jump_FSG_states,
    state_next: ID_jump_FSG_states,
}

impl SigFSM for ID_jump_FSM {
    fn reason(&self) -> signal_reason {
        signal_reason::jump_resolution
    }

    //action should return Normal when reaching the finish state
    fn action(&self) -> pipeline_action {
        match self.state {
            ID_jump_FSG_states::FLUSH => pipeline_action::Flush,
            _ => pipeline_action::Normal,
        }
    }

    fn get_ops(&self) -> HashMap<CPU_stages, pipeline_action> {
        HashMap::<CPU_stages, pipeline_action>::from([(CPU_stages::IF, pipeline_action::Flush)])
    }

    fn advance_winner(&mut self) -> bool {
        self.state_next = match self.state {
            ID_jump_FSG_states::FLUSH => ID_jump_FSG_states::IDLE,
            ID_jump_FSG_states::IDLE => ID_jump_FSG_states::IDLE,
        };

        self.state = self.state_next;
        return true;
    }

    fn handle_blocked(&mut self) {}
}

impl ID_jump_FSM {
    pub const fn new() -> Self {
        Self {
            state: ID_jump_FSG_states::FLUSH,
            state_next: ID_jump_FSG_states::IDLE,
        }
    }
}
