use super::*;
use crate::DSIM3_CFG_PATH;
use crate::DSIM3_OUT_DIR;
use crate::cpu::pimcpu_types::{fatptr_rf, inst};
use crate::memory::dramsim3_wrapper::dramsim3_wrapper;
use crate::memory::mem_portal::dram_req;
use crate::sim_engine::engine::Engine;

pub fn engine_runs_pim_load_through_mem_fsm_and_dram_portal() {
    // let mut engine = Engine::new_pim_only();
    let mut engine = Engine::new_scheduled_host_pim();

    engine.get_cpu().get_agu().insert(0, 0, 16);
    engine
        .get_cpu()
        .get_RF()
        .write_fregs(0, fatptr_rf::new(0, 0));
    engine.get_cpu().get_fmem().mem_write_data(0, &[42; 4]);
    engine.get_cpu().get_RF().write_vregs(3, [0; 4]);

    let prog = [
        inst::LD128 { rd: 3, frs: 0 },
        inst::LD128 { rd: 4, frs: 0 },
        inst::LD128 { rd: 5, frs: 0 },
    ];
    engine.get_cpu().get_imem().flash_in(&prog);

    for _cycle in 0..10_000 {
        engine.tick();
    }

    assert_eq!(engine.get_cpu().get_RF().read_vregs(3), [42; 4]);
    assert_eq!(engine.get_cpu().get_RF().read_vregs(4), [42; 4]);
    assert_eq!(engine.get_cpu().get_RF().read_vregs(5), [42; 4]);
}

#[test]
fn dramsim3_wrapper_test() {
    let req = dram_req::new(0, true, true);
    let mut dsim3 = dramsim3_wrapper::new(DSIM3_CFG_PATH, DSIM3_OUT_DIR, 0, 0, 0, 0);
    dsim3.SetPimMode(true);

    if dsim3.WillAcceptTransaction(0, false) {
        let mut req = req;
        req.set_id(dsim3.get_req_id());
        req.set_issue_time(0);
        dsim3.AddTransactionReq(req);
    }

    let timeout = 10_000;
    let mut success = false;

    for _cycle in 0..timeout {
        if !dsim3.ClockTick().is_empty() {
            println!("Successfully committed one req");
            success = true;
            break;
        }
    }

    assert!(success, "dsim3 wrapper failed to response to request");
    println!("dsim3 wrapper success to resposne to request");
}
