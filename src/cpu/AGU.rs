use crate::cpu::pimcpu_types::{AGUop, ALUop, CPU_stages, DMAop, WBop, arch_action, fatptr_rf};
use crate::cpu::pipeline::CPU;

use crate::cpu::EX::EX_AGU_rf;
use crate::cpu::signal_scoreboard::{SigFSM, pipeline_action, signal_reason, signal_req};

pub struct AGU_MEM_rf {
    valid: bool,
    flush: bool,

    phys_addr: Option<u32>,
    arith_in: Option<[u32; 4]>,
    ptr_result: Option<fatptr_rf>,

    dma_op: DMAop,
    wb_op: WBop,
}

impl AGU_MEM_rf {
    pub const fn new() -> Self {
        Self {
            valid: false,
            flush: false,
            phys_addr: None,
            arith_in: None,
            ptr_result: None,

            dma_op: DMAop::NOP,
            wb_op: WBop::NOP,
        }
    }

    pub fn is_valid(&self) -> bool {
        self.valid
    }

    pub fn get_phys_addr(&self) -> Option<u32> {
        self.phys_addr
    }

    pub fn get_arith_in(&self) -> Option<[u32; 4]> {
        self.arith_in
    }

    pub fn get_ptr_result(&self) -> Option<fatptr_rf> {
        self.ptr_result
    }

    pub fn get_dma_op(&self) -> DMAop {
        self.dma_op.clone()
    }

    pub fn get_wb_op(&self) -> WBop {
        self.wb_op.clone()
    }
}

impl CPU {
    fn eval_AGU(ex_agu_rf: &EX_AGU_rf) -> (AGU_MEM_rf, signal_req, Vec<arch_action>) {
        if !ex_agu_rf.is_valid() {
            (
                AGU_MEM_rf {
                    valid: false,
                    flush: false,
                    phys_addr: None,
                    arith_in: None,
                    ptr_result: None,
                    dma_op: DMAop::NOP,
                    wb_op: WBop::NOP,
                },
                signal_req::new(signal_reason::no_reason, CPU_stages::ID, None),
                [arch_action::DoNothing].to_vec(),
            )
        } else {
            match ex_agu_rf.get_agu_op() {
                AGUop::NOP => (
                    AGU_MEM_rf {
                        valid: true,
                        flush: false,
                        phys_addr: None,
                        arith_in: None,
                        ptr_result: None,
                        dma_op: DMAop::NOP,
                        wb_op: WBop::NOP,
                    },
                    signal_req::new(signal_reason::no_reason, CPU_stages::ID, None),
                    [arch_action::DoNothing].to_vec(),
                ),
                /*
                 * TODO:
                 * Need to implement AGU unit first
                 * Implement another AGU_unit.rs in memory/
                 */
                AGUop::CHK { fptr_lit } => {
                    todo!()
                }
                AGUop::ADD {
                    fptr_lit,
                    rs1_lit,
                    idx_imm,
                } => {
                    todo!()
                }
                AGUop::SUB {
                    fptr_lit,
                    rs1_lit,
                    idx_imm,
                } => {
                    todo!()
                }
            }
        }
    }
}
