use crate::PE::pe_top::PE;
use crate::PE::types::inst;

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
    assert!(pe.has_buffered_inst());

    pe.allow_next();
    pe.tick();
    pe.tick();

    assert_eq!(read_vrf(&mut pe, 3), [9; 8]);
    assert_eq!(read_vrf(&mut pe, 6), [0; 8]);
    assert!(pe.has_finished());
    assert!(pe.has_buffered_inst());

    pe.tick();
    assert_eq!(read_vrf(&mut pe, 6), [0; 8]);
    assert!(!pe.has_finished());

    pe.allow_next();
    pe.tick();
    pe.tick();

    assert_eq!(read_vrf(&mut pe, 6), [17; 8]);
    assert!(pe.has_finished());
    assert!(!pe.has_buffered_inst());
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
fn PE_valid_nop_finishes_test() {
    let mut pe = PE::new();

    pe.push_host_inst(inst::NOP);
    pe.allow_next();
    pe.tick();
    pe.tick();

    assert!(pe.has_finished());
}
