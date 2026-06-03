use super::*;
use crate::cpu::pimcpu_types::{fatptr_rf, inst};
use crate::sim_engine::engine::Engine;

// #[test]
pub fn engine_runs_pim_load_through_mem_fsm_and_dram_portal() {
    let mut engine = Engine::new_pim_only();

    engine.get_cpu().get_agu().insert(0, 0, 16);
    engine
        .get_cpu()
        .get_RF()
        .write_fregs(0, fatptr_rf::new(0, 0));
    engine.get_cpu().get_fmem().mem_write_data(0, &[42; 4]);
    engine.get_cpu().get_RF().write_vregs(3, [0; 4]);

    let prog = [inst::LD128 { rd: 3, frs: 0 },
    inst::LD128 { rd: 4, frs: 0 },
    inst::LD128 { rd: 5, frs: 0 },
    inst::LD128 { rd: 6, frs: 0 },
    ];
    engine.get_cpu().get_imem().flash_in(&prog);

    for _cycle in 0..10_000 {
        engine.tick();
    }

    assert_eq!(engine.get_cpu().get_RF().read_vregs(3), [42; 4]);
}
