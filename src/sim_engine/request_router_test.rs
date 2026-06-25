use crate::PE::types::inst;
use crate::sim_engine::request_router::{
    PIM_CMD_PAGE_BASE, PIM_CMD_SLOT_SIZE, decode_pim_cmd, is_pim_cmd_request, pim_cmd, routing_addr,
};

const REG_A_SHIFT: u32 = 0;
const REG_B_SHIFT: u32 = 4;
const REG_C_SHIFT: u32 = 8;
const REG_D_SHIFT: u32 = 12;
const MEM_ADDR_SHIFT: u32 = 16;
const REG_MASK: u64 = 0xf;

const OP_NOP: u64 = 0;
const OP_ADD128: u64 = 1;
const OP_SUB128: u64 = 2;
const OP_MUL128: u64 = 3;
const OP_MAC128: u64 = 4;
const OP_RELU: u64 = 5;
const OP_LD128: u64 = 6;
const OP_ST128: u64 = 7;
const OP_LD32: u64 = 8;
const OP_ST32: u64 = 9;
const OP_CGO_START: u64 = 10;
const OP_CGO_QUERY: u64 = 11;

pub(crate) fn encode_pim_cmd(command: pim_cmd) -> (u64, [u64; 8]) {
    match command {
        pim_cmd::Fgo(instruction) => encode_fgo_cmd(instruction),
        pim_cmd::CgoStart => encode_cgo_cmd(OP_CGO_START),
        pim_cmd::CgoQuery => encode_cgo_cmd(OP_CGO_QUERY),
    }
}

pub(crate) fn encode_fgo_cmd(instruction: inst) -> (u64, [u64; 8]) {
    let (opcode, payload0) = match instruction {
        inst::NOP => (OP_NOP, 0),
        inst::ADD128 { vRD, vRS0, vRS1 } => (OP_ADD128, pack_regs(vRD, vRS0, vRS1, 0)),
        inst::SUB128 { vRD, vRS0, vRS1 } => (OP_SUB128, pack_regs(vRD, vRS0, vRS1, 0)),
        inst::MUL128 { vRD, vRS0, vRS1 } => (OP_MUL128, pack_regs(vRD, vRS0, vRS1, 0)),
        inst::MAC128 {
            sRD,
            sRS0,
            vRS0,
            vRS1,
        } => (OP_MAC128, pack_regs(sRD, vRS0, vRS1, sRS0)),
        inst::ReLU { vRD, vRS0 } => (OP_RELU, pack_regs(vRD, vRS0, 0, 0)),
        inst::LD128 { vRD, addr } => (OP_LD128, pack_mem(vRD, addr)),
        inst::ST128 { vRS, addr } => (OP_ST128, pack_mem(vRS, addr)),
        inst::LD32 { sRD, addr } => (OP_LD32, pack_mem(sRD, addr)),
        inst::ST32 { sRS, addr } => (OP_ST32, pack_mem(sRS, addr)),
    };

    let mut payload = [0; 8];
    payload[0] = payload0;
    (PIM_CMD_PAGE_BASE + PIM_CMD_SLOT_SIZE * opcode, payload)
}

pub(crate) fn encode_cgo_cmd(opcode: u64) -> (u64, [u64; 8]) {
    (PIM_CMD_PAGE_BASE + PIM_CMD_SLOT_SIZE * opcode, [0; 8])
}

fn pack_regs(reg_a: u8, reg_b: u8, reg_c: u8, reg_d: u8) -> u64 {
    (((reg_a as u64) & REG_MASK) << REG_A_SHIFT)
        | (((reg_b as u64) & REG_MASK) << REG_B_SHIFT)
        | (((reg_c as u64) & REG_MASK) << REG_C_SHIFT)
        | (((reg_d as u64) & REG_MASK) << REG_D_SHIFT)
}

fn pack_mem(reg_a: u8, mem_addr: u32) -> u64 {
    (((reg_a as u64) & REG_MASK) << REG_A_SHIFT) | ((mem_addr as u64) << MEM_ADDR_SHIFT)
}

#[test]
fn pe_request_uses_fixed_instruction_page() {
    assert!(!is_pim_cmd_request(PIM_CMD_PAGE_BASE - 1));
    assert!(is_pim_cmd_request(PIM_CMD_PAGE_BASE));
    assert!(is_pim_cmd_request(
        PIM_CMD_PAGE_BASE + PIM_CMD_SLOT_SIZE * OP_ST32
    ));
    assert!(is_pim_cmd_request(PIM_CMD_PAGE_BASE + 0xfff));
    assert!(!is_pim_cmd_request(PIM_CMD_PAGE_BASE + 0x1000));
    assert_eq!(routing_addr(PIM_CMD_PAGE_BASE), PIM_CMD_PAGE_BASE);
}

#[test]
fn rejects_unaligned_and_unknown_slots() {
    let payload = [0; 8];
    assert!(decode_pim_cmd(PIM_CMD_PAGE_BASE + 1, &payload).is_err());
    assert!(decode_pim_cmd(PIM_CMD_PAGE_BASE + PIM_CMD_SLOT_SIZE * 12, &payload).is_err());
}

#[test]
fn fixed_slots_round_trip_supported_instructions() {
    let cases = [
        inst::NOP,
        inst::ADD128 {
            vRD: 3,
            vRS0: 1,
            vRS1: 2,
        },
        inst::SUB128 {
            vRD: 4,
            vRS0: 5,
            vRS1: 6,
        },
        inst::MUL128 {
            vRD: 7,
            vRS0: 8,
            vRS1: 9,
        },
        inst::MAC128 {
            sRD: 2,
            sRS0: 3,
            vRS0: 4,
            vRS1: 5,
        },
        inst::ReLU { vRD: 6, vRS0: 7 },
        inst::LD128 {
            vRD: 8,
            addr: 0x1234,
        },
        inst::ST128 {
            vRS: 9,
            addr: 0x5678,
        },
        inst::LD32 {
            sRD: 1,
            addr: 0xabcd,
        },
        inst::ST32 {
            sRS: 2,
            addr: 0xef01,
        },
    ];

    for instruction in cases {
        let (addr, payload) = encode_fgo_cmd(instruction);
        let pim_cmd::Fgo(decoded) = decode_pim_cmd(addr, &payload).unwrap() else {
            panic!("FGO slot decoded as a non-FGO command");
        };
        assert_same_inst(decoded, instruction);
    }
}

#[test]
fn fixed_slots_decode_cgo_commands() {
    let (start_addr, start_payload) = encode_pim_cmd(pim_cmd::CgoStart);
    assert!(matches!(
        decode_pim_cmd(start_addr, &start_payload),
        Ok(pim_cmd::CgoStart)
    ));

    let (query_addr, query_payload) = encode_pim_cmd(pim_cmd::CgoQuery);
    assert!(matches!(
        decode_pim_cmd(query_addr, &query_payload),
        Ok(pim_cmd::CgoQuery)
    ));
}

fn assert_same_inst(actual: inst, expected: inst) {
    match (actual, expected) {
        (inst::NOP, inst::NOP) => {}
        (
            inst::ADD128 {
                vRD: avrd,
                vRS0: avrs0,
                vRS1: avrs1,
            },
            inst::ADD128 {
                vRD: evrd,
                vRS0: evrs0,
                vRS1: evrs1,
            },
        )
        | (
            inst::SUB128 {
                vRD: avrd,
                vRS0: avrs0,
                vRS1: avrs1,
            },
            inst::SUB128 {
                vRD: evrd,
                vRS0: evrs0,
                vRS1: evrs1,
            },
        )
        | (
            inst::MUL128 {
                vRD: avrd,
                vRS0: avrs0,
                vRS1: avrs1,
            },
            inst::MUL128 {
                vRD: evrd,
                vRS0: evrs0,
                vRS1: evrs1,
            },
        ) => {
            assert_eq!((avrd, avrs0, avrs1), (evrd, evrs0, evrs1));
        }
        (
            inst::MAC128 {
                sRD: asrd,
                sRS0: asrs0,
                vRS0: avrs0,
                vRS1: avrs1,
            },
            inst::MAC128 {
                sRD: esrd,
                sRS0: esrs0,
                vRS0: evrs0,
                vRS1: evrs1,
            },
        ) => {
            assert_eq!((asrd, asrs0, avrs0, avrs1), (esrd, esrs0, evrs0, evrs1));
        }
        (
            inst::ReLU {
                vRD: avrd,
                vRS0: avrs0,
            },
            inst::ReLU {
                vRD: evrd,
                vRS0: evrs0,
            },
        ) => {
            assert_eq!((avrd, avrs0), (evrd, evrs0));
        }
        (
            inst::LD128 {
                vRD: avrd,
                addr: aaddr,
            },
            inst::LD128 {
                vRD: evrd,
                addr: eaddr,
            },
        ) => {
            assert_eq!((avrd, aaddr), (evrd, eaddr));
        }
        (
            inst::ST128 {
                vRS: avrs,
                addr: aaddr,
            },
            inst::ST128 {
                vRS: evrs,
                addr: eaddr,
            },
        ) => {
            assert_eq!((avrs, aaddr), (evrs, eaddr));
        }
        (
            inst::LD32 {
                sRD: asrd,
                addr: aaddr,
            },
            inst::LD32 {
                sRD: esrd,
                addr: eaddr,
            },
        ) => {
            assert_eq!((asrd, aaddr), (esrd, eaddr));
        }
        (
            inst::ST32 {
                sRS: asrs,
                addr: aaddr,
            },
            inst::ST32 {
                sRS: esrs,
                addr: eaddr,
            },
        ) => {
            assert_eq!((asrs, aaddr), (esrs, eaddr));
        }
        _ => panic!("decoded instruction did not match expected instruction"),
    }
}
