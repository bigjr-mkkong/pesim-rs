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
fn boot_controller_is_guarded_by_processor_kind() {
    assert!(Engine::new_cgo().cgo_boot.is_some());
    assert!(Engine::new_fgo().cgo_boot.is_none());
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
use crate::sim_engine::engine_alloc::{PSEUDO_BANK_CACHELINES, PSEUDO_BANK_ENTRIES};
use crate::sim_engine::request_router::{decode_pim_cmd, pim_cmd};
use crate::sim_engine::request_router_test::{encode_fgo_cmd, encode_pim_cmd};

fn engine_request(addr: u64, is_write: bool) -> EngineRequest {
    EngineRequest {
        addr,
        is_write,
        decoded_cmd: decode_pim_cmd(addr, &[0; 8]),
    }
}

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

    engine.enqueue_host_mem_request(dram_req::new(0x40, true, false));
    engine.enqueue_host_mem_request(dram_req::new(0x80, true, false));

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
    engine.enqueue_host_pim_request(
        dram_req::new_with_payload(addr, payload, false, false),
        pim_cmd::FGO(pe_inst::ADD128 {
            vRD: 3,
            vRS0: 1,
            vRS1: 2,
        }),
    );

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
    engine.enqueue_host_pim_request(
        dram_req::new_with_payload(addr, payload, false, false),
        pim_cmd::FGO(pe_inst::NOP),
    );

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
    assert!(!engine.canAccept(engine_request(addr, true)));
}

#[test]
fn fgo_rejects_cgo_commands_and_cgo_query_is_read_only() {
    let mut fgo = Engine::new_fgo();
    let (query_addr, _) = encode_pim_cmd(pim_cmd::CGO_Query);
    let (start_addr, _) = encode_pim_cmd(pim_cmd::CGO_Start);
    assert!(!fgo.canAccept(engine_request(query_addr, false)));
    assert!(!fgo.canAccept(engine_request(start_addr, true)));

    let mut cgo = Engine::new_cgo();
    assert!(cgo.canAccept(engine_request(query_addr, false)));
    assert!(!cgo.canAccept(engine_request(query_addr, true)));
    assert!(cgo.canAccept(engine_request(start_addr, true)));
    assert!(!cgo.canAccept(engine_request(start_addr, false)));
}

#[test]
fn engine_admission_uses_the_supplied_decode_result() {
    let mut engine = Engine::new_cgo();
    let request = EngineRequest {
        addr: 0x40,
        is_write: false,
        decoded_cmd: Ok(Some(pim_cmd::CGO_Query)),
    };

    assert!(engine.canAccept(request));
}

#[test]
fn cgo_start_gates_cpu_execution_and_query_reports_finished() {
    let mut engine = Engine::new_cgo();
    engine
        .set_scheduling_mode(EngineSchedulingMode::CGO_only)
        .unwrap();
    engine.get_cpu().get_RF().write_vregs(1, [3; 4]);
    engine.get_cpu().get_RF().write_vregs(2, [4; 4]);
    engine
        .get_cpu()
        .get_fmem()
        .mem_write_data(0, &[8, 1, 0, 0])
        .unwrap();
    for chunk in 1..8 {
        engine
            .get_cpu()
            .get_fmem()
            .mem_write_data(chunk, &[0; 4])
            .unwrap();
    }
    let add = (0x1_u32 << 12) | (3 << 9) | (1 << 6) | (2 << 3);
    let equal_exit = (0xc_u32 << 12) | (3 << 9) | (3 << 6);
    engine
        .get_cpu()
        .get_fmem()
        .mem_write_data(8, &[add | (equal_exit << 16), 0, 0, 0])
        .unwrap();

    for _ in 0..8 {
        engine.tick();
    }
    assert_eq!(engine.get_cpu().get_RF().read_vregs(3), [0; 4]);

    let (query_addr, query_payload) = encode_pim_cmd(pim_cmd::CGO_Query);
    engine.enqueue_host_pim_request(
        dram_req::new_with_payload(query_addr, query_payload, true, false),
        pim_cmd::CGO_Query,
    );
    engine.tick();
    let before = engine
        .get_host_complete()
        .expect("CGO query should complete on the next tick");
    assert_eq!(before.get_payload()[0], 0);

    let (start_addr, start_payload) = encode_pim_cmd(pim_cmd::CGO_Start);
    engine.enqueue_host_pim_request(
        dram_req::new_with_payload(start_addr, start_payload, false, false),
        pim_cmd::CGO_Start,
    );
    engine.tick();
    assert_eq!(
        engine
            .get_host_complete()
            .expect("CGO start should complete on the next tick")
            .get_addr(),
        start_addr
    );
    assert!(!engine.get_cpu().is_started());

    let mut boot_ticks = 0;
    for tick in 1..=10_000 {
        engine.tick();
        assert_eq!(engine.get_cpu().get_RF().read_vregs(3), [0; 4]);
        if engine.get_cpu().is_started() {
            boot_ticks = tick;
            break;
        }
    }
    assert!(
        boot_ticks > 1,
        "boot must not complete in the start-command tick"
    );
    assert!(matches!(
        engine.get_cpu().get_imem().read_inst(0),
        Some(inst::ADD128 {
            rd: 3,
            rs1: 1,
            rs2: 2
        })
    ));
    assert!(matches!(
        engine.get_cpu().get_imem().read_inst(1),
        Some(inst::EqualExit { rd: 3, rs1: 3 })
    ));

    for _ in 0..10_000 {
        engine.tick();
        if engine.get_cpu().get_RF().read_vregs(3) == [7; 4] && engine.get_cpu().is_finished() {
            break;
        }
    }
    assert_eq!(engine.get_cpu().get_RF().read_vregs(3), [7; 4]);
    assert!(engine.get_cpu().is_finished());

    engine.enqueue_host_pim_request(
        dram_req::new_with_payload(query_addr, query_payload, true, false),
        pim_cmd::CGO_Query,
    );
    engine.tick();
    let after = engine
        .get_host_complete()
        .expect("CGO query should complete on the next tick");
    assert_eq!(after.get_payload()[0], 1);
}

#[test]
fn first_FGO_command_closes_host_initialization_mirroring() {
    let mut engine = Engine::new_fgo();
    let mut initial_payload = [0; 8];
    initial_payload[0] = u64::from_le_bytes([1, 0, 2, 0, 3, 0, 4, 0]);
    initial_payload[1] = u64::from_le_bytes([5, 0, 6, 0, 7, 0, 8, 0]);
    engine.mirror_host_write(64, &initial_payload);
    assert_eq!(
        engine.get_pe().get_fmem().mem_read_v(4),
        Some([1, 2, 3, 4, 5, 6, 7, 8])
    );

    let (cmd_addr, cmd_payload) = encode_fgo_cmd(pe_inst::NOP);
    engine.enqueue_host_pim_request(
        dram_req::new_with_payload(cmd_addr, cmd_payload, false, false),
        pim_cmd::FGO(pe_inst::NOP),
    );

    let mut later_payload = [0; 8];
    later_payload[0] = u64::from_le_bytes([8, 0, 7, 0, 6, 0, 5, 0]);
    later_payload[1] = u64::from_le_bytes([4, 0, 3, 0, 2, 0, 1, 0]);
    engine.mirror_host_write(64, &later_payload);

    assert_eq!(
        engine.get_pe().get_fmem().mem_read_v(4),
        Some([1, 2, 3, 4, 5, 6, 7, 8])
    );
}

#[test]
fn pseudo_bank_base_is_added_to_pim_timing_addresses() {
    let pseudo_bank = 3;
    let mut engine = Engine::new_fgo_at(0, 0, 0, 2, pseudo_bank);

    let global_addr = engine.dsim3.request_addr_to_dram_addr_for_test(0, true);
    let local_addr = engine.dsim3.global_addr_to_local_components(global_addr);

    assert_eq!(local_addr.channel, 0);
    assert_eq!(local_addr.rank, 0);
    assert_eq!(local_addr.bank_group, 0);
    assert_eq!(local_addr.bank, 2);
    assert_eq!(
        local_addr.bank_local_addr,
        pseudo_bank * PSEUDO_BANK_CACHELINES
    );
}

#[test]
#[should_panic(expected = "PIM address is outside its pseudo bank")]
fn fgo_memory_address_outside_pseudo_bank_is_fatal() {
    let mut engine = Engine::new_fgo();
    let instruction = pe_inst::LD128 {
        vRD: 1,
        addr: PSEUDO_BANK_ENTRIES as u32,
    };
    let (addr, payload) = encode_fgo_cmd(instruction);

    engine.enqueue_host_pim_request(
        dram_req::new_with_payload(addr, payload, false, false),
        pim_cmd::FGO(instruction),
    );
}
