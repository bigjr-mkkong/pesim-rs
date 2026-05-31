use crate::cpu::pimcpu_types::{arch_action, CPU_stages, WBop};
use crate::cpu::pipeline::CPU;
use crate::cpu::signal_scoreboard::{signal_reason, signal_req};
use crate::cpu::MEM::MEM_WB_RF;


impl CPU {
    fn eval_WB(mem_wb_rf: &MEM_WB_RF) -> (Option<()>, signal_req, Vec<arch_action>) {
        if !mem_wb_rf.is_valid() {
            return (
                None,
                signal_req::new(signal_reason::no_reason, CPU_stages::WB, None),
                [arch_action::DoNothing].to_vec(),
            );
        }

        match mem_wb_rf.get_wb_op() {
            WBop::NOP => (
                None,
                signal_req::new(signal_reason::no_reason, CPU_stages::WB, None),
                [arch_action::DoNothing].to_vec(),
            ),
            WBop::WB_VEC { rd } => {
                if let Some(content) = mem_wb_rf.get_arith_result() {
                    (
                        None,
                        signal_req::new(signal_reason::no_reason, CPU_stages::WB, None),
                        [arch_action::WriteVRF {
                            rd: rd as u16,
                            content,
                        }]
                        .to_vec(),
                    )
                } else {
                    (
                        None,
                        signal_req::new(signal_reason::exception, CPU_stages::WB, None),
                        [arch_action::DoNothing].to_vec(),
                    )
                }
            }
            WBop::WB_FPTR { frd } => {
                if let Some(content) = mem_wb_rf.get_ptr_result() {
                    (
                        None,
                        signal_req::new(signal_reason::no_reason, CPU_stages::WB, None),
                        [arch_action::WriteFPTR {
                            frd: frd as u16,
                            content,
                        }]
                        .to_vec(),
                    )
                } else {
                    (
                        None,
                        signal_req::new(signal_reason::exception, CPU_stages::WB, None),
                        [arch_action::DoNothing].to_vec(),
                    )
                }
            }
        }
    }
}

