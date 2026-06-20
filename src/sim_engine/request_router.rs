use crate::PE::types::inst;

// Temporary host-address encoding. Keep all mapping details in this module so
// the final address-to-instruction format can replace it without touching the
// simulator or engine scheduling code.
const PE_MARKER_MASK: u64 = 0xf000_0000_0000_0000;
const PE_MARKER: u64 = 0xf000_0000_0000_0000;
const OPCODE_SHIFT: u32 = 56;
const REG_MASK: u64 = 0xf;
const ROUTING_ADDR_MASK: u64 = (1_u64 << 44) - 1;

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

pub fn decode_pe_inst(addr: u64) -> Result<inst, &'static str> {
    if !is_pe_request(addr) {
        return Err("address is not encoded as a PE instruction");
    }

    let opcode = ((addr >> OPCODE_SHIFT) & 0xf) as u8;
    let rd = ((addr >> 52) & REG_MASK) as u8;
    let rs0 = ((addr >> 48) & REG_MASK) as u8;
    let rs1 = ((addr >> 44) & REG_MASK) as u8;

    /*
     * TODO
     * current implementation is for end-test only, real integration should use addr as opcode and
     * data payload as oprands.
     *
     * Implement gem5 stub for this function first, then back here and change mapping
     *
     * In this case, decode_pe_inst should contain a data field, as well as enqueue
     */
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
