use crate::cpu::pimcpu_types::{AGUop, ALUop, CPU_stages, DMAop, WBop, arch_action, fatptr_rf};
use crate::cpu::signal_scoreboard::{SigFSM, pipeline_action, signal_reason, signal_req};
use crate::cpu::pipeline::CPU;

use crate::cpu::AGU::AGU_MEM_rf;

pub struct MEM_WB_RF {
    valid: bool,
    flush: bool,

    arith_result: Option<[u32; 4]>,
    ptr_result: Option<fatptr_rf>,

    wb_op: WBop,
}

impl MEM_WB_RF{
    pub const fn new() -> Self{
        Self {
            valid: false,
            flush: false,

            arith_result: None,
            ptr_result: None,
            wb_op: WBop::NOP
        }
    }

    pub fn is_valid(&self) -> bool{
        self.valid
    }

    pub fn get_arith_result(&self) -> Option<[u32; 4]> {
        self.arith_result
    }

    pub fn get_ptr_result(&self) -> Option<fatptr_rf> {
        self.ptr_result
    }

    pub fn get_wb_op(&self) -> WBop{
        self.wb_op
    }
}


impl CPU {
    fn eval_MEM(agu_mem_rf: &AGU_MEM_rf) -> (MEM_WB_RF, signal_req, Vec<arch_action>) {
        if !agu_mem_rf.is_valid() {
            (
                MEM_WB_RF{
                    valid: false,
                    flush: false,
                    arith_result: None,
                    ptr_result: None,
                    wb_op: WBop::NOP
                },
                signal_req::new(signal_reason::no_reason, CPU_stages::MEM, None),
                [arch_action::DoNothing].to_vec(),
            )
        } else {
            match agu_mem_rf.get_dma_op() {
                DMAop::NOP => {
                    todo!()
                },
                DMAop::READ_VEC => {
                    todo!()

                },
                DMAop::WRITE_VEC { data_lit } => {
                    todo!()

                },
                DMAop::READ_FPTR => {
                    todo!()

                },
                DMAop::WRITE_FPTR { fptr_data_lit } => {
                    todo!()

                }
            }
        }
    }

}
