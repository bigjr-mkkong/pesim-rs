use crate::cpu::RF::arch_rf;
use crate::cpu::imem::IMEM;
use crate::cpu::pimcpu_types;
use std::collections::HashMap;

use crate::cpu::AGU::AGU_MEM_rf;
use crate::cpu::EX::EX_AGU_rf;
use crate::cpu::ID::ID_EX_rf;
use crate::cpu::IF::IF_ID_rf;
use crate::cpu::MEM::MEM_WB_RF;
use crate::cpu::signal_scoreboard::sig_resolver;

pub const PC_TESTING: u16 = 0xffff;

/*
 * TODO
 * Each pipeline stage should return the next stage, the signal to address timing/flush/stall, as
 * well as the changes to architectural register. The last one is unsolved and need to be done
 */

pub struct CPU {
    imem: IMEM,
    RF: arch_rf,

    if_id_rf: IF_ID_rf,
    id_ex_rf: ID_EX_rf,
    ex_agu_rf: EX_AGU_rf,
    agu_mem_rf: AGU_MEM_rf,
    mem_wb_rf: MEM_WB_RF,

    pipeline_ctrl: sig_resolver,
}

impl CPU {
    pub fn new() -> Self {
        Self {
            imem: IMEM::new(),
            RF: arch_rf::new(),
            if_id_rf: IF_ID_rf::new(),
            id_ex_rf: ID_EX_rf::new(),
            ex_agu_rf: EX_AGU_rf::new(),
            agu_mem_rf: AGU_MEM_rf::new(),
            mem_wb_rf: todo!(),
            pipeline_ctrl: sig_resolver::new(),
        }
    }
}
