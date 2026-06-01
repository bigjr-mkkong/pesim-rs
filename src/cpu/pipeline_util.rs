use crate::cpu::pimcpu_types::{arch_action, fatptr_rf};
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

    /* TODO
         * "Please implement data forwarding and RAW hazard resolution for my 6-stage pipeline. Follow these strict architectural rules:

Phase 1: Struct Updates

1.    Add rd fields to DMAop::READ_VEC and DMAop::WRITE_VEC.

2.    Add frd fields to DMAop::READ_FPTR and DMAop::WRITE_FPTR.

3.    Add frd field to AGUop::ADD and AGUop::SUB.

4.    Add rs fields (e.g., rs1, rs2) to all ALUop variants.

5.    Ensure CPU::eval_ID() populates all these new fields correctly.

Phase 2: The Forwarding Resolvers
Implement the ex_bypass_get_rs1, ex_bypass_get_rs2, agu_bypass_get_frs, and agu_bypass_get_rs1 functions.

    Rule 1 (The Zero Register): If the requested register is 0, immediately return Some(original_lit). Never forward to register 0.

    Rule 2 (Distance Priority): You must check pipeline registers in order from Youngest to Oldest. For EX, check EX_AGU first, then AGU_MEM, then MEM_WB. The first one that matches the requested rs wins.

    Rule 3 (Load-Use Hazard): If the winning match is a DMAop::READ_VEC (or READ_FPTR) that is currently in the EX_AGU or AGU_MEM stages, the data does not exist yet. You must return None.

    Rule 4 (Success / Default): If a valid forwarding path is found, return Some(forwarded_data). If no pipeline register matches, return Some(original_lit).

Phase 3: The RAW FSM

    In eval_EX() and eval_AGU(), call these bypass functions. If any return None, do NOT perform the ALU/AGU math. Instead, submit a signal_reason::RAW_resolution request to the Scoreboard.

    Implement the RAW_resolution FSM as a 1-cycle stateless FSM. It should simply request pipeline_action::Stall for IF, ID, and EX (and AGU if triggered from AGU).

    Because eval_EX runs combinationally every cycle, it will naturally keep re-submitting this 1-cycle FSM until the Load data finally reaches MEM_WB, at which point the bypass function will return Some(data) and the stall will automatically end."


    Phase 4: rust specific implementation context
    To ensure the code compiles without lifetime or borrow checker errors, please adhere to the following structural realities of my codebase:

        State Location: The pipeline registers are accessed via self inside the CPU struct (e.g., self.ex_agu_rf, self.agu_mem_rf, self.mem_wb_rf).

        Enum Matching: To check if a pipeline register holds a specific operation, you must pattern match the inner enums. For example, to check for a Load in EX_AGU, you will likely match against self.ex_agu_rf.dma_op to see if it is Some(DMAop::READ_VEC { rd, .. }).

        Data Flow (No direct mutation): eval_EX and eval_AGU do NOT mutate the Scoreboard directly. They evaluate combinationally and return the FSM as a Command Pattern. If the bypass function returns None, the evaluation function should abort its normal math and return (Some(Box::new(RAW_resolutionFsm::new())), None).

        The FSM Implementation: When implementing the RAW_resolutionFsm, it must implement my SigFSM trait. Its get_ops() method should return a HashMap<CPU_stages, pipeline_action> containing pipeline_action::Stall for the IF, ID, EX (and AGU if applicable) stages. Its advance_winner() function should simply return true (because it is a 1-cycle stateless FSM).
         */

    pub fn ex_bypass_get_rs1(&self, rs1: u8, rs1_lit: [u32; 4]) -> Option<[u32; 4]> {
        todo!()
    }

    pub fn ex_bypass_get_rs2(&self, rs2: u8, rs2_lit: [u32; 4]) -> Option<[u32; 4]> {
        todo!()
    }

    pub fn agu_bypass_get_frs(&self, frs1: u8, frs_lit: fatptr_rf) -> Option<fatptr_rf> {
        todo!()
    }

    pub fn agu_bypass_get_rs1(&self, rs1: u8, rs1_lit: [u32; 4]) -> Option<[u32; 4]> {
        todo!()
    }
}
