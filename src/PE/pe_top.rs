/*
 * This directory describe the PE architecture for HBM-PIM liked PIM
 * A two cycle PE with no IF(directly receive instruction from host)
 *
 */

use crate::PE::ISSUE::ISSUE_EX_RF;
use crate::PE::RF::arch_rf;
use crate::PE::types::inst;

pub struct PE {
    host_inst: inst,
    issue_ex_rf: ISSUE_EX_RF,
    Arf: arch_rf,
}

impl PE {
    pub fn new() -> Self {
        Self {
            host_inst: inst::NOP,
            issue_ex_rf: ISSUE_EX_RF::new(),
            Arf: arch_rf::new(),
        }
    }

    pub fn tick() {
        /*
         * TODO
         * Implement pipeline control logic by placing ISSUE.rs and EX.rs eval functions here
         * Use the same idea as src/cpu/pipeline.rs, but here we have a simplified model as PE only
         * contain two stage
         * In this case, the only signal need to be handled is MEM_stop
         */
    }
}
