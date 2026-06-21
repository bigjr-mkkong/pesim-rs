use crate::PE::types::inst;
use crate::sim_engine::request_router::{decode_pe_inst, is_pe_request, routing_addr};

const PE_MARKER: u64 = 0xf000_0000_0000_0000;
const OPCODE_SHIFT: u32 = 0;
const RD_SHIFT: u32 = 4;
const RS0_SHIFT: u32 = 8;
const RS1_SHIFT: u32 = 12;
const REG_MASK: u64 = 0xf;
const ROUTING_ADDR_MASK: u64 = 0x0fff_ffff_ffff_ffff;

const OP_NOP: u8 = 0;
const OP_ADD128: u8 = 1;
const OP_SUB128: u8 = 2;

pub(crate) fn encode_pe_inst(instruction: inst, route_addr: u64) -> (u64, [u64; 8]) {
    let (opcode, rd, rs0, rs1) = match instruction {
        inst::NOP => (OP_NOP, 0, 0, 0),
        inst::ADD128 { vRD, vRS0, vRS1 } => (OP_ADD128, vRD, vRS0, vRS1),
        inst::SUB128 { vRD, vRS0, vRS1 } => (OP_SUB128, vRD, vRS0, vRS1),
        _ => panic!("temporary PE address encoder only supports NOP, ADD128, and SUB128"),
    };

    let addr = PE_MARKER | (route_addr & ROUTING_ADDR_MASK);
    let mut payload = [0; 8];
    payload[0] = ((opcode as u64) << OPCODE_SHIFT)
        | ((rd as u64 & REG_MASK) << RD_SHIFT)
        | ((rs0 as u64 & REG_MASK) << RS0_SHIFT)
        | ((rs1 as u64 & REG_MASK) << RS1_SHIFT);
    (addr, payload)
}

#[test]
fn temporary_mapping_round_trips_supported_instructions() {
    let (addr, payload) = encode_pe_inst(
        inst::ADD128 {
            vRD: 3,
            vRS0: 1,
            vRS1: 2,
        },
        0x1234,
    );

    assert!(is_pe_request(addr));
    assert_eq!(routing_addr(addr), 0x1234);
    assert!(matches!(
        decode_pe_inst(addr, &payload),
        Ok(inst::ADD128 {
            vRD: 3,
            vRS0: 1,
            vRS1: 2
        })
    ));
}

#[test]
fn payload_selects_instruction_for_the_same_address() {
    let (add_addr, add_payload) = encode_pe_inst(
        inst::ADD128 {
            vRD: 3,
            vRS0: 1,
            vRS1: 2,
        },
        0x400,
    );
    let (sub_addr, sub_payload) = encode_pe_inst(
        inst::SUB128 {
            vRD: 3,
            vRS0: 1,
            vRS1: 2,
        },
        0x400,
    );

    assert_eq!(add_addr, sub_addr);
    assert_ne!(add_payload, sub_payload);
    assert!(matches!(
        decode_pe_inst(add_addr, &add_payload),
        Ok(inst::ADD128 { .. })
    ));
    assert!(matches!(
        decode_pe_inst(sub_addr, &sub_payload),
        Ok(inst::SUB128 { .. })
    ));
}
