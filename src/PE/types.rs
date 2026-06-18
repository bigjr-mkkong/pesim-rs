// LD128, vRD, addr
// ST128, vRS, addr
// LD32, sRD, addr
// ST32, sRS, addr
// ADD128 vRD, vRS0, vRS1
// SUB128 vRD, vRS0, vRS1
// MUL128 vRD, vRS0, vRS1
// MAC128 sRD, sRS0, vRS0, vRS1
// ReLU128 vRD, vRS0

#[derive(Clone, Copy)]
pub enum inst {
    LD128 {
        vRD: u8,
        addr: u32,
    },
    ST128 {
        vRS: u8,
        addr: u32,
    },
    LD32 {
        sRD: u8,
        addr: u32,
    },
    ST32 {
        sRS: u8,
        addr: u32,
    },
    ADD128 {
        vRD: u8,
        vRS0: u8,
        vRS1: u8,
    },
    SUB128 {
        vRD: u8,
        vRS0: u8,
        vRS1: u8,
    },
    MUL128 {
        vRD: u8,
        vRS0: u8,
        vRS1: u8,
    },
    MAC128 {
        sRD: u8,
        sRS0: u8,
        vRS0: u8,
        vRS1: u8,
    },
    ReLU {
        vRD: u8,
        vRS0: u8,
    },
    NOP,
}

#[derive(Clone, Copy)]
pub enum ALUop {
    ADD {
        vRS0: u8,
        vRS1: u8,
        vRS0_lit: [i16; 8],
        vRS1_lit: [i16; 8],
    },
    SUB {
        vRS0: u8,
        vRS1: u8,
        vRS0_lit: [i16; 8],
        vRS1_lit: [i16; 8],
    },
    MUL {
        vRS0: u8,
        vRS1: u8,
        vRS0_lit: [i16; 8],
        vRS1_lit: [i16; 8],
    },
    MAC {
        sRS0: u8,
        vRS0: u8,
        vRS1: u8,
        sRS0_lit: i32,
        vRS0_lit: [i16; 8],
        vRS1_lit: [i16; 8],
    },
    ReLU {
        vRS0: u8,
        vRS0_lit: [i16; 8],
    },
    NOP,
}

#[derive(Clone, Copy)]
pub enum MEMop {
    NOP,
    ReadV { addr: u32 },
    WriteV { addr: u32, vRS: u8, data: [i16; 8] },
    ReadS { addr: u32 },
    WriteS { addr: u32, sRS: u8, data: i32 },
}

#[derive(Clone, Copy)]
pub enum WBop {
    NOP,
    VWrite { vRD: u8 },
    SWrite { sRD: u8 },
}

#[derive(Clone, Copy, Hash)]
pub enum arch_action {
    DoNothing,
    WriteVRF { vRD: u8, content: [i16; 8] },
    WriteSRF { sRD: u8, content: i32 },
    WriteMEM_V { addr: u32, content: [i16; 8] },
    WriteMEM_S { addr: u32, content: i32 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum arch_dest {
    Vec_RF(u8),
    Scalar_RF(u8),
    Vec_MEM(u32),
    Scalar_MEM(u32),
}

impl arch_action {
    pub fn dest(&self) -> Option<arch_dest> {
        match self {
            arch_action::DoNothing => None,
            arch_action::WriteVRF { vRD, .. } => Some(arch_dest::Vec_RF(*vRD)),
            arch_action::WriteSRF { sRD, .. } => Some(arch_dest::Scalar_RF(*sRD)),
            arch_action::WriteMEM_V { addr, .. } => Some(arch_dest::Vec_MEM(*addr)),
            arch_action::WriteMEM_S { addr, .. } => Some(arch_dest::Scalar_MEM(*addr)),
        }
    }
}

#[derive(Clone, Copy, Hash, PartialEq, Eq)]
pub enum PE_stages {
    ISSUE,
    EX,
}
