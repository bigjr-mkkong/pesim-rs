use crate::cpu::RF::arch_rf;
use crate::cpu::pimcpu_types::{CPU_stages, arch_action, inst};
use crate::cpu::pipeline::CPU;
use crate::cpu::signal_scoreboard::{pipeline_action, signal_reason, signal_req};

pub struct IF_ID_rf {
    valid: bool,
    flush: bool,
    pc: u16,

    fetched_inst: inst,
}

impl IF_ID_rf {
    pub const fn new() -> Self {
        Self {
            valid: false,
            flush: false,
            pc: 0,
            fetched_inst: inst::NOP,
        }
    }

    pub fn is_valid(&self) -> bool {
        self.valid
    }

    pub fn get_fetched_inst(&self) -> inst {
        self.fetched_inst
    }
}

impl CPU {
    fn eval_IF(rf: &arch_rf) -> (IF_ID_rf, signal_req, Vec<arch_action>) {
        (
            IF_ID_rf {
                valid: true,
                flush: false,
                fetched_inst: inst::NOP,
                pc: rf.read_pc(),
            },
            signal_req::new(signal_reason::no_reason, CPU_stages::IF, None),
            [arch_action::DoNothing].to_vec(),
        )
    }
}
