// LD128, vRD, addr
// ST128, vRS, addr
// LD32, sRD, addr
// ST32, sRS, addr
// ADD128 vRD, vRS0, vRS1
// SUB128 vRD, vRS0, vRS1
// MUL128 vRD, vRS0, vRS1
// MAC128 sRD, sRS0, vRS0, vRS1
// ReLU128 vRD, vRS0

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
        vRS0_lit: [i16; 8],
        vRS1_lit: [i16; 8],
    },
    SUB {
        vRS0_lit: [i16; 8],
        vRS1_lit: [i16; 8],
    },
    MUL {
        vRS0_lit: [i16; 8],
        vRS1_lit: [i16; 8],
    },
    MAC {
        sRS0_lit: i32,
        vRS0_lit: [i16; 8],
        vRS1_lit: [i16; 8],
    },
    ReLU {
        vRS0_lit: [i16; 8],
    },
    NOP,
}

#[derive(Clone, Copy)]
pub enum MEMop {
    NOP,
    ReadV { addr: u32 },
    WriteV { addr: u32, data: [i16; 8] },
    ReadS { addr: u32 },
    WriteS { addr: u32, data: i32 },
}

#[derive(Clone, Copy)]
pub enum WBop {
    NOP,
    VWrite { vRD: u8 },
    SWrite { sRD: u8 },
}
