use crate::PE::types::inst as pe_inst;
use crate::cpu::pimcpu_types::{fatptr_rf, inst};
use crate::memory::mem_portal::dram_req;
use crate::sim_engine::engine::{Engine, EngineSchedulingMode};
use crate::sim_engine::request_router::pim_cmd;
use crate::sim_engine::request_router_test::{encode_fgo_cmd, encode_pim_cmd};
use crate::sim_engine::sim::{Sim, SimMode, engine_cfg};

impl Sim {
    fn set_mode_for_test(&mut self, sim_mode: SimMode) {
        self.sim_mode = sim_mode;
    }

    fn add_engine_with_scheduling_for_test(
        &mut self,
        cfg: engine_cfg,
        scheduling_mode: EngineSchedulingMode,
    ) {
        match self.engines.entry(cfg) {
            std::collections::hash_map::Entry::Occupied(_) => {
                panic!("Cannot add engine with given cfg: already existed");
            }
            std::collections::hash_map::Entry::Vacant(ent) => {
                let mut engine = match cfg {
                    engine_cfg::CGO { .. } => Engine::new_cgo(),
                    engine_cfg::FGO { .. } => Engine::new_fgo(),
                };
                engine
                    .set_scheduling_mode(scheduling_mode)
                    .expect("test scheduling mode should match engine configuration");
                ent.insert(engine);
            }
        }
    }

    fn cgo_engine_cfg_for_addr_for_test(&mut self, addr: u64) -> engine_cfg {
        let addr_bulk = self.dsim3.global_addr_to_local_components(addr);

        engine_cfg::CGO {
            ch: addr_bulk.channel,
            ra: addr_bulk.rank,
            bg: addr_bulk.bank_group,
            ba: addr_bulk.bank,
        }
    }

    fn addr_maps_to_engine_for_test(&mut self, addr: u64) -> bool {
        self.get_engine_cfg(addr).is_some()
    }

    fn engine_mut_for_test(&mut self, cfg: engine_cfg) -> &mut Engine {
        self.engines
            .get_mut(&cfg)
            .expect("Cannot find engine with given cfg")
    }
}

/*
 * It's hard to test correctness as simulator is more like a timing simulator instead of function
 * simulator.
 * In this case, to verify correctness, log the finish time for requests and if finish time is
 * positive(>1), mark this request as correct
 */

const MAX_ENQUEUE_TICKS: u64 = 10_000;
const MAX_DRAIN_TICKS: u64 = 100_000;
const PIM_PROGRAM_TICKS: u64 = 10_000;

fn enqueue_when_accepted(sim: &mut Sim, addr: u64, is_write: bool) {
    for _ in 0..MAX_ENQUEUE_TICKS {
        if sim.canAccept(addr, is_write) {
            sim.enqueue(addr, is_write);
            return;
        }

        sim.tick();
    }

    panic!("sim did not accept request for addr {addr:#x}, is_write={is_write}");
}

fn drain_until_completions(sim: &mut Sim, expected_count: usize) -> Vec<(dram_req, u64)> {
    let mut completions = Vec::new();

    for finish_time in 1..=MAX_DRAIN_TICKS {
        sim.tick();

        while sim.hasComplete() {
            let req = sim
                .getComplete()
                .expect("hasComplete() returned true without a completion");
            assert!(
                finish_time > 1,
                "request completed at non-positive finish time {finish_time}"
            );
            completions.push((req, finish_time));
        }

        if completions.len() == expected_count {
            return completions;
        }
    }

    panic!(
        "timed out after {MAX_DRAIN_TICKS} ticks: observed {} completions, expected {expected_count}",
        completions.len()
    );
}

struct HostDriverResult {
    completions: Vec<(dram_req, u64)>,
    max_outstanding: usize,
}

#[derive(Clone, Copy)]
struct HostDriverRequest {
    addr: u64,
    payload: [u64; 8],
    is_write: bool,
}

impl HostDriverRequest {
    fn memory(addr: u64, is_write: bool) -> Self {
        Self {
            addr,
            payload: [0; 8],
            is_write,
        }
    }

    fn pe(addr: u64, payload: [u64; 8]) -> Self {
        Self {
            addr,
            payload,
            is_write: true,
        }
    }
}

fn run_host_driver(sim: &mut Sim, requests: &[HostDriverRequest]) -> HostDriverResult {
    let mut submitted = 0;
    let mut completions = Vec::new();
    let mut max_outstanding = 0;

    for cycle in 1..=MAX_DRAIN_TICKS {
        // Model a host that can issue at most one request per cycle and obeys
        // backpressure instead of preloading the simulator with a fixed batch.
        if let Some(request) = requests.get(submitted).copied()
            && sim.canAccept(request.addr, request.is_write)
        {
            sim.enqueue_with_data(request.addr, request.payload, request.is_write);
            submitted += 1;
        }

        max_outstanding = max_outstanding.max(submitted - completions.len());
        sim.tick();

        while sim.hasComplete() {
            completions.push((
                sim.getComplete()
                    .expect("hasComplete() returned true without a completion"),
                cycle,
            ));
        }

        if submitted == requests.len() && completions.len() == requests.len() {
            return HostDriverResult {
                completions,
                max_outstanding,
            };
        }
    }

    panic!(
        "host driver timed out: submitted {submitted}/{}, completed {}/{}",
        requests.len(),
        completions.len(),
        requests.len()
    );
}

fn expected_host_requests(requests: &[HostDriverRequest]) -> Vec<(u64, bool)> {
    requests
        .iter()
        .map(|request| (request.addr, request.is_write))
        .collect()
}

fn assert_completions_carry_issue_times(completions: &[(dram_req, u64)]) {
    for (req, _finish_time) in completions {
        req.get_issue_time()
            .expect("completed request should carry an issue time");
    }
}

fn find_addr_for_new_cgo_cfg(sim: &mut Sim, used_cfgs: &[engine_cfg]) -> (u64, engine_cfg) {
    for addr in (0..(1_u64 << 24)).step_by(64) {
        let cfg = sim.cgo_engine_cfg_for_addr_for_test(addr);

        if !used_cfgs.contains(&cfg) {
            return (addr, cfg);
        }
    }

    panic!("could not find an address that maps to a new engine cfg");
}

fn find_addrs_outside_engine_area(sim: &mut Sim, count: usize) -> Vec<u64> {
    let mut addrs = Vec::new();

    for addr in (0..(1_u64 << 24)).step_by(64) {
        if !sim.addr_maps_to_engine_for_test(addr) {
            addrs.push(addr);

            if addrs.len() == count {
                return addrs;
            }
        }
    }

    panic!("could not find {count} addresses outside configured engine areas");
}

fn find_addrs_inside_engine_area(sim: &mut Sim, count: usize) -> Vec<u64> {
    let mut addrs = Vec::new();

    for addr in (0..(1_u64 << 24)).step_by(64) {
        if sim.addr_maps_to_engine_for_test(addr) {
            addrs.push(addr);

            if addrs.len() == count {
                return addrs;
            }
        }
    }

    panic!("could not find {count} addresses inside configured engine areas");
}

fn assert_completed_requests_match(completions: &[(dram_req, u64)], expected: &[(u64, bool)]) {
    assert_eq!(completions.len(), expected.len());

    let mut observed = completions
        .iter()
        .map(|(req, _finish_time)| (req.get_addr(), !req.is_read(), req.is_pim()))
        .collect::<Vec<_>>();

    for (expected_addr, expected_is_write) in expected {
        let pos = observed
            .iter()
            .position(|(addr, is_write, is_pim)| {
                *addr == *expected_addr && *is_write == *expected_is_write && !*is_pim
            })
            .unwrap_or_else(|| {
                panic!(
                    "missing completion for addr {expected_addr:#x}, is_write={expected_is_write}"
                )
            });
        observed.remove(pos);
    }

    assert!(
        observed.is_empty(),
        "unexpected completions remained: {observed:?}"
    );
}

fn configure_vecadd(engine: &mut Engine, lhs: u32, rhs: u32) {
    let cpu = engine.get_cpu();
    cpu.start();

    cpu.get_agu().insert(0, 0, 16);
    cpu.get_RF().write_fregs(0, fatptr_rf::new(0, 0));
    cpu.get_fmem().mem_write_data(0, &[0; 4]);
    cpu.get_RF().write_vregs(1, [lhs; 4]);
    cpu.get_RF().write_vregs(2, [rhs; 4]);
    cpu.get_RF().write_vregs(3, [0; 4]);
    cpu.get_RF().write_vregs(4, [0; 4]);

    let prog = [
        inst::ADD128 {
            rd: 3,
            rs1: 1,
            rs2: 2,
        },
        inst::ST128 { rs: 3, frd: 0 },
        inst::LD128 { rd: 4, frs: 0 },
    ];
    cpu.get_imem().flash_in(&prog);
}

fn assert_vecadd_result(engine: &mut Engine, expected: u32) {
    let cpu = engine.get_cpu();

    assert_eq!(cpu.get_RF().read_vregs(3), [expected; 4]);
    assert_eq!(cpu.get_RF().read_vregs(4), [expected; 4]);
    assert_eq!(cpu.get_fmem().mem_read_data(0), Some([expected; 4]));
}

fn configure_complex_cgo_program(engine: &mut Engine) {
    let cpu = engine.get_cpu();
    cpu.start();

    cpu.get_agu().insert(0, 0, 16);
    cpu.get_RF().write_fregs(0, fatptr_rf::new(0, 0));
    cpu.get_fmem().mem_write_data(0, &[0; 4]);
    cpu.get_RF().write_vregs(1, [19; 4]);
    cpu.get_RF().write_vregs(2, [23; 4]);

    let prog = [
        inst::ADD128 {
            rd: 3,
            rs1: 1,
            rs2: 2,
        }, // r3 = 42
        inst::MUL128 {
            rd: 4,
            rs1: 3,
            rs2: 2,
        }, // r4 = 966
        inst::SUB128 {
            rd: 5,
            rs1: 4,
            rs2: 1,
        }, // r5 = 947
        inst::AND128 {
            rd: 6,
            rs1: 5,
            rs2: 3,
        }, // r6 = 34
        inst::ST128 { rs: 5, frd: 0 },
        inst::LD128 { rd: 7, frs: 0 },
        inst::SUB128 {
            rd: 7,
            rs1: 7,
            rs2: 6,
        }, // r7 = 913
        inst::ST128 { rs: 7, frd: 0 },
        inst::LD128 { rd: 4, frs: 0 },
    ];
    cpu.get_imem().flash_in(&prog);
}

fn assert_complex_cgo_result(engine: &mut Engine) {
    let cpu = engine.get_cpu();

    assert_eq!(cpu.get_RF().read_vregs(3), [42; 4]);
    assert_eq!(cpu.get_RF().read_vregs(5), [947; 4]);
    assert_eq!(cpu.get_RF().read_vregs(6), [34; 4]);
    assert_eq!(cpu.get_RF().read_vregs(7), [913; 4]);
    assert_eq!(cpu.get_RF().read_vregs(4), [913; 4]);
    assert_eq!(cpu.get_fmem().mem_read_data(0), Some([913; 4]));
}

#[test]
fn sim_engine_cfg_selects_processor_but_not_scheduling_policy() {
    let mut sim = Sim::new();
    let fgo_cfg = engine_cfg::FGO {
        ch: 0,
        ra: 0,
        bg: 0,
        ba: 0,
    };

    sim.add_engines(fgo_cfg);
    sim.set_engine_scheduling_mode(fgo_cfg, EngineSchedulingMode::Host_FGO_share)
        .expect("FGO configuration should construct an FGO engine");
    sim.engine_mut_for_test(fgo_cfg)
        .get_pe()
        .get_Arf()
        .write_sRF(1, 7);

    assert_eq!(
        sim.engine_mut_for_test(fgo_cfg)
            .get_pe()
            .get_Arf()
            .read_sRF(1),
        7
    );
}

fn tick_pim_program(sim: &mut Sim) {
    for _ in 0..PIM_PROGRAM_TICKS {
        sim.tick();
    }
}

#[test]
fn sim_hostonly_noengine() {
    /*
     * This test will create a Sim with no engines inside, and it will keep receive memory traces
     * and handle them as host request
     */
    let mut sim = Sim::new();
    let requests = [(0x0, false), (0x40, true), (0x80, false), (0xc0, true)];

    for (addr, is_write) in requests {
        enqueue_when_accepted(&mut sim, addr, is_write);
    }

    let completions = drain_until_completions(&mut sim, requests.len());
    assert_completions_carry_issue_times(&completions);

    assert_completed_requests_match(&completions, &requests);
}

#[test]
fn sim_pimonly() {
    /*
     * This test will create a Sim with only one engine inside, and run a simple vecadd program
     * No host request will be made
     */
    let mut sim = Sim::new();
    sim.set_mode_for_test(SimMode::Pim);

    let (_engine_addr, cfg) = find_addr_for_new_cgo_cfg(&mut sim, &[]);
    sim.add_engines(cfg);
    sim.set_engine_scheduling_mode(cfg, EngineSchedulingMode::CGO_only)
        .expect("CGO engine should accept CGO-only scheduling");
    configure_vecadd(sim.engine_mut_for_test(cfg), 1, 2);

    tick_pim_program(&mut sim);

    assert!(
        !sim.hasComplete(),
        "PIM-only test should not emit host completions"
    );
    assert_vecadd_result(sim.engine_mut_for_test(cfg), 3);
}

#[test]
fn sim_multithread_pimonly() {
    /*
     * This test will create multiple engines and run vecadd on both of them.
     * No host request will be made
     */
    let mut sim = Sim::new();
    sim.set_mode_for_test(SimMode::Pim);

    let (_first_addr, first_cfg) = find_addr_for_new_cgo_cfg(&mut sim, &[]);
    let (_second_addr, second_cfg) = find_addr_for_new_cgo_cfg(&mut sim, &[first_cfg]);

    sim.add_engines(first_cfg);
    sim.add_engines(second_cfg);
    sim.set_engine_scheduling_mode(first_cfg, EngineSchedulingMode::CGO_only)
        .expect("CGO engine should accept CGO-only scheduling");
    sim.set_engine_scheduling_mode(second_cfg, EngineSchedulingMode::CGO_only)
        .expect("CGO engine should accept CGO-only scheduling");
    configure_vecadd(sim.engine_mut_for_test(first_cfg), 10, 5);
    configure_vecadd(sim.engine_mut_for_test(second_cfg), 20, 7);

    tick_pim_program(&mut sim);

    assert!(
        !sim.hasComplete(),
        "PIM-only test should not emit host completions"
    );
    assert_vecadd_result(sim.engine_mut_for_test(first_cfg), 15);
    assert_vecadd_result(sim.engine_mut_for_test(second_cfg), 27);
}

#[test]
fn sim_pim_host_together() {
    /*
     * This test will create one engine and run vecadd on it.
     * It will also push fake host request into host-only area
     */
    let mut sim = Sim::new();
    sim.set_mode_for_test(SimMode::Pim);

    let (_engine_addr, cfg) = find_addr_for_new_cgo_cfg(&mut sim, &[]);
    sim.add_engines(cfg);
    sim.set_engine_scheduling_mode(cfg, EngineSchedulingMode::CGO_only)
        .expect("CGO engine should accept CGO-only scheduling");
    configure_vecadd(sim.engine_mut_for_test(cfg), 6, 9);

    let host_addrs = find_addrs_outside_engine_area(&mut sim, 2);
    let requests = [(host_addrs[0], false), (host_addrs[1], true)];

    for (addr, is_write) in requests {
        assert!(
            !sim.addr_maps_to_engine_for_test(addr),
            "host-together request addr {addr:#x} should remain outside engine area"
        );
        enqueue_when_accepted(&mut sim, addr, is_write);
    }

    let completions = drain_until_completions(&mut sim, requests.len());
    assert_completions_carry_issue_times(&completions);
    tick_pim_program(&mut sim);
    assert_vecadd_result(sim.engine_mut_for_test(cfg), 15);

    assert_completed_requests_match(&completions, &requests);
}

#[test]
fn sim_pim_host_concurrent() {
    /*
     * This test will create one engine and run vecadd on it.
     * It will also push fake host request into engine's queue
     */
    let mut sim = Sim::new();
    sim.set_mode_for_test(SimMode::Pim);

    let (_engine_addr, cfg) = find_addr_for_new_cgo_cfg(&mut sim, &[]);
    sim.add_engine_with_scheduling_for_test(cfg, EngineSchedulingMode::Host_CGO_share);
    configure_vecadd(sim.engine_mut_for_test(cfg), 11, 13);

    let host_addrs = find_addrs_inside_engine_area(&mut sim, 2);
    let requests = [(host_addrs[0], false), (host_addrs[1], true)];

    for (addr, is_write) in requests {
        assert!(
            sim.addr_maps_to_engine_for_test(addr),
            "concurrent host request addr {addr:#x} should target an engine"
        );
        enqueue_when_accepted(&mut sim, addr, is_write);
    }

    let completions = drain_until_completions(&mut sim, requests.len());
    assert_completions_carry_issue_times(&completions);
    tick_pim_program(&mut sim);
    assert_vecadd_result(sim.engine_mut_for_test(cfg), 24);

    assert_completed_requests_match(&completions, &requests);
}

#[test]
fn sim_routes_encoded_request_to_pe_and_returns_dram_req_completion() {
    let mut sim = Sim::new();
    sim.set_mode_for_test(SimMode::Pim);
    let cfg = engine_cfg::FGO {
        ch: 0,
        ra: 0,
        bg: 0,
        ba: 0,
    };
    sim.add_engines(cfg);
    sim.set_engine_scheduling_mode(cfg, EngineSchedulingMode::Host_FGO_share)
        .unwrap();
    sim.engine_mut_for_test(cfg)
        .get_pe()
        .get_Arf()
        .write_vRF(1, [7; 8]);
    sim.engine_mut_for_test(cfg)
        .get_pe()
        .get_Arf()
        .write_vRF(2, [8; 8]);

    let (addr, payload) = encode_fgo_cmd(pe_inst::ADD128 {
        vRD: 3,
        vRS0: 1,
        vRS1: 2,
    });
    assert!(sim.canAccept(addr, true));
    sim.enqueue_with_data(addr, payload, true);

    let completions = drain_until_completions(&mut sim, 1);
    assert_eq!(completions[0].0.get_addr(), addr);
    assert!(completions[0].0.get_id().is_some());
    assert!(completions[0].0.get_issue_time().is_some());
    assert_eq!(
        sim.engine_mut_for_test(cfg).get_pe().get_Arf().read_vRF(3),
        [15; 8]
    );
}

#[test]
fn sim_broadcasts_encoded_request_to_all_fgo_engines() {
    let mut sim = Sim::new();
    sim.set_mode_for_test(SimMode::Pim);
    let first_cfg = engine_cfg::FGO {
        ch: 0,
        ra: 0,
        bg: 0,
        ba: 0,
    };
    let second_cfg = engine_cfg::FGO {
        ch: 0,
        ra: 0,
        bg: 0,
        ba: 1,
    };
    let cgo_cfg = engine_cfg::CGO {
        ch: 0,
        ra: 0,
        bg: 0,
        ba: 2,
    };

    sim.add_engine_with_scheduling_for_test(first_cfg, EngineSchedulingMode::Host_FGO_share);
    sim.add_engine_with_scheduling_for_test(second_cfg, EngineSchedulingMode::Host_FGO_share);
    sim.add_engine_with_scheduling_for_test(cgo_cfg, EngineSchedulingMode::Host_CGO_share);

    for cfg in [first_cfg, second_cfg] {
        let pe = sim.engine_mut_for_test(cfg).get_pe();
        pe.get_Arf().write_vRF(1, [2; 8]);
        pe.get_Arf().write_vRF(2, [5; 8]);
    }

    let (addr, payload) = encode_fgo_cmd(pe_inst::ADD128 {
        vRD: 3,
        vRS0: 1,
        vRS1: 2,
    });
    assert!(sim.canAccept(addr, true));
    sim.enqueue_with_data(addr, payload, true);

    let completions = drain_until_completions(&mut sim, 2);
    assert_eq!(completions.len(), 2);
    assert!(completions.iter().all(|(req, _)| req.get_addr() == addr));
    assert_eq!(
        sim.engine_mut_for_test(first_cfg)
            .get_pe()
            .get_Arf()
            .read_vRF(3),
        [7; 8]
    );
    assert_eq!(
        sim.engine_mut_for_test(second_cfg)
            .get_pe()
            .get_Arf()
            .read_vRF(3),
        [7; 8]
    );
}

#[test]
fn sim_broadcasts_cgo_commands_only_to_cgo_engines() {
    let mut sim = Sim::new();
    sim.set_mode_for_test(SimMode::Pim);
    let first_cgo = engine_cfg::CGO {
        ch: 0,
        ra: 0,
        bg: 0,
        ba: 0,
    };
    let second_cgo = engine_cfg::CGO {
        ch: 0,
        ra: 0,
        bg: 0,
        ba: 1,
    };
    let fgo_cfg = engine_cfg::FGO {
        ch: 0,
        ra: 0,
        bg: 0,
        ba: 2,
    };
    sim.add_engine_with_scheduling_for_test(first_cgo, EngineSchedulingMode::CGO_only);
    sim.add_engine_with_scheduling_for_test(second_cgo, EngineSchedulingMode::CGO_only);
    sim.add_engine_with_scheduling_for_test(fgo_cfg, EngineSchedulingMode::Host_FGO_share);

    let (start_addr, start_payload) = encode_pim_cmd(pim_cmd::CgoStart);
    assert!(sim.canAccept(start_addr, true));
    assert!(!sim.canAccept(start_addr, false));
    sim.enqueue_with_data(start_addr, start_payload, true);

    sim.tick();
    let mut completions = Vec::new();
    while sim.hasComplete() {
        completions.push(sim.getComplete().unwrap());
    }
    assert_eq!(completions.len(), 2);
    assert!(
        completions
            .iter()
            .all(|req| req.get_addr() == start_addr && !req.is_read())
    );
    assert!(sim.engine_mut_for_test(first_cgo).get_cpu().is_started());
    assert!(sim.engine_mut_for_test(second_cgo).get_cpu().is_started());

    let (query_addr, query_payload) = encode_pim_cmd(pim_cmd::CgoQuery);
    assert!(sim.canAccept(query_addr, false));
    assert!(!sim.canAccept(query_addr, true));
    sim.enqueue_with_data(query_addr, query_payload, false);

    sim.tick();
    let mut completions = Vec::new();
    while sim.hasComplete() {
        completions.push(sim.getComplete().unwrap());
    }
    assert_eq!(completions.len(), 2);
    assert!(
        completions
            .iter()
            .all(|req| req.get_addr() == query_addr && req.is_read() && req.get_payload()[0] == 0)
    );
}

#[test]
fn sim_rejects_encoded_request_without_fgo_engine() {
    let mut sim = Sim::new();
    sim.set_mode_for_test(SimMode::Pim);
    let (addr, _) = encode_fgo_cmd(pe_inst::NOP);

    assert!(!sim.canAccept(addr, true));
}

#[test]
fn sim_CGO_host_together() {
    let mut sim = Sim::new();
    sim.set_mode_for_test(SimMode::Pim);

    let (_engine_addr, cfg) = find_addr_for_new_cgo_cfg(&mut sim, &[]);
    sim.add_engines(cfg);
    sim.set_engine_scheduling_mode(cfg, EngineSchedulingMode::Host_CGO_share)
        .unwrap();
    configure_complex_cgo_program(sim.engine_mut_for_test(cfg));

    let inside = find_addrs_inside_engine_area(&mut sim, 12);
    let outside = find_addrs_outside_engine_area(&mut sim, 12);
    let mut requests = Vec::with_capacity(24);
    for idx in 0..12 {
        requests.push(HostDriverRequest::memory(inside[idx], idx % 2 == 0));
        requests.push(HostDriverRequest::memory(outside[idx], idx % 3 == 0));
    }

    let result = run_host_driver(&mut sim, &requests);

    assert!(
        result.max_outstanding > 1,
        "host driver never had concurrent outstanding requests"
    );
    assert_completions_carry_issue_times(&result.completions);
    assert_completed_requests_match(&result.completions, &expected_host_requests(&requests));

    tick_pim_program(&mut sim);
    let engine = sim.engine_mut_for_test(cfg);
    assert_complex_cgo_result(engine);
    assert!(engine.scheduler_was_invoked_for_test());
    assert!(engine.scheduler_entered_host_for_test());
    assert!(engine.scheduler_entered_pim_for_test());
}

#[test]
fn sim_general_test() {
    let mut sim = Sim::new();
    assert!(sim.burst_size() != 0, "sim burst size is zero");
}

#[test]
fn sim_FGO_host_together() {
    let mut sim = Sim::new();
    sim.set_mode_for_test(SimMode::Pim);
    let cfg = engine_cfg::FGO {
        ch: 0,
        ra: 0,
        bg: 0,
        ba: 0,
    };
    sim.add_engines(cfg);
    sim.set_engine_scheduling_mode(cfg, EngineSchedulingMode::Host_FGO_share)
        .unwrap();

    {
        let pe = sim.engine_mut_for_test(cfg).get_pe();
        pe.get_Arf().write_vRF(1, [20; 8]);
        pe.get_Arf().write_vRF(2, [3; 8]);
        pe.get_Arf().write_vRF(5, [100; 8]);
        pe.get_Arf().write_vRF(6, [7; 8]);
    }

    let pe_instructions = [
        pe_inst::ADD128 {
            vRD: 3,
            vRS0: 1,
            vRS1: 2,
        },
        pe_inst::SUB128 {
            vRD: 4,
            vRS0: 1,
            vRS1: 2,
        },
        pe_inst::ADD128 {
            vRD: 7,
            vRS0: 5,
            vRS1: 6,
        },
        pe_inst::SUB128 {
            vRD: 8,
            vRS0: 5,
            vRS1: 6,
        },
        pe_inst::ADD128 {
            vRD: 9,
            vRS0: 1,
            vRS1: 6,
        },
        pe_inst::SUB128 {
            vRD: 10,
            vRS0: 5,
            vRS1: 2,
        },
    ];
    let inside = find_addrs_inside_engine_area(&mut sim, pe_instructions.len());
    let outside = find_addrs_outside_engine_area(&mut sim, pe_instructions.len());
    let mut requests = Vec::with_capacity(pe_instructions.len() * 3);

    for (idx, instruction) in pe_instructions.into_iter().enumerate() {
        let (addr, payload) = encode_fgo_cmd(instruction);
        requests.push(HostDriverRequest::pe(addr, payload));
        requests.push(HostDriverRequest::memory(inside[idx], idx % 2 == 0));
        requests.push(HostDriverRequest::memory(outside[idx], idx % 2 != 0));
    }

    let result = run_host_driver(&mut sim, &requests);

    assert!(
        result.max_outstanding > 1,
        "host driver never had concurrent outstanding requests"
    );
    assert_completions_carry_issue_times(&result.completions);
    assert_completed_requests_match(&result.completions, &expected_host_requests(&requests));

    let engine = sim.engine_mut_for_test(cfg);
    assert!(engine.scheduler_was_invoked_for_test());
    assert!(engine.scheduler_entered_host_for_test());
    assert!(engine.scheduler_entered_pim_for_test());

    let pe = engine.get_pe();
    assert_eq!(pe.get_Arf().read_vRF(3), [23; 8]);
    assert_eq!(pe.get_Arf().read_vRF(4), [17; 8]);
    assert_eq!(pe.get_Arf().read_vRF(7), [107; 8]);
    assert_eq!(pe.get_Arf().read_vRF(8), [93; 8]);
    assert_eq!(pe.get_Arf().read_vRF(9), [27; 8]);
    assert_eq!(pe.get_Arf().read_vRF(10), [97; 8]);
}
