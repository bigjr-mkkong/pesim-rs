use crate::cpu::pimcpu_types::{fatptr_rf, inst};
use crate::memory::mem_portal::dram_req;
use crate::sim_engine::engine::{Engine, EngineSchedulingMode};
use crate::sim_engine::sim::{Sim, SimMode, engine_cfg};

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
    sim.add_engine_with_scheduling_for_test(cfg, EngineSchedulingMode::ScheduledHostPim);
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
