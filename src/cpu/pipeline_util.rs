use crate::cpu::pimcpu_types::{DMAop, WBop, arch_action, fatptr_rf};
use crate::cpu::pipeline::CPU;
use std::collections::HashSet;

impl CPU {
    pub fn arch_update(&mut self, op_vec: Vec<arch_action>) {
        let mut seen_dest = HashSet::new();
        let mut real_ops = Vec::new();

        for op in op_vec {
            let Some(dest) = op.dest() else {
                continue;
            };

            if !seen_dest.insert(dest) {
                panic!(
                    "Arch Update failed: duplicated architectural destination: {:?}",
                    dest
                );
            }

            real_ops.push(op);
        }

        let current_pc = self.get_RF().read_pc();
        let mut pc_next = current_pc.wrapping_add(1);

        for op in real_ops {
            match op {
                arch_action::WritePC { new_pc } => {
                    pc_next = new_pc;
                }
                arch_action::HoldPC => {
                    pc_next = current_pc;
                }
                arch_action::WriteVRF { rd, content } => {
                    self.get_RF().write_vregs(rd, content);
                }
                arch_action::WriteFPTR { frd, content } => {
                    self.get_RF().write_fregs(frd, content);
                }
                arch_action::WriteMEM_DATA { addr, content } => {
                    self.get_fmem().mem_write_data(addr, &content);
                }
                arch_action::WriteMEM_FPTR { addr, content } => {
                    self.get_fmem().mem_write_fptr(addr, &content);
                }
                arch_action::DoNothing => unreachable!("DoNothing was filtered out"),
            }
        }

        self.get_RF().write_pc(pc_next);
    }

    pub fn ex_bypass_get_rs1(&self, rs1: u8, rs1_lit: [u32; 4]) -> Option<[u32; 4]> {
        self.ex_bypass_get_vreg(rs1, rs1_lit)
    }

    pub fn ex_bypass_get_rs2(&self, rs2: u8, rs2_lit: [u32; 4]) -> Option<[u32; 4]> {
        self.ex_bypass_get_vreg(rs2, rs2_lit)
    }

    pub fn agu_bypass_get_frs(&self, frs: u8, frs_lit: fatptr_rf) -> Option<fatptr_rf> {
        // if frs == 0 {
        //     return Some(frs_lit);
        // }

        if self.agu_mem_rf.is_valid() {
            if let WBop::WB_FPTR { frd } = self.agu_mem_rf.get_wb_op() {
                if frd == frs {
                    if matches!(self.agu_mem_rf.get_dma_op(), DMAop::READ_FPTR { .. }) {
                        return None;
                    }
                    return self.agu_mem_rf.get_ptr_result();
                }
            }
        }

        if self.mem_wb_rf.is_valid() {
            if let WBop::WB_FPTR { frd } = self.mem_wb_rf.get_wb_op() {
                if frd == frs {
                    return self.mem_wb_rf.get_ptr_result();
                }
            }
        }

        Some(frs_lit)
    }

    pub fn agu_bypass_get_rs1(&self, rs1: u8, rs1_lit: [u32; 4]) -> Option<[u32; 4]> {
        if rs1 == 0 {
            return Some(rs1_lit);
        }

        if self.agu_mem_rf.is_valid() {
            if let WBop::WB_VEC { rd } = self.agu_mem_rf.get_wb_op() {
                if rd == rs1 {
                    if matches!(self.agu_mem_rf.get_dma_op(), DMAop::READ_VEC { .. }) {
                        return None;
                    }
                    return self.agu_mem_rf.get_arith_result();
                }
            }
        }

        if self.mem_wb_rf.is_valid() {
            if let WBop::WB_VEC { rd } = self.mem_wb_rf.get_wb_op() {
                if rd == rs1 {
                    return self.mem_wb_rf.get_arith_result();
                }
            }
        }

        Some(rs1_lit)
    }

    pub fn agu_bypass_dma_op(&self, dma_op: DMAop) -> Option<DMAop> {
        match dma_op {
            DMAop::WRITE_VEC { rs, data_lit } => self
                .agu_bypass_get_rs1(rs, data_lit)
                .map(|data_lit| DMAop::WRITE_VEC { rs, data_lit }),
            DMAop::WRITE_FPTR { frs, fptr_data_lit } => self
                .agu_bypass_get_frs(frs, fptr_data_lit)
                .map(|fptr_data_lit| DMAop::WRITE_FPTR { frs, fptr_data_lit }),
            _ => Some(dma_op),
        }
    }

    fn ex_bypass_get_vreg(&self, rs: u8, rs_lit: [u32; 4]) -> Option<[u32; 4]> {
        if rs == 0 {
            return Some(rs_lit);
        }

        if self.ex_agu_rf.is_valid() {
            if let WBop::WB_VEC { rd } = self.ex_agu_rf.get_wb_op() {
                if rd == rs {
                    if matches!(self.ex_agu_rf.get_dma_op(), DMAop::READ_VEC { .. }) {
                        return None;
                    }
                    return self.ex_agu_rf.get_arith_result();
                }
            }
        }

        if self.agu_mem_rf.is_valid() {
            if let WBop::WB_VEC { rd } = self.agu_mem_rf.get_wb_op() {
                if rd == rs {
                    if matches!(self.agu_mem_rf.get_dma_op(), DMAop::READ_VEC { .. }) {
                        return None;
                    }
                    return self.agu_mem_rf.get_arith_result();
                }
            }
        }

        if self.mem_wb_rf.is_valid() {
            if let WBop::WB_VEC { rd } = self.mem_wb_rf.get_wb_op() {
                if rd == rs {
                    return self.mem_wb_rf.get_arith_result();
                }
            }
        }

        Some(rs_lit)
    }
}
