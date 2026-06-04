use super::*;
use crate::cpu::pimcpu_types::{fatptr_rf, inst};
use crate::sim_engine::engine::Engine;
use crate::memory::mem_portal::dram_req;
use crate::memory::dramsim3_wrapper::dramsim3_wrapper;
use crate::DSIM3_CFG_PATH;
use crate::DSIM3_OUT_DIR;

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
    ];
    engine.get_cpu().get_imem().flash_in(&prog);

    for _cycle in 0..10_000 {
        engine.tick();
    }

    assert_eq!(engine.get_cpu().get_RF().read_vregs(3), [42; 4]);
}

#[test]
fn dramsim3_wrapper_test() {
    let req = dram_req::new(0, false, false);
    let mut dsim3 = dramsim3_wrapper::new(DSIM3_CFG_PATH, DSIM3_OUT_DIR, 0, 0, 0, 0);
    if dsim3.WillAcceptTransaction(0, false) {
        dsim3.AddTransactionReq(req);
    }

    let timeout = 10_000;
    let mut success = true;
    let mut cycl = 0;
    for req in dsim3.ClockTick() {
        if cycl > timeout {
            success = false;
            break;
        }
        println!("Successfully committed one req");
        cycl += 1;
    }

    assert!(success, "dsim3 wrapper failed to response to request");
    println!("dsim3 wrapper success to resposne to request");
}

