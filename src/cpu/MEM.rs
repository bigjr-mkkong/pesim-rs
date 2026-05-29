use crate::cpu::pimcpu_types;
use crate::cpu::pipeline::CPU;

pub struct MEM_WB_RF {
    valid: bool,
    flush: bool,

    arith_result: Option<[u32; 4]>,
    ptr_result: Option<pimcpu_types::fatptr_rf>,

    wb_op: pimcpu_types::WBop,
}
