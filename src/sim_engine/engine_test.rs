use super::*;
use crate::DSIM3_CFG_PATH;
use crate::DSIM3_OUT_DIR;
use crate::cpu::pimcpu_types::{fatptr_rf, inst};
use crate::memory::dramsim3_wrapper::dramsim3_wrapper;
use crate::memory::mem_portal::dram_req;
use crate::sim_engine::engine::Engine;
use std::sync::atomic::Ordering;

impl Engine {
    pub(crate) fn scheduler_was_invoked_for_test(&self) -> bool {
        self.scheduler_probe.load(Ordering::Relaxed) & SCHED_PROBE_INVOKED != 0
    }

    pub(crate) fn scheduler_entered_host_for_test(&self) -> bool {
        self.scheduler_probe.load(Ordering::Relaxed) & SCHED_PROBE_ENTERED_HOST != 0
    }

    pub(crate) fn scheduler_entered_pim_for_test(&self) -> bool {
        self.scheduler_probe.load(Ordering::Relaxed) & SCHED_PROBE_ENTERED_PIM != 0
    }
}

#[test]
fn engine_runs_pim_load_through_mem_fsm_and_dram_portal() {
    let mut engine = Engine::new_cgo();
    engine
        .set_scheduling_mode(crate::sim_engine::engine::EngineSchedulingMode::Host_CGO_share)
        .expect("CGO engine should accept host/CGO scheduling");

    engine.get_cpu().get_agu().insert(0, 0, 16);
    engine
        .get_cpu()
        .get_RF()
        .write_fregs(0, fatptr_rf::new(0, 0));
    engine.get_cpu().get_fmem().mem_write_data(0, &[42; 4]);
    engine.get_cpu().get_RF().write_vregs(3, [0; 4]);
    engine.get_cpu().start();

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

use crate::PE::types::inst as pe_inst;
use crate::sim_engine::request_router::pim_cmd;
use crate::sim_engine::request_router_test::{encode_fgo_cmd, encode_pim_cmd};

#[test]
fn scheduling_configuration_is_one_time_and_processor_checked() {
    let mut cgo = Engine::new_cgo();
    assert_eq!(
        cgo.set_scheduling_mode(EngineSchedulingMode::Host_FGO_share),
        Err("scheduling mode is incompatible with the engine processor")
    );
    cgo.set_scheduling_mode(EngineSchedulingMode::CGO_only)
        .unwrap();
    assert_eq!(
        cgo.set_scheduling_mode(EngineSchedulingMode::Host_CGO_share),
        Err("engine scheduling mode can only be configured once")
    );

    let mut fgo = Engine::new_fgo();
    assert_eq!(
        fgo.set_scheduling_mode(EngineSchedulingMode::CGO_only),
        Err("scheduling mode is incompatible with the engine processor")
    );
    fgo.set_scheduling_mode(EngineSchedulingMode::Host_FGO_share)
        .unwrap();
}

#[test]
#[should_panic(expected = "cannot tick an engine before configuring")]
fn unconfigured_engine_rejects_tick() {
    Engine::new_cgo().tick();
}

#[test]
fn fgo_switch_delay_counts_complete_cycles_in_both_directions() {
    let mut engine = Engine::new_fgo();
    engine.set_external_signal_delays(2, 3);
    engine
        .set_scheduling_mode(EngineSchedulingMode::Host_FGO_share)
        .unwrap();

    engine.switch(EngineMode::PIM);
    engine.mode = engine.next_mode;
    for _ in 0..2 {
        engine.schedule();
        engine.mode = engine.next_mode;
        assert_eq!(engine.mode, EngineMode::switch_delay);
    }
    engine.schedule();
    engine.mode = engine.next_mode;
    assert_eq!(engine.mode, EngineMode::HOST);

    engine.switch(EngineMode::HOST);
    engine.mode = engine.next_mode;
    for _ in 0..3 {
        engine.schedule();
        engine.mode = engine.next_mode;
        assert_eq!(engine.mode, EngineMode::switch_delay);
    }
    engine.schedule();
    engine.mode = engine.next_mode;
    assert_eq!(engine.mode, EngineMode::PIM);
}

#[test]
fn fgo_round_robin_completes_one_pe_and_fifo_host_request_at_a_time() {
    let mut engine = Engine::new_fgo();
    engine
        .set_scheduling_mode(EngineSchedulingMode::Host_FGO_share)
        .unwrap();

    {
        let pe = engine.get_pe();
        pe.get_Arf().write_vRF(1, [4; 8]);
        pe.get_Arf().write_vRF(2, [5; 8]);
        pe.get_Arf().write_vRF(4, [20; 8]);
        pe.get_Arf().write_vRF(5, [3; 8]);
        pe.push_host_inst(pe_inst::ADD128 {
            vRD: 3,
            vRS0: 1,
            vRS1: 2,
        });
        pe.push_host_inst(pe_inst::SUB128 {
            vRD: 6,
            vRS0: 4,
            vRS1: 5,
        });
    }

    engine.host_push_req(portal_req::HOST_REQ {
        req: dram_req::new(0x40, true, false),
    });
    engine.host_push_req(portal_req::HOST_REQ {
        req: dram_req::new(0x80, true, false),
    });

    let mut completed_addrs = Vec::new();
    for _ in 0..20_000 {
        engine.tick();
        while let Some(req) = engine.get_host_complete() {
            completed_addrs.push(req.get_addr());
        }

        let pe_done = {
            let pe = engine.get_pe();
            pe.get_Arf().read_vRF(3) == [9; 8] && pe.get_Arf().read_vRF(6) == [17; 8]
        };
        if pe_done && completed_addrs.len() == 2 {
            break;
        }
    }

    assert_eq!(completed_addrs, vec![0x40, 0x80]);
    assert_eq!(engine.get_pe().get_Arf().read_vRF(3), [9; 8]);
    assert_eq!(engine.get_pe().get_Arf().read_vRF(6), [17; 8]);
}

#[test]
fn fgo_waits_for_memory_instruction_completion() {
    let mut engine = Engine::new_fgo();
    engine
        .set_scheduling_mode(EngineSchedulingMode::Host_FGO_share)
        .unwrap();
    {
        let pe = engine.get_pe();
        pe.get_fmem().mem_write_s(0x300, 2468).unwrap();
        pe.push_host_inst(pe_inst::LD32 {
            sRD: 7,
            addr: 0x300,
        });
    }

    for _ in 0..20_000 {
        engine.tick();
        if engine.get_pe().get_Arf().read_sRF(7) == 2468 {
            assert!(!engine.get_pe().has_buffered_inst());
            return;
        }
    }

    panic!("FGO memory instruction did not complete through the engine DRAM path");
}

#[test]
fn fgo_decodes_pe_request_and_returns_original_dram_req() {
    let mut engine = Engine::new_fgo();
    engine
        .set_scheduling_mode(EngineSchedulingMode::Host_FGO_share)
        .unwrap();
    engine.get_pe().get_Arf().write_vRF(1, [4; 8]);
    engine.get_pe().get_Arf().write_vRF(2, [5; 8]);

    let (addr, payload) = encode_fgo_cmd(pe_inst::ADD128 {
        vRD: 3,
        vRS0: 1,
        vRS1: 2,
    });
    engine.host_push_req(portal_req::HOST_REQ {
        req: dram_req::new_with_payload(addr, payload, false, false),
    });

    for _ in 0..16 {
        engine.tick();
        if let Some(completed) = engine.get_host_complete() {
            assert_eq!(completed.get_addr(), addr);
            assert_eq!(completed.get_payload(), &payload);
            assert_eq!(engine.get_pe().get_Arf().read_vRF(3), [9; 8]);
            return;
        }
    }

    panic!("encoded FGO command did not complete");
}

#[test]
fn encoded_nop_is_a_valid_pe_request() {
    let mut engine = Engine::new_fgo();
    engine
        .set_scheduling_mode(EngineSchedulingMode::Host_FGO_share)
        .unwrap();
    let (addr, payload) = encode_fgo_cmd(pe_inst::NOP);
    engine.host_push_req(portal_req::HOST_REQ {
        req: dram_req::new_with_payload(addr, payload, false, false),
    });

    for _ in 0..16 {
        engine.tick();
        if let Some(completed) = engine.get_host_complete() {
            assert_eq!(completed.get_addr(), addr);
            return;
        }
    }

    panic!("encoded PE NOP did not complete");
}

#[test]
fn cgo_rejects_encoded_pe_request() {
    let mut engine = Engine::new_cgo();
    let (addr, _) = encode_fgo_cmd(pe_inst::NOP);
    assert!(!engine.canAccept(addr, true));
}

#[test]
fn fgo_rejects_cgo_commands_and_cgo_query_is_read_only() {
    let mut fgo = Engine::new_fgo();
    let (query_addr, _) = encode_pim_cmd(pim_cmd::CgoQuery);
    let (start_addr, _) = encode_pim_cmd(pim_cmd::CgoStart);
    assert!(!fgo.canAccept(query_addr, false));
    assert!(!fgo.canAccept(start_addr, true));

    let mut cgo = Engine::new_cgo();
    assert!(cgo.canAccept(query_addr, false));
    assert!(!cgo.canAccept(query_addr, true));
    assert!(cgo.canAccept(start_addr, true));
    assert!(!cgo.canAccept(start_addr, false));
}

#[test]
fn cgo_start_gates_cpu_execution_and_query_reports_finished() {
    let mut engine = Engine::new_cgo();
    engine
        .set_scheduling_mode(EngineSchedulingMode::CGO_only)
        .unwrap();
    engine.get_cpu().get_RF().write_vregs(1, [3; 4]);
    engine.get_cpu().get_RF().write_vregs(2, [4; 4]);
    engine.get_cpu().get_imem().flash_in(&[
        inst::ADD128 {
            rd: 3,
            rs1: 1,
            rs2: 2,
        },
        inst::EqualExit { rd: 3, rs1: 3 },
    ]);

    for _ in 0..8 {
        engine.tick();
    }
    assert_eq!(engine.get_cpu().get_RF().read_vregs(3), [0; 4]);

    let (query_addr, query_payload) = encode_pim_cmd(pim_cmd::CgoQuery);
    engine.host_push_req(portal_req::HOST_REQ {
        req: dram_req::new_with_payload(query_addr, query_payload, true, false),
    });
    engine.tick();
    let before = engine
        .get_host_complete()
        .expect("CGO query should complete on the next tick");
    assert_eq!(before.get_payload()[0], 0);

    let (start_addr, start_payload) = encode_pim_cmd(pim_cmd::CgoStart);
    engine.host_push_req(portal_req::HOST_REQ {
        req: dram_req::new_with_payload(start_addr, start_payload, false, false),
    });
    engine.tick();
    assert_eq!(
        engine
            .get_host_complete()
            .expect("CGO start should complete on the next tick")
            .get_addr(),
        start_addr
    );

    for _ in 0..10_000 {
        engine.tick();
        if engine.get_cpu().get_RF().read_vregs(3) == [7; 4] && engine.get_cpu().is_finished() {
            break;
        }
    }
    assert_eq!(engine.get_cpu().get_RF().read_vregs(3), [7; 4]);
    assert!(engine.get_cpu().is_finished());

    engine.host_push_req(portal_req::HOST_REQ {
        req: dram_req::new_with_payload(query_addr, query_payload, true, false),
    });
    engine.tick();
    let after = engine
        .get_host_complete()
        .expect("CGO query should complete on the next tick");
    assert_eq!(after.get_payload()[0], 1);
}
