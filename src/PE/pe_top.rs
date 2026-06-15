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
        //This function will assemble each stage together and perform update over RF/MEM
    }
}
