use crate::cpu::pimcpu_types::arch_action;
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
}
