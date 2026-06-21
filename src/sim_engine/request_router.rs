use crate::PE::types::inst;
use crate::memory::mem_portal::cacheline_payload;

// Temporary host-address encoding. Keep all mapping details in this module so
// the final address-to-instruction format can replace it without touching the
// simulator or engine scheduling code.
const PE_MARKER_MASK: u64 = 0xf000_0000_0000_0000;
const PE_MARKER: u64 = 0xf000_0000_0000_0000;
const OPCODE_SHIFT: u32 = 0;
const RD_SHIFT: u32 = 4;
const RS0_SHIFT: u32 = 8;
const RS1_SHIFT: u32 = 12;
const REG_MASK: u64 = 0xf;
const ROUTING_ADDR_MASK: u64 = !PE_MARKER_MASK;

const OP_NOP: u8 = 0;
const OP_ADD128: u8 = 1;
const OP_SUB128: u8 = 2;

pub fn is_pe_request(addr: u64) -> bool {
    addr & PE_MARKER_MASK == PE_MARKER
}

pub fn routing_addr(addr: u64) -> u64 {
    if is_pe_request(addr) {
        addr & ROUTING_ADDR_MASK
    } else {
        addr
    }
}

pub fn decode_pe_inst(addr: u64, payload: &cacheline_payload) -> Result<inst, &'static str> {
    if !is_pe_request(addr) {
        return Err("address is not encoded as a PE instruction");
    }

    // Temporary configurable mapping: address selects the PE request space and
    // target engine, while payload word 0 carries opcode/register fields.
    let encoded = payload[0];
    let opcode = ((encoded >> OPCODE_SHIFT) & 0xf) as u8;
    let rd = ((encoded >> RD_SHIFT) & REG_MASK) as u8;
    let rs0 = ((encoded >> RS0_SHIFT) & REG_MASK) as u8;
    let rs1 = ((encoded >> RS1_SHIFT) & REG_MASK) as u8;
    match opcode {
        OP_NOP => Ok(inst::NOP),
        OP_ADD128 => Ok(inst::ADD128 {
            vRD: rd,
            vRS0: rs0,
            vRS1: rs1,
        }),
        OP_SUB128 => Ok(inst::SUB128 {
            vRD: rd,
            vRS0: rs0,
            vRS1: rs1,
        }),
        _ => Err("unknown PE instruction opcode"),
    }
}
