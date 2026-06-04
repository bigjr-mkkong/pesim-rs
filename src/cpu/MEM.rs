use crate::cpu::AGU::AGU_MEM_rf;
use crate::cpu::pimcpu_types::{CPU_stages, DMAop, WBop, arch_action, fatptr_rf};
use crate::cpu::pipeline::CPU;
use crate::cpu::signal_scoreboard::{SigFSM, pipeline_action, signal_reason, signal_req};
use std::collections::{HashMap, HashSet};

use crate::memory::flat_memory::flat_mem;
use crate::memory::mem_portal::{dram_portal, dram_req, portal_req};

#[derive(Clone, Copy)]
pub struct MEM_WB_RF {
    valid: bool,

    arith_result: Option<[u32; 4]>,
    ptr_result: Option<fatptr_rf>,

    wb_op: WBop,
}

impl MEM_WB_RF {
    pub const fn new() -> Self {
        Self {
            valid: false,

            arith_result: None,
            ptr_result: None,
            wb_op: WBop::NOP,
        }
    }

    pub fn is_valid(&self) -> bool {
        self.valid
    }

    pub fn invalidate(&mut self) {
        self.valid = false;
    }

    pub fn get_arith_result(&self) -> Option<[u32; 4]> {
        self.arith_result
    }

    pub fn get_ptr_result(&self) -> Option<fatptr_rf> {
        self.ptr_result
    }

    pub fn get_wb_op(&self) -> WBop {
        self.wb_op
    }
}

impl CPU {
    pub fn eval_MEM(
        &self,
        agu_mem_rf: &AGU_MEM_rf,
        fmem: &flat_mem,
    ) -> (MEM_WB_RF, signal_req, Vec<arch_action>) {
        if !agu_mem_rf.is_valid() {
            (
                MEM_WB_RF {
                    valid: false,
                    arith_result: None,
                    ptr_result: None,
                    wb_op: WBop::NOP,
                },
                signal_req::new(signal_reason::no_reason, CPU_stages::MEM, None),
                [arch_action::DoNothing].to_vec(),
            )
        } else {
            match agu_mem_rf.get_dma_op() {
                DMAop::NOP => (
                    MEM_WB_RF {
                        valid: true,
                        arith_result: agu_mem_rf.get_arith_result(),
                        ptr_result: agu_mem_rf.get_ptr_result(),
                        wb_op: agu_mem_rf.get_wb_op(),
                    },
                    signal_req::new(signal_reason::no_reason, CPU_stages::MEM, None),
                    [arch_action::DoNothing].to_vec(),
                ),
                DMAop::READ_VEC { .. } => {
                    if let Some(paddr) = agu_mem_rf.get_phys_addr() {
                        (
                            MEM_WB_RF {
                                valid: true,
                                arith_result: fmem.mem_read_data(paddr),
                                ptr_result: None,
                                wb_op: agu_mem_rf.get_wb_op(),
                            },
                            signal_req::new(
                                signal_reason::MEM_block {
                                    addr: paddr as u64,
                                    is_read: true,
                                },
                                CPU_stages::MEM,
                                Some(HashSet::<CPU_stages>::from([
                                    CPU_stages::IF,
                                    CPU_stages::ID,
                                    CPU_stages::EX,
                                    CPU_stages::AGU,
                                ])),
                            ),
                            [arch_action::DoNothing].to_vec(),
                        )
                    } else {
                        (
                            MEM_WB_RF {
                                valid: true,
                                arith_result: None,
                                ptr_result: None,
                                wb_op: WBop::NOP,
                            },
                            signal_req::new(signal_reason::no_reason, CPU_stages::MEM, None),
                            [arch_action::DoNothing].to_vec(),
                        )
                    }
                }
                DMAop::WRITE_VEC { data_lit, .. } => {
                    if let Some(paddr) = agu_mem_rf.get_phys_addr() {
                        (
                            MEM_WB_RF {
                                valid: true,
                                arith_result: None,
                                ptr_result: None,
                                wb_op: WBop::NOP,
                            },
                            signal_req::new(
                                signal_reason::MEM_block {
                                    addr: paddr as u64,
                                    is_read: false,
                                },
                                CPU_stages::MEM,
                                Some(HashSet::<CPU_stages>::from([
                                    CPU_stages::IF,
                                    CPU_stages::ID,
                                    CPU_stages::EX,
                                    CPU_stages::AGU,
                                ])),
                            ),
                            [arch_action::WriteMEM_DATA {
                                addr: paddr,
                                content: data_lit,
                            }]
                            .to_vec(),
                        )
                    } else {
                        (
                            MEM_WB_RF {
                                valid: true,
                                arith_result: None,
                                ptr_result: None,
                                wb_op: WBop::NOP,
                            },
                            signal_req::new(signal_reason::no_reason, CPU_stages::MEM, None),
                            [arch_action::DoNothing].to_vec(),
                        )
                    }
                }
                DMAop::READ_FPTR { .. } => {
                    if let Some(paddr) = agu_mem_rf.get_phys_addr() {
                        (
                            MEM_WB_RF {
                                valid: true,
                                arith_result: None,
                                ptr_result: fmem.mem_read_fptr(paddr),
                                wb_op: agu_mem_rf.get_wb_op(),
                            },
                            signal_req::new(
                                signal_reason::MEM_block {
                                    addr: paddr as u64,
                                    is_read: true,
                                },
                                CPU_stages::MEM,
                                Some(HashSet::<CPU_stages>::from([
                                    CPU_stages::IF,
                                    CPU_stages::ID,
                                    CPU_stages::EX,
                                    CPU_stages::AGU,
                                ])),
                            ),
                            [arch_action::DoNothing].to_vec(),
                        )
                    } else {
                        (
                            MEM_WB_RF {
                                valid: true,
                                arith_result: None,
                                ptr_result: None,
                                wb_op: WBop::NOP,
                            },
                            signal_req::new(signal_reason::no_reason, CPU_stages::MEM, None),
                            [arch_action::DoNothing].to_vec(),
                        )
                    }
                }
                DMAop::WRITE_FPTR { fptr_data_lit, .. } => {
                    if let Some(paddr) = agu_mem_rf.get_phys_addr() {
                        (
                            MEM_WB_RF {
                                valid: true,
                                arith_result: None,
                                ptr_result: None,
                                wb_op: WBop::NOP,
                            },
                            signal_req::new(
                                signal_reason::MEM_block {
                                    addr: paddr as u64,
                                    is_read: false,
                                },
                                CPU_stages::MEM,
                                Some(HashSet::<CPU_stages>::from([
                                    CPU_stages::IF,
                                    CPU_stages::ID,
                                    CPU_stages::EX,
                                    CPU_stages::AGU,
                                ])),
                            ),
                            [arch_action::WriteMEM_FPTR {
                                addr: paddr,
                                content: fptr_data_lit,
                            }]
                            .to_vec(),
                        )
                    } else {
                        (
                            MEM_WB_RF {
                                valid: true,
                                arith_result: None,
                                ptr_result: None,
                                wb_op: WBop::NOP,
                            },
                            signal_req::new(signal_reason::no_reason, CPU_stages::MEM, None),
                            [arch_action::DoNothing].to_vec(),
                        )
                    }
                }
            }
        }
    }
}

#[derive(Clone, Copy)]
pub enum MEM_stop_FSM_states {
    Submit,
    Stall,
    WriteBack,
    Release,
    Idle,
}

#[derive(Clone)]
pub struct MEM_stop_FSM {
    state: MEM_stop_FSM_states,
    state_next: MEM_stop_FSM_states,
    req: Option<dram_req>,
    dram_port: Option<dram_portal>,
}

impl SigFSM for MEM_stop_FSM {
    fn reason(&self) -> signal_reason {
        signal_reason::mem_block_kind()
    }

    //action should return Normal when reaching the finish state
    fn action(&self) -> pipeline_action {
        match self.state {
            MEM_stop_FSM_states::Submit
            | MEM_stop_FSM_states::Stall
            | MEM_stop_FSM_states::WriteBack
            | MEM_stop_FSM_states::Release => pipeline_action::Stall,
            MEM_stop_FSM_states::Idle => pipeline_action::Normal,
        }
    }

    fn get_ops(&self) -> HashMap<CPU_stages, pipeline_action> {
        let mut ops = HashMap::<CPU_stages, pipeline_action>::from([
            (CPU_stages::IF, pipeline_action::Stall),  //stall ifid
            (CPU_stages::ID, pipeline_action::Stall),  //stall idex
            (CPU_stages::EX, pipeline_action::Stall),  //stall exagu
            (CPU_stages::AGU, pipeline_action::Stall), //stall agumem
        ]);

        match self.state {
            MEM_stop_FSM_states::Submit | MEM_stop_FSM_states::Stall => {
                // The memory transaction is still in flight, so MEM must keep
                // holding AGU/MEM instead of producing a premature WB value.
                ops.insert(CPU_stages::MEM, pipeline_action::Stall);
                ops
            }
            MEM_stop_FSM_states::WriteBack => {
                // The transaction completed; allow MEM to publish MEM/WB while
                // the younger stages remain held for one more cycle.
                ops
            }
            MEM_stop_FSM_states::Release => {
                // Keep the WB latch stable for the release cycle so a dependent
                // operation can consume the WB forwarding path instead of RF.
                HashMap::from([(CPU_stages::WB, pipeline_action::Stall)])
            }
            MEM_stop_FSM_states::Idle => HashMap::new(),
        }
    }

    fn advance_winner(&mut self, sig_reason: signal_reason) -> bool {
        self.state_next = match self.state {
            MEM_stop_FSM_states::Submit => {
                if let signal_reason::MEM_block { addr, is_read } = sig_reason {
                    let req = dram_req::new(addr, is_read, true);
                    self.req = Some(req.clone());

                    if let Some(dram_port) = &mut self.dram_port {
                        dram_port.submit(portal_req::PIM_REQ { req });
                        MEM_stop_FSM_states::Stall
                    } else {
                        MEM_stop_FSM_states::WriteBack
                    }
                } else {
                    MEM_stop_FSM_states::WriteBack
                }
            }
            MEM_stop_FSM_states::Stall => {
                if let (Some(req), Some(dram_port)) = (&self.req, &mut self.dram_port) {
                    if dram_port.take_completed(req).is_some() {
                        MEM_stop_FSM_states::WriteBack
                    } else {
                        MEM_stop_FSM_states::Stall
                    }
                } else {
                    MEM_stop_FSM_states::WriteBack
                }
            }
            MEM_stop_FSM_states::WriteBack => MEM_stop_FSM_states::Release,
            MEM_stop_FSM_states::Release => MEM_stop_FSM_states::Idle,
            MEM_stop_FSM_states::Idle => MEM_stop_FSM_states::Idle,
        };

        self.state = self.state_next;
        return true;
    }

    fn handle_blocked(&mut self) {}
}

impl MEM_stop_FSM {
    pub const fn new() -> Self {
        Self {
            state: MEM_stop_FSM_states::Submit,
            state_next: MEM_stop_FSM_states::Submit,
            req: None,
            dram_port: None,
        }
    }

    pub fn new_with_dram_port(dram_port: dram_portal) -> Self {
        Self {
            state: MEM_stop_FSM_states::Submit,
            state_next: MEM_stop_FSM_states::Submit,
            req: None,
            dram_port: Some(dram_port),
        }
    }
}
