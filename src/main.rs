#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]

use cpu::pimcpu_types::{fatptr_rf, inst};
use cpu::pipeline::CPU;
use memory::dramsim3_wrapper::dramsim3_wrapper as dsim3_wrapper;

const DSIM3_CFG_PATH: &str = "/home/michael/Projects/playground/testprogram/pesim-rs/third-party/DRAMsim3/configs/DDR4_4Gb_x4_2400.ini";

const DSIM3_OUT_DIR: &str = "/home/michael/Projects/playground/testprogram/pesim-rs/output";

fn main() {
    let mut dsim3_inst: dsim3_wrapper =
        dsim3_wrapper::new(DSIM3_CFG_PATH, DSIM3_OUT_DIR, 0, 0, 0, 0);

    println!("Dramsim3 tCK: {}", dsim3_inst.get_TCK());

    let addr: u64 = 0x0;

    let is_write: bool = false;
    if dsim3_inst.WillAcceptTransaction(addr, is_write) == true {
        dsim3_inst.AddTransaction(addr, is_write, false);
    }

    loop {
        dsim3_inst.ClockTick();
        let pend_tr = dsim3_inst.get_pend_write(addr, false);
        if pend_tr == 0 {
            break;
        }
    }
    println!("Dramsim3 PING-PONG works");

    let mut pimcpu = CPU::new();

    // Register memory region entry with ID 0.
    // Region starts at address 0 and has 16 entries of [u32; 4].
    pimcpu.get_agu().insert(0, 0, 16);

    // fregs[0] -> MEM[0]
    // fregs[1] -> MEM[1]
    pimcpu.get_RF().write_fregs(0, fatptr_rf::new(0, 0));
    pimcpu.get_RF().write_fregs(1, fatptr_rf::new(0, 1));

    // Initial memory:
    // MEM[0] contains the value to load.
    // MEM[1] is the store destination.
    pimcpu.get_fmem().mem_write_data(0, &[123; 4]);
    pimcpu.get_fmem().mem_write_data(1, &[0; 4]);

    // Initial vRF:
    // v3 intentionally starts as zero.
    // If ST128 reads stale v3, it will store [0; 4] into MEM[1].
    pimcpu.get_RF().write_vregs(3, [0; 4]);
    pimcpu.get_RF().write_vregs(4, [0; 4]);

    let prog: [inst; 3] = [
        // v3 = MEM[0] = [123; 4]
        inst::LD128 { rd: 3, frs: 0 },
        // Should store newly loaded v3 into MEM[1].
        // If load-to-store-data forwarding/stall is broken,
        // this stores stale v3 = [0; 4].
        inst::ST128 { rs: 3, frd: 1 },
        // Reload MEM[1] into v4.
        inst::LD128 { rd: 4, frs: 1 },
    ];

    pimcpu.get_imem().flash_in(&prog);

    for _cycle in 0..30 {
        pimcpu.tick();
    }

    println!("v3 = {:?}", pimcpu.get_RF().read_vregs(3));
    println!("v4 = {:?}", pimcpu.get_RF().read_vregs(4));

    assert_eq!(pimcpu.get_RF().read_vregs(3), [123; 4]);
    assert_eq!(pimcpu.get_RF().read_vregs(4), [123; 4]);
}

mod cpu;
mod errors;
mod memory;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forwards_loaded_vector_into_following_store() {
        let mut pimcpu = CPU::new();

        pimcpu.get_agu().insert(0, 0, 16);
        pimcpu.get_RF().write_fregs(0, fatptr_rf::new(0, 0));
        pimcpu.get_RF().write_fregs(1, fatptr_rf::new(0, 1));
        pimcpu.get_fmem().mem_write_data(0, &[123; 4]);
        pimcpu.get_fmem().mem_write_data(1, &[0; 4]);
        pimcpu.get_RF().write_vregs(3, [0; 4]);
        pimcpu.get_RF().write_vregs(4, [0; 4]);

        let prog: [inst; 3] = [
            inst::LD128 { rd: 3, frs: 0 },
            inst::ST128 { rs: 3, frd: 1 },
            inst::LD128 { rd: 4, frs: 1 },
        ];

        pimcpu.get_imem().flash_in(&prog);

        for _cycle in 0..30 {
            pimcpu.tick();
        }

        assert_eq!(pimcpu.get_RF().read_vregs(3), [123; 4]);
        assert_eq!(pimcpu.get_RF().read_vregs(4), [123; 4]);
    }

    #[test]
    fn forwards_loaded_vector_into_following_alu() {
        let mut pimcpu = CPU::new();

        pimcpu.get_agu().insert(0, 0, 16);
        pimcpu.get_RF().write_fregs(0, fatptr_rf::new(0, 0));

        // MEM[0] = [10; 4]
        pimcpu.get_fmem().mem_write_data(0, &[10; 4]);

        // v3 will receive load result.
        // v2 is ALU operand.
        // v4 is result.
        pimcpu.get_RF().write_vregs(2, [7; 4]);
        pimcpu.get_RF().write_vregs(3, [0; 4]);
        pimcpu.get_RF().write_vregs(4, [0; 4]);

        let prog: [inst; 2] = [
            // v3 = MEM[0] = [10; 4]
            inst::LD128 { rd: 3, frs: 0 },

            // Should use freshly loaded v3.
            // v4 = v3 + v2 = [10; 4] + [7; 4] = [17; 4]
            inst::ADD128 {
                rd: 4,
                rs1: 3,
                rs2: 2,
            },
        ];

        pimcpu.get_imem().flash_in(&prog);

        for _cycle in 0..30 {
            pimcpu.tick();
        }

        assert_eq!(pimcpu.get_RF().read_vregs(3), [10; 4]);
        assert_eq!(pimcpu.get_RF().read_vregs(4), [17; 4]);
    }

    #[test]
    fn forwards_alu_result_into_following_store() {
        let mut pimcpu = CPU::new();

        pimcpu.get_agu().insert(0, 0, 16);
        pimcpu.get_RF().write_fregs(0, fatptr_rf::new(0, 0));

        // MEM[0] starts empty.
        pimcpu.get_fmem().mem_write_data(0, &[0; 4]);

        // v1 + v2 should be stored immediately by ST128.
        pimcpu.get_RF().write_vregs(1, [20; 4]);
        pimcpu.get_RF().write_vregs(2, [5; 4]);
        pimcpu.get_RF().write_vregs(3, [0; 4]);
        pimcpu.get_RF().write_vregs(4, [0; 4]);

        let prog: [inst; 3] = [
            // v3 = [20; 4] + [5; 4] = [25; 4]
            inst::ADD128 {
                rd: 3,
                rs1: 1,
                rs2: 2,
            },

            // Should store freshly computed v3 into MEM[0].
            inst::ST128 { rs: 3, frd: 0 },

            // Reload MEM[0] into v4.
            inst::LD128 { rd: 4, frs: 0 },
        ];

        pimcpu.get_imem().flash_in(&prog);

        for _cycle in 0..30 {
            pimcpu.tick();
        }

        assert_eq!(pimcpu.get_RF().read_vregs(3), [25; 4]);
        assert_eq!(pimcpu.get_RF().read_vregs(4), [25; 4]);
    }

    #[test]
    fn forwards_loaded_fatptr_into_following_fatptr_store() {
        let mut pimcpu = CPU::new();

        pimcpu.get_agu().insert(0, 0, 16);

        // f0 -> MEM[0]
        // f1 -> MEM[1]
        //
        // MEM[0] will contain a fat pointer.
        // MEM[1] will receive that fat pointer through FatPtrSt.
        pimcpu.get_RF().write_fregs(0, fatptr_rf::new(0, 0));
        pimcpu.get_RF().write_fregs(1, fatptr_rf::new(0, 1));

        // This is the fat pointer value we want to load and then store.
        let expected_ptr = fatptr_rf::new(0, 7);

        // Initial f2 is deliberately wrong/stale.
        // If FatPtrSt reads stale f2, the test should fail.
        pimcpu.get_RF().write_fregs(2, fatptr_rf::new(0, 3));

        // MEM[0] = expected_ptr
        // MEM[1] = some wrong initial pointer
        pimcpu.get_fmem().mem_write_fptr(0, &expected_ptr);
        pimcpu
            .get_fmem()
            .mem_write_fptr(1, &fatptr_rf::new(0, 4));

        let prog: [inst; 3] = [
            // f2 = *f0 = MEM[0] = expected_ptr
            inst::FatPtrLD { frd: 2, frs: 0 },

            // *f1 = f2
            //
            // Should store freshly loaded f2, not stale f2.
            inst::FatPtrST { frs: 2, frd: 1 },

            // Reload MEM[1] into f3 so we can check RF state.
            inst::FatPtrLD { frd: 3, frs: 1 },
        ];

        pimcpu.get_imem().flash_in(&prog);

        for _cycle in 0..30 {
            pimcpu.tick();
        }

        assert_eq!(pimcpu.get_RF().read_fregs(2), Some(expected_ptr));
        assert_eq!(pimcpu.get_RF().read_fregs(3), Some(expected_ptr));
    }
}

