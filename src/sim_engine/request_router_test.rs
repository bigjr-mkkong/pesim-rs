use crate::PE::types::inst;
use crate::sim_engine::request_router::{decode_pe_inst, is_pe_request, routing_addr};

const PE_MARKER: u64 = 0xf000_0000_0000_0000;
const OPCODE_SHIFT: u32 = 56;
const REG_MASK: u64 = 0xf;
const ROUTING_ADDR_MASK: u64 = (1_u64 << 44) - 1;

const OP_NOP: u8 = 0;
const OP_ADD128: u8 = 1;
const OP_SUB128: u8 = 2;

pub(crate) fn encode_pe_inst(instruction: inst, route_addr: u64) -> u64 {
    let (opcode, rd, rs0, rs1) = match instruction {
        inst::NOP => (OP_NOP, 0, 0, 0),
        inst::ADD128 { vRD, vRS0, vRS1 } => (OP_ADD128, vRD, vRS0, vRS1),
        inst::SUB128 { vRD, vRS0, vRS1 } => (OP_SUB128, vRD, vRS0, vRS1),
        _ => panic!("temporary PE address encoder only supports NOP, ADD128, and SUB128"),
    };

    PE_MARKER
        | ((opcode as u64) << OPCODE_SHIFT)
        | ((rd as u64 & REG_MASK) << 52)
        | ((rs0 as u64 & REG_MASK) << 48)
        | ((rs1 as u64 & REG_MASK) << 44)
        | (route_addr & ROUTING_ADDR_MASK)
}

#[test]
fn temporary_mapping_round_trips_supported_instructions() {
    let addr = encode_pe_inst(
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
        decode_pe_inst(addr),
        Ok(inst::ADD128 {
            vRD: 3,
            vRS0: 1,
            vRS1: 2
        })
    ));
}
