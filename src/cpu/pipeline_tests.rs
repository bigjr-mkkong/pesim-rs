use crate::CPU;
use crate::cpu::pimcpu_types::{fatptr_rf, inst};
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

        for _cycle in 0..1000 {
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

        for _cycle in 0..1000 {
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

        for _cycle in 0..1000 {
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
        pimcpu.get_fmem().mem_write_fptr(1, &fatptr_rf::new(0, 4));

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

        for _cycle in 0..1000 {
            pimcpu.tick();
        }

        assert_eq!(pimcpu.get_RF().read_fregs(2), Some(expected_ptr));
        assert_eq!(pimcpu.get_RF().read_fregs(3), Some(expected_ptr));
    }

    #[test]
    fn forwards_alu_result_with_one_instruction_gap() {
        let mut pimcpu = CPU::new();

        pimcpu.get_RF().write_vregs(1, [10; 4]);
        pimcpu.get_RF().write_vregs(2, [3; 4]);
        pimcpu.get_RF().write_vregs(3, [7; 4]);
        pimcpu.get_RF().write_vregs(4, [0; 4]);
        pimcpu.get_RF().write_vregs(5, [0; 4]);
        pimcpu.get_RF().write_vregs(6, [0; 4]);

        let prog: [inst; 3] = [
            // v4 = 10 + 3 = 13
            inst::ADD128 {
                rd: 4,
                rs1: 1,
                rs2: 2,
            },
            // independent instruction
            inst::ADD128 {
                rd: 5,
                rs1: 1,
                rs2: 3,
            },
            // should use forwarded v4 = 13
            // v6 = 13 + 7 = 20
            inst::ADD128 {
                rd: 6,
                rs1: 4,
                rs2: 3,
            },
        ];

        pimcpu.get_imem().flash_in(&prog);

        for _ in 0..1000 {
            pimcpu.tick();
        }

        assert_eq!(pimcpu.get_RF().read_vregs(4), [13; 4]);
        assert_eq!(pimcpu.get_RF().read_vregs(6), [20; 4]);
    }

    #[test]
    fn newest_alu_producer_wins() {
        let mut pimcpu = CPU::new();

        pimcpu.get_RF().write_vregs(1, [10; 4]);
        pimcpu.get_RF().write_vregs(2, [1; 4]);
        pimcpu.get_RF().write_vregs(3, [100; 4]);
        pimcpu.get_RF().write_vregs(4, [0; 4]);
        pimcpu.get_RF().write_vregs(5, [0; 4]);

        let prog: [inst; 4] = [
            // v4 = 10 + 1 = 11
            inst::ADD128 {
                rd: 4,
                rs1: 1,
                rs2: 2,
            },
            // v4 = 11 + 1 = 12
            inst::ADD128 {
                rd: 4,
                rs1: 4,
                rs2: 2,
            },
            // v4 = 12 + 1 = 13
            inst::ADD128 {
                rd: 4,
                rs1: 4,
                rs2: 2,
            },
            // must use newest v4 = 13
            // v5 = 13 + 100 = 113
            inst::ADD128 {
                rd: 5,
                rs1: 4,
                rs2: 3,
            },
        ];

        pimcpu.get_imem().flash_in(&prog);

        for _ in 0..1000 {
            pimcpu.tick();
        }

        assert_eq!(pimcpu.get_RF().read_vregs(4), [13; 4]);
        assert_eq!(pimcpu.get_RF().read_vregs(5), [113; 4]);
    }

    #[test]
    fn store_does_not_use_stale_vector_value() {
        let mut pimcpu = CPU::new();

        pimcpu.get_agu().insert(0, 0, 16);
        pimcpu.get_RF().write_fregs(0, fatptr_rf::new(0, 0));

        pimcpu.get_fmem().mem_write_data(0, &[0; 4]);

        // v3 starts stale.
        pimcpu.get_RF().write_vregs(1, [8; 4]);
        pimcpu.get_RF().write_vregs(2, [9; 4]);
        pimcpu.get_RF().write_vregs(3, [111; 4]);
        pimcpu.get_RF().write_vregs(4, [0; 4]);

        let prog: [inst; 3] = [
            // v3 should become [17; 4]
            inst::ADD128 {
                rd: 3,
                rs1: 1,
                rs2: 2,
            },
            // must store [17; 4], not stale [111; 4]
            inst::ST128 { rs: 3, frd: 0 },
            // reload
            inst::LD128 { rd: 4, frs: 0 },
        ];

        pimcpu.get_imem().flash_in(&prog);

        for _ in 0..1000 {
            pimcpu.tick();
        }

        assert_eq!(pimcpu.get_RF().read_vregs(3), [17; 4]);
        assert_eq!(pimcpu.get_RF().read_vregs(4), [17; 4]);
    }

    #[test]
    fn forwards_fatptr_add_into_following_load() {
        let mut pimcpu = CPU::new();

        pimcpu.get_agu().insert(0, 0, 16);

        // f1 starts at MEM[0]
        pimcpu.get_RF().write_fregs(1, fatptr_rf::new(0, 0));

        // v2[0] = 3, so FatPtrAdd should make f1 point to MEM[3]
        pimcpu.get_RF().write_vregs(2, [3, 0, 0, 0]);
        pimcpu.get_RF().write_vregs(4, [0; 4]);

        pimcpu.get_fmem().mem_write_data(0, &[111; 4]);
        pimcpu.get_fmem().mem_write_data(3, &[999; 4]);

        let prog: [inst; 2] = [
            // f1 = f1 + v2[0] = MEM[3]
            inst::FatPtrADD {
                frd: 1,
                frs: 1,
                rs1: 2,
                imm_idx: 0,
            },
            // must use new f1, so load MEM[3]
            inst::LD128 { rd: 4, frs: 1 },
        ];

        pimcpu.get_imem().flash_in(&prog);

        for _ in 0..1000 {
            pimcpu.tick();
        }

        assert_eq!(pimcpu.get_RF().read_vregs(4), [999; 4]);
    }

    #[test]
    fn forwards_alu_result_into_fatptr_add_index_operand() {
        let mut pimcpu = CPU::new();

        pimcpu.get_agu().insert(0, 0, 16);

        pimcpu.get_RF().write_fregs(1, fatptr_rf::new(0, 0));

        pimcpu.get_RF().write_vregs(1, [1; 4]);
        pimcpu.get_RF().write_vregs(2, [2; 4]);
        pimcpu.get_RF().write_vregs(3, [0; 4]);
        pimcpu.get_RF().write_vregs(4, [0; 4]);

        pimcpu.get_fmem().mem_write_data(3, &[777; 4]);

        let prog: [inst; 3] = [
            // v3 = [3; 4]
            inst::ADD128 {
                rd: 3,
                rs1: 1,
                rs2: 2,
            },
            // f1 = f1 + v3[0] = MEM[3]
            // must use forwarded v3
            inst::FatPtrADD {
                frd: 1,
                frs: 1,
                rs1: 3,
                imm_idx: 0,
            },
            // should load MEM[3]
            inst::LD128 { rd: 4, frs: 1 },
        ];

        pimcpu.get_imem().flash_in(&prog);

        for _ in 0..1000 {
            pimcpu.tick();
        }

        assert_eq!(pimcpu.get_RF().read_vregs(4), [777; 4]);
    }

    #[test]
    fn equal_exit_uses_forwarded_alu_result() {
        let mut pimcpu = CPU::new();

        pimcpu.get_RF().write_vregs(1, [10; 4]);
        pimcpu.get_RF().write_vregs(2, [5; 4]);
        pimcpu.get_RF().write_vregs(3, [15; 4]);
        pimcpu.get_RF().write_vregs(4, [0; 4]);

        let prog: [inst; 2] = [
            // v4 = [15; 4]
            inst::ADD128 {
                rd: 4,
                rs1: 1,
                rs2: 2,
            },
            // should see forwarded v4 and exit
            inst::EqualExit { rd: 4, rs1: 3 },
        ];

        pimcpu.get_imem().flash_in(&prog);

        for _ in 0..1000 {
            pimcpu.tick();
        }

        // Replace this with your actual halted/exited API.
    }

    #[test]
    fn jump_flushes_wrong_path_instruction() {
        let mut pimcpu = CPU::new();

        pimcpu.get_RF().write_vregs(1, [1; 4]);
        pimcpu.get_RF().write_vregs(2, [2; 4]);
        pimcpu.get_RF().write_vregs(3, [3; 4]);
        pimcpu.get_RF().write_vregs(4, [0; 4]);
        pimcpu.get_RF().write_vregs(5, [0; 4]);

        let prog: [inst; 4] = [
            // Jump to instruction 3
            inst::JUMP { inst_imm: 3 },
            // Wrong-path instruction. Must not commit.
            inst::ADD128 {
                rd: 4,
                rs1: 1,
                rs2: 2,
            },
            // Also wrong-path if fetched speculatively.
            inst::ADD128 {
                rd: 5,
                rs1: 1,
                rs2: 2,
            },
            // Real target.
            inst::ADD128 {
                rd: 4,
                rs1: 2,
                rs2: 3,
            },
        ];

        pimcpu.get_imem().flash_in(&prog);

        for _ in 0..1000 {
            pimcpu.tick();
        }

        // If wrong-path instruction committed, v4 might be [3; 4].
        // Correct target gives [5; 4].
        assert_eq!(pimcpu.get_RF().read_vregs(4), [5; 4]);

        // v5 should remain unchanged.
        assert_eq!(pimcpu.get_RF().read_vregs(5), [0; 4]);
    }
}
