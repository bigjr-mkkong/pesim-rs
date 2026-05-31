use crate::cpu::RF::arch_rf;
use crate::cpu::imem::IMEM;
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

        let mut pc_next: Option<u16> = None;

        for op in real_ops {
            match op {
                arch_action::WritePC { new_pc } => {
                    pc_next = Some(new_pc);
                }
                arch_action::HoldPC => {
                    pc_next = Some(self.get_RF().read_pc());
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

        match pc_next {
            Some(next_pc) => {
                self.get_RF().write_pc(next_pc);
            }
            None => {
                let pc = self.get_RF().read_pc();
                self.get_RF().write_pc(pc.wrapping_add(1));
            }
        }
    }
}
