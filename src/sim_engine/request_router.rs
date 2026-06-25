use crate::PE::types::inst as pe_inst;
use crate::memory::mem_portal::cacheline_payload;

#[derive(Clone, Copy)]
pub enum pim_cmd {
    Fgo(pe_inst),
    CgoStart,
    CgoQuery,
}

impl pim_cmd {
    pub fn expects_write(&self) -> bool {
        !matches!(self, pim_cmd::CgoQuery)
    }
}

pub const PIM_CMD_PAGE_BASE: u64 = 0x1_7ffe_f000;
pub const PIM_CMD_PAGE_END: u64 = 0x1_7ffe_ffff;
pub const PIM_CMD_SLOT_SIZE: u64 = std::mem::size_of::<cacheline_payload>() as u64;

const REG_A_SHIFT: u32 = 0;
const REG_B_SHIFT: u32 = 4;
const REG_C_SHIFT: u32 = 8;
const REG_D_SHIFT: u32 = 12;
const MEM_ADDR_SHIFT: u32 = 16;
const REG_MASK: u64 = 0xf;
const MEM_ADDR_MASK: u64 = 0xffff_ffff;

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

pub fn is_pim_cmd_request(addr: u64) -> bool {
    (PIM_CMD_PAGE_BASE..=PIM_CMD_PAGE_END).contains(&addr)
}

pub fn routing_addr(addr: u64) -> u64 {
    addr
}

pub fn decode_pim_cmd(addr: u64, payload: &cacheline_payload) -> Result<pim_cmd, &'static str> {
    if !is_pim_cmd_request(addr) {
        return Err("address is not inside the PIM command page");
    }

    let offset = addr - PIM_CMD_PAGE_BASE;
    if offset % PIM_CMD_SLOT_SIZE != 0 {
        return Err("PIM command address is not cacheline aligned");
    }

    let opcode = offset / PIM_CMD_SLOT_SIZE;
    let encoded = payload[0];
    let reg_a = ((encoded >> REG_A_SHIFT) & REG_MASK) as u8;
    let reg_b = ((encoded >> REG_B_SHIFT) & REG_MASK) as u8;
    let reg_c = ((encoded >> REG_C_SHIFT) & REG_MASK) as u8;
    let reg_d = ((encoded >> REG_D_SHIFT) & REG_MASK) as u8;
    let mem_addr = ((encoded >> MEM_ADDR_SHIFT) & MEM_ADDR_MASK) as u32;

    match opcode {
        OP_NOP => Ok(pim_cmd::Fgo(pe_inst::NOP)),
        OP_ADD128 => Ok(pim_cmd::Fgo(pe_inst::ADD128 {
            vRD: reg_a,
            vRS0: reg_b,
            vRS1: reg_c,
        })),
        OP_SUB128 => Ok(pim_cmd::Fgo(pe_inst::SUB128 {
            vRD: reg_a,
            vRS0: reg_b,
            vRS1: reg_c,
        })),
        OP_MUL128 => Ok(pim_cmd::Fgo(pe_inst::MUL128 {
            vRD: reg_a,
            vRS0: reg_b,
            vRS1: reg_c,
        })),
        OP_MAC128 => Ok(pim_cmd::Fgo(pe_inst::MAC128 {
            sRD: reg_a,
            sRS0: reg_d,
            vRS0: reg_b,
            vRS1: reg_c,
        })),
        OP_RELU => Ok(pim_cmd::Fgo(pe_inst::ReLU {
            vRD: reg_a,
            vRS0: reg_b,
        })),
        OP_LD128 => Ok(pim_cmd::Fgo(pe_inst::LD128 {
            vRD: reg_a,
            addr: mem_addr,
        })),
        OP_ST128 => Ok(pim_cmd::Fgo(pe_inst::ST128 {
            vRS: reg_a,
            addr: mem_addr,
        })),
        OP_LD32 => Ok(pim_cmd::Fgo(pe_inst::LD32 {
            sRD: reg_a,
            addr: mem_addr,
        })),
        OP_ST32 => Ok(pim_cmd::Fgo(pe_inst::ST32 {
            sRS: reg_a,
            addr: mem_addr,
        })),
        OP_CGO_START => Ok(pim_cmd::CgoStart),
        OP_CGO_QUERY => Ok(pim_cmd::CgoQuery),
        _ => Err("unknown PIM command opcode"),
    }
}

pub fn validate_pim_cmd_access(cmd: pim_cmd, is_write: bool) -> Result<(), &'static str> {
    if cmd.expects_write() == is_write {
        Ok(())
    } else {
        Err("PIM command access direction does not match command opcode")
    }
}
