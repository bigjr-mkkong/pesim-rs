use crate::memory::AGU_unit::{BOUND_BITS, IDX_BITS};
#[derive(Copy, Clone)]
pub struct fatptr_rf {
    tag: u8,
    offset: u32,
}

impl fatptr_rf {
    pub const fn new(tag_: u8, offset_: u32) -> Self{
        Self{
            tag: tag_,
            offset: offset_
        }
    }

    pub fn get_idx(&self) -> u8 {
        crate::check_bound!(self.tag, IDX_BITS)
    }

    pub fn get_offset(&self) -> u32 {
        crate::check_bound!(self.offset, BOUND_BITS)
    }
}

#[derive(Copy, Clone)]
pub enum inst {
    NOP,
    ADD128 {
        rd: u8,
        rs1: u8,
        rs2: u8,
    },
    SUB128 {
        rd: u8,
        rs1: u8,
        rs2: u8,
    },
    MUL128 {
        rd: u8,
        rs1: u8,
        rs2: u8,
    },
    AND128 {
        rd: u8,
        rs1: u8,
        rs2: u8,
    },
    LD128 {
        rd: u8,
        frs: u8,
    },
    ST128 {
        rs: u8,
        frd: u8,
    },
    FatPtrLD {
        frd: u8,
        frs: u8,
    },
    FatPtrST {
        frd: u8,
        frs: u8,
    },
    FatPtrADD {
        frd: u8,
        frs: u8,
        rs1: u8,
        imm_idx: u8,
    },
    FatPtrSUB {
        frd: u8,
        frs: u8,
        rs1: u8,
        imm_idx: u8,
    },
    JUMP {
        inst_imm: u16,
    },
    EqualExit {
        rd: u8,
        rs1: u8,
    },
}

#[derive(Copy, Clone, Hash, Eq, PartialEq)]
pub enum CPU_stages {
    IF,
    ID,
    EX,
    AGU,
    MEM,
    WB,
}

impl CPU_stages {
    fn get_rank(&self) -> u8 {
        match self {
            CPU_stages::IF => 1,
            CPU_stages::ID => 2,
            CPU_stages::EX => 3,
            CPU_stages::AGU => 4,
            CPU_stages::MEM => 5,
            CPU_stages::WB => 6,
        }
    }
}

impl PartialOrd for CPU_stages {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for CPU_stages {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.get_rank().cmp(&&other.get_rank())
    }
}

#[derive(Clone, Copy)]
pub enum ALUop {
    NOP,
    ADD {
        rs1_lit: [u32; 4],
        rs2_lit: [u32; 4],
    },
    SUB {
        rs1_lit: [u32; 4],
        rs2_lit: [u32; 4],
    },
    AND {
        rs1_lit: [u32; 4],
        rs2_lit: [u32; 4],
    },
    MUL {
        rs1_lit: [u32; 4],
        rs2_lit: [u32; 4],
    },
    TEST {
        rs1_lit: [u32; 4],
        rs2_lit: [u32; 4],
    },
}

#[derive(Clone, Copy)]
pub enum AGUop {
    NOP,
    CHK {
        fptr_lit: fatptr_rf,
    },
    ADD {
        fptr_lit: fatptr_rf,
        rs1_lit: [u32; 4],
        idx_imm: u8,
    },
    SUB {
        fptr_lit: fatptr_rf,
        rs1_lit: [u32; 4],
        idx_imm: u8,
    },
}

#[derive(Clone, Copy)]
pub enum DMAop {
    NOP,
    READ_VEC,
    WRITE_VEC { data_lit: [u32; 4] },
    READ_FPTR,
    WRITE_FPTR { fptr_data_lit: fatptr_rf },
}

#[derive(Clone, Copy)]
pub enum WBop {
    NOP,
    WB_VEC { rd: u8 },
    WB_FPTR { frd: u8 },
}

/*
 * TODO
 * HoldPC will not change current PC, some previous DoNothing arch_action should be it
 */
#[derive(Clone, Copy)]
pub enum arch_action {
    DoNothing,
    WritePC { new_pc: u16 },
    HoldPC,
    WriteVRF { rd: u16, content: [u32; 4] },
    WriteFPTR { frd: u16, content: fatptr_rf },
    WriteMEM_DATA { addr: u32, content: [u32; 4] },
    WriteMEM_FPTR { addr: u32, content: fatptr_rf }
}
