use crate::cpu::RF::arch_rf;
use crate::cpu::pimcpu_types::{CPU_stages, arch_action, inst};
use crate::cpu::pipeline::CPU;
use crate::cpu::signal_scoreboard::{pipeline_action, signal_reason, signal_req};
use crate::cpu::imem::IMEM;

pub struct IF_ID_rf {
    valid: bool,
    pc: u16,

    fetched_inst: inst,
}

impl IF_ID_rf {
    pub const fn new() -> Self {
        Self {
            valid: false,
            pc: 0,
            fetched_inst: inst::NOP,
        }
    }

    pub fn is_valid(&self) -> bool {
        self.valid
    }

    pub fn invalidate(&mut self) {
        self.valid = false;
    }

    pub fn get_fetched_inst(&self) -> inst {
        self.fetched_inst
    }
}

impl CPU {
    pub fn eval_IF(&self, rf: &arch_rf, imem: &IMEM) -> (IF_ID_rf, signal_req, Vec<arch_action>) {
        let pc_ = rf.read_pc();
        (
            IF_ID_rf {
                valid: true,
                fetched_inst: imem.read_inst(pc_).expect("No instruction exists in pc: {pc_}"),
                pc: pc_,
            },
            signal_req::new(signal_reason::no_reason, CPU_stages::IF, None),
            [arch_action::DoNothing].to_vec(),
        )
    }
}
