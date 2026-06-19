/*
 * This directory describe the PE architecture for HBM-PIM liked PIM
 * A two cycle PE with no IF(directly receive instruction from host)
 *
 */

use crate::PE::EX::{EX_WB_RF, PE_MEM_stop_FSM};
use crate::PE::ISSUE::ISSUE_EX_RF;
use crate::PE::RF::arch_rf;
use crate::PE::types::{PE_stages, arch_action, inst};
use crate::cpu::signal_scoreboard::pipeline_action;
use crate::memory::flat_memory::pe_flat_mem;
use crate::memory::mem_portal::dram_portal;
use std::collections::{HashSet, VecDeque};

pub struct PE {
    imem: VecDeque<inst>,
    fetch_next_allowed: bool,
    finished: bool,
    issue_ex_rf: ISSUE_EX_RF,
    pub(crate) ex_wb_forward_rf: EX_WB_RF,
    Arf: arch_rf,
    fmem: pe_flat_mem,
    mem_stop_fsm: PE_MEM_stop_FSM,
}

impl PE {
    pub fn new() -> Self {
        Self {
            imem: VecDeque::new(),
            fetch_next_allowed: false,
            finished: false,
            issue_ex_rf: ISSUE_EX_RF::new(),
            ex_wb_forward_rf: EX_WB_RF::new(),
            Arf: arch_rf::new(),
            fmem: pe_flat_mem::new(),
            mem_stop_fsm: PE_MEM_stop_FSM::new(),
        }
    }

    pub fn new_with_dram_port(dram_port: dram_portal) -> Self {
        Self {
            imem: VecDeque::new(),
            fetch_next_allowed: false,
            finished: false,
            issue_ex_rf: ISSUE_EX_RF::new(),
            ex_wb_forward_rf: EX_WB_RF::new(),
            Arf: arch_rf::new(),
            fmem: pe_flat_mem::new(),
            mem_stop_fsm: PE_MEM_stop_FSM::new_with_dram_port(dram_port),
        }
    }

    pub fn push_host_inst(&mut self, host_inst: inst) {
        if !matches!(host_inst, inst::NOP) {
            self.imem.push_back(host_inst);
        }
    }

    pub fn set_host_inst(&mut self, host_inst: inst) {
        self.push_host_inst(host_inst);
    }

    pub fn allow_next(&mut self) {
        self.fetch_next_allowed = true;
    }

    pub fn has_finished(&mut self) -> bool {
        let finished = self.finished;
        self.finished = false;
        finished
    }

    fn fetch_inst(&mut self) -> inst {
        if self.fetch_next_allowed {
            self.fetch_next_allowed = false;
            self.imem.pop_front().unwrap_or(inst::NOP)
        } else {
            inst::NOP
        }
    }

    pub fn get_Arf(&mut self) -> &mut arch_rf {
        &mut self.Arf
    }

    pub fn get_fmem(&mut self) -> &mut pe_flat_mem {
        &mut self.fmem
    }

    pub fn tick(&mut self) {
        let issue_ex_snapshot = self.issue_ex_rf;
        let (ex_wb_next, ex_sigreq, ex_archop) = self.eval_EX(&issue_ex_snapshot, &self.fmem);

        let pipeline_op = self.mem_stop_fsm.get_decision(ex_sigreq);
        let stage_action = |stage| {
            pipeline_op
                .get(&stage)
                .copied()
                .unwrap_or(pipeline_action::Normal)
        };

        if stage_action(PE_stages::EX) == pipeline_action::Normal {
            let completed_arch_update = ex_archop.iter().any(|op| op.dest().is_some());
            self.arch_update(ex_archop);
            if completed_arch_update {
                self.finished = true;
            }
        }

        if stage_action(PE_stages::EX) == pipeline_action::Normal && ex_wb_next.is_valid() {
            self.ex_wb_forward_rf = ex_wb_next;
        }

        let stage_op = |producer_act, consumer_act| match (producer_act, consumer_act) {
            (_, pipeline_action::Stall) => pipeline_action::Stall,
            (pipeline_action::Normal, pipeline_action::Normal) => pipeline_action::Normal,
            (pipeline_action::Stall, pipeline_action::Normal) => pipeline_action::Flush,
            (_, pipeline_action::Flush | pipeline_action::END) => pipeline_action::Flush,
            (pipeline_action::Flush | pipeline_action::END, _) => pipeline_action::Flush,
        };

        match stage_op(stage_action(PE_stages::ISSUE), stage_action(PE_stages::EX)) {
            pipeline_action::Normal => {
                let issue_inst = self.fetch_inst();
                self.issue_ex_rf = Self::eval_ISSUE(issue_inst, &self.Arf);
            }
            pipeline_action::Stall => {}
            pipeline_action::Flush | pipeline_action::END => self.issue_ex_rf = ISSUE_EX_RF::new(),
        }
    }

    fn arch_update(&mut self, op_vec: Vec<arch_action>) {
        let mut seen_dest = HashSet::new();
        let mut real_ops = Vec::new();

        for op in op_vec {
            let Some(dest) = op.dest() else {
                continue;
            };

            if !seen_dest.insert(dest) {
                panic!(
                    "PE arch update failed: duplicated architectural destination: {:?}",
                    dest
                );
            }

            real_ops.push(op);
        }

        for op in real_ops {
            match op {
                arch_action::WriteVRF { vRD, content } => self.Arf.write_vRF(vRD, content),
                arch_action::WriteSRF { sRD, content } => self.Arf.write_sRF(sRD, content),
                arch_action::WriteMEM_V { addr, content } => {
                    self.fmem.mem_write_v(addr, &content);
                }
                arch_action::WriteMEM_S { addr, content } => {
                    self.fmem.mem_write_s(addr, content);
                }
                arch_action::DoNothing => unreachable!("DoNothing was filtered out"),
            }
        }
    }
}

fn seed_vrf(pe: &mut PE, reg: u8, value: [i16; 8]) {
    pe.get_Arf().write_vRF(reg, value);
}

fn seed_srf(pe: &mut PE, reg: u8, value: i32) {
    pe.get_Arf().write_sRF(reg, value);
}

fn seed_mem_v(pe: &mut PE, addr: u32, value: [i16; 8]) {
    pe.get_fmem().mem_write_v(addr, &value).unwrap();
}

fn seed_mem_s(pe: &mut PE, addr: u32, value: i32) {
    pe.get_fmem().mem_write_s(addr, value).unwrap();
}

fn read_vrf(pe: &mut PE, reg: u8) -> [i16; 8] {
    pe.get_Arf().read_vRF(reg)
}

fn read_srf(pe: &mut PE, reg: u8) -> i32 {
    pe.get_Arf().read_sRF(reg)
}

fn read_mem_v(pe: &mut PE, addr: u32) -> [i16; 8] {
    pe.get_fmem().mem_read_v(addr).unwrap()
}

fn read_mem_s(pe: &mut PE, addr: u32) -> i32 {
    pe.get_fmem().mem_read_s(addr).unwrap()
}

fn run_rf_inst(pe: &mut PE, instruction: inst) -> usize {
    pe.push_host_inst(instruction);
    pe.allow_next();
    pe.tick();
    pe.tick();
    2
}

fn run_mem_inst_until(pe: &mut PE, instruction: inst, complete: impl Fn(&mut PE) -> bool) -> usize {
    pe.push_host_inst(instruction);
    pe.allow_next();
    pe.tick();

    for cycles in 2..=128 {
        pe.tick();
        if complete(pe) {
            return cycles;
        }
    }

    panic!("memory instruction did not complete within 128 cycles");
}

#[test]
fn PE_ADD_test() {
    let mut pe = PE::new();
    seed_vrf(&mut pe, 1, [1, 2, 3, 4, 5, 6, 7, 8]);
    seed_vrf(&mut pe, 2, [8, 7, 6, 5, 4, 3, 2, 1]);

    let cycles = run_rf_inst(
        &mut pe,
        inst::ADD128 {
            vRD: 3,
            vRS0: 1,
            vRS1: 2,
        },
    );

    assert_eq!(read_vrf(&mut pe, 3), [9; 8]);
    assert_eq!(cycles, 2);
}

#[test]
fn PE_SUB_test() {
    let mut pe = PE::new();
    seed_vrf(&mut pe, 1, [10, 20, 30, 40, 50, 60, 70, 80]);
    seed_vrf(&mut pe, 2, [1, 2, 3, 4, 5, 6, 7, 8]);

    let cycles = run_rf_inst(
        &mut pe,
        inst::SUB128 {
            vRD: 3,
            vRS0: 1,
            vRS1: 2,
        },
    );

    assert_eq!(read_vrf(&mut pe, 3), [9, 18, 27, 36, 45, 54, 63, 72]);
    assert_eq!(cycles, 2);
}

#[test]
fn PE_MUL_test() {
    let mut pe = PE::new();
    seed_vrf(&mut pe, 1, [1, 2, 3, 4, 5, 6, 7, 8]);
    seed_vrf(&mut pe, 2, [8, 7, 6, 5, 4, 3, 2, 1]);

    let cycles = run_rf_inst(
        &mut pe,
        inst::MUL128 {
            vRD: 3,
            vRS0: 1,
            vRS1: 2,
        },
    );

    assert_eq!(read_vrf(&mut pe, 3), [8, 14, 18, 20, 20, 18, 14, 8]);
    assert_eq!(cycles, 2);
}

#[test]
fn PE_MAC_test() {
    let mut pe = PE::new();
    seed_srf(&mut pe, 1, 100);
    seed_vrf(&mut pe, 1, [1, 2, 3, 4, 5, 6, 7, 8]);
    seed_vrf(&mut pe, 2, [8, 7, 6, 5, 4, 3, 2, 1]);

    let cycles = run_rf_inst(
        &mut pe,
        inst::MAC128 {
            sRD: 2,
            sRS0: 1,
            vRS0: 1,
            vRS1: 2,
        },
    );

    assert_eq!(read_srf(&mut pe, 2), 220);
    assert_eq!(cycles, 2);
}

#[test]
fn PE_LD128_test() {
    let mut pe = PE::new();
    let data = [11, 22, 33, 44, 55, 66, 77, 88];
    seed_mem_v(&mut pe, 0x100, data);

    let cycles = run_mem_inst_until(
        &mut pe,
        inst::LD128 {
            vRD: 1,
            addr: 0x100,
        },
        |pe| read_vrf(pe, 1) == data,
    );

    assert_eq!(read_vrf(&mut pe, 1), data);
    assert!(cycles >= 2);
}

#[test]
fn PE_ST128_test() {
    let mut pe = PE::new();
    let data = [3, 1, 4, 1, 5, 9, 2, 6];
    seed_vrf(&mut pe, 1, data);

    let cycles = run_mem_inst_until(
        &mut pe,
        inst::ST128 {
            vRS: 1,
            addr: 0x104,
        },
        |pe| read_mem_v(pe, 0x104) == data,
    );

    assert_eq!(read_mem_v(&mut pe, 0x104), data);
    assert!(cycles >= 2);
}

#[test]
fn PE_LD32_test() {
    let mut pe = PE::new();
    seed_mem_s(&mut pe, 0x200, 12345);

    let cycles = run_mem_inst_until(
        &mut pe,
        inst::LD32 {
            sRD: 1,
            addr: 0x200,
        },
        |pe| read_srf(pe, 1) == 12345,
    );

    assert_eq!(read_srf(&mut pe, 1), 12345);
    assert!(cycles >= 2);
}

#[test]
fn PE_ST32_test() {
    let mut pe = PE::new();
    seed_srf(&mut pe, 1, -6789);

    let cycles = run_mem_inst_until(
        &mut pe,
        inst::ST32 {
            sRS: 1,
            addr: 0x204,
        },
        |pe| read_mem_s(pe, 0x204) == -6789,
    );

    assert_eq!(read_mem_s(&mut pe, 0x204), -6789);
    assert!(cycles >= 2);
}

#[test]
fn PE_EX_to_EX_forward_test() {
    let mut pe = PE::new();
    seed_vrf(&mut pe, 1, [1, 2, 3, 4, 5, 6, 7, 8]);
    seed_vrf(&mut pe, 2, [8, 7, 6, 5, 4, 3, 2, 1]);
    seed_vrf(&mut pe, 5, [10; 8]);

    pe.push_host_inst(inst::ADD128 {
        vRD: 3,
        vRS0: 1,
        vRS1: 2,
    });
    pe.push_host_inst(inst::ADD128 {
        vRD: 4,
        vRS0: 3,
        vRS1: 5,
    });

    pe.allow_next();
    pe.tick();
    pe.allow_next();
    pe.tick();
    pe.tick();

    assert_eq!(read_vrf(&mut pe, 3), [9; 8]);
    assert_eq!(read_vrf(&mut pe, 4), [19; 8]);
}

#[test]
fn PE_imem_waits_for_allow_next_test() {
    let mut pe = PE::new();
    seed_vrf(&mut pe, 1, [1; 8]);
    seed_vrf(&mut pe, 2, [2; 8]);

    pe.push_host_inst(inst::ADD128 {
        vRD: 3,
        vRS0: 1,
        vRS1: 2,
    });

    pe.tick();
    pe.tick();

    assert_eq!(read_vrf(&mut pe, 3), [0; 8]);
    assert!(!pe.has_finished());

    pe.allow_next();
    pe.tick();
    pe.tick();

    assert_eq!(read_vrf(&mut pe, 3), [3; 8]);
    assert!(pe.has_finished());
    assert!(!pe.has_finished());
}

#[test]
fn PE_allow_next_fetches_one_buffered_instruction_test() {
    let mut pe = PE::new();
    seed_vrf(&mut pe, 1, [4; 8]);
    seed_vrf(&mut pe, 2, [5; 8]);
    seed_vrf(&mut pe, 4, [20; 8]);
    seed_vrf(&mut pe, 5, [3; 8]);

    pe.push_host_inst(inst::ADD128 {
        vRD: 3,
        vRS0: 1,
        vRS1: 2,
    });
    pe.push_host_inst(inst::SUB128 {
        vRD: 6,
        vRS0: 4,
        vRS1: 5,
    });

    pe.allow_next();
    pe.tick();
    pe.tick();

    assert_eq!(read_vrf(&mut pe, 3), [9; 8]);
    assert_eq!(read_vrf(&mut pe, 6), [0; 8]);
    assert!(pe.has_finished());

    pe.tick();
    assert_eq!(read_vrf(&mut pe, 6), [0; 8]);
    assert!(!pe.has_finished());

    pe.allow_next();
    pe.tick();
    pe.tick();

    assert_eq!(read_vrf(&mut pe, 6), [17; 8]);
    assert!(pe.has_finished());
}

#[test]
fn PE_has_finished_tracks_memory_arch_update_test() {
    let mut pe = PE::new();
    seed_mem_s(&mut pe, 0x300, 2468);

    pe.push_host_inst(inst::LD32 {
        sRD: 7,
        addr: 0x300,
    });
    pe.allow_next();
    pe.tick();

    assert!(!pe.has_finished());

    for _ in 0..128 {
        pe.tick();
        if read_srf(&mut pe, 7) == 2468 {
            assert!(pe.has_finished());
            assert!(!pe.has_finished());
            return;
        }
    }

    panic!("memory instruction did not complete within 128 cycles");
}

#[test]
fn PE_nop_does_not_finish_test() {
    let mut pe = PE::new();

    pe.set_host_inst(inst::NOP);
    pe.allow_next();
    pe.tick();
    pe.tick();

    assert!(!pe.has_finished());
}
