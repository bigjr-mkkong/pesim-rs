use super::*;
use std::ffi::CString;

const TEST_MEM_BEGIN: u64 = 0x8000_0000;
const TEST_CONTROLLER_SIZE: u64 = 8 * 1024 * 1024 * 1024;

fn new_test_sim() -> *mut PESim_body {
    let config_file = CString::new(FALLBACK_DSIM3_CFG_PATH).unwrap();
    let output_dir = CString::new(DSIM3_OUT_DIR).unwrap();
    let config = PESim_config {
        config_file: config_file.as_ptr(),
        output_dir: output_dir.as_ptr(),
        controller_id: 0,
        controller_base: TEST_MEM_BEGIN,
        controller_size: TEST_CONTROLLER_SIZE,
        pim_size: 0,
    };
    pesim_new(&config)
}

fn new_pim_test_sim() -> *mut PESim_body {
    let config_file = CString::new(PIM_DSIM3_CFG_PATH).unwrap();
    let output_dir = CString::new(DSIM3_OUT_DIR).unwrap();
    let config = PESim_config {
        config_file: config_file.as_ptr(),
        output_dir: output_dir.as_ptr(),
        controller_id: 1,
        controller_base: TEST_MEM_BEGIN,
        controller_size: TEST_CONTROLLER_SIZE,
        pim_size: 64 * 1024 * 1024,
    };
    pesim_new(&config)
}

fn ini_value<'a>(contents: &'a str, key: &str) -> Option<&'a str> {
    contents.lines().find_map(|line| {
        let (candidate, value) = line.split_once('=')?;
        (candidate.trim() == key).then(|| value.trim())
    })
}

#[test]
fn fallback_and_pim_dramsim3_configs_are_distinct_and_address_compatible() {
    assert_ne!(FALLBACK_DSIM3_CFG_PATH, PIM_DSIM3_CFG_PATH);

    let fallback = std::fs::read_to_string(FALLBACK_DSIM3_CFG_PATH)
        .expect("fallback DRAMSim3 configuration should be readable");
    let pim = std::fs::read_to_string(PIM_DSIM3_CFG_PATH)
        .expect("PIM DRAMSim3 configuration should be readable");

    assert_eq!(
        ini_value(&fallback, "address_mapping"),
        ini_value(&pim, "address_mapping"),
        "fallback and PIM timing models must decode the same physical addresses"
    );
}

#[test]
fn c_abi_drives_request_to_completion() {
    let sim = new_test_sim();
    assert!(!sim.is_null());
    assert!(pesim_clock_period(sim) > 0.0);
    assert!(pesim_queue_size(sim) > 0);
    assert!(pesim_burst_size(sim) > 0);

    let addr = TEST_MEM_BEGIN + 0x100;
    let payload = PESim_payload {
        dword_payload: [0xdead_beef; 8],
        payload_sz_bytes: 64,
    };
    assert!(pesim_canAccept(sim, addr, true));
    assert!(pesim_enqueue_with_data(sim, addr, payload, true));

    for _ in 0..100_000 {
        pesim_tick(sim);
        if pesim_has_complete(sim) {
            let completed = pesim_get_complete(sim);
            assert_eq!(completed.addr, addr);
            assert!(completed.is_write);
            pesim_reset_stats(sim);
            pesim_free(sim);
            return;
        }
    }

    pesim_free(sim);
    panic!("C ABI request did not complete");
}

#[test]
fn c_abi_round_trips_address_above_shifted_four_gib_range() {
    let sim = new_test_sim();
    assert!(!sim.is_null());

    let addr = TEST_MEM_BEGIN + (1_u64 << 32) + 0x100;
    let payload = PESim_payload {
        dword_payload: [0xdead_beef; 8],
        payload_sz_bytes: 4,
    };
    assert!(pesim_canAccept(sim, addr, true));
    assert!(pesim_enqueue_with_data(sim, addr, payload, true));

    for _ in 0..100_000 {
        pesim_tick(sim);
        if pesim_has_complete(sim) {
            let completed = pesim_get_complete(sim);
            assert_eq!(completed.addr, addr);
            assert!(completed.is_write);
            pesim_free(sim);
            return;
        }
    }

    pesim_free(sim);
    panic!("C ABI request above the shifted 4 GiB range did not complete");
}

#[test]
fn c_abi_null_pointer_calls_are_safe() {
    let null = std::ptr::null_mut();
    assert!(!pesim_canAccept(null, 0, false));
    assert!(!pesim_can_accept_pim_cmd(
        null,
        0,
        PESim_payload::default(),
        true
    ));
    assert!(!pesim_enqueue_pim_cmd(
        null,
        0,
        PESim_payload::default(),
        true
    ));
    assert!(!pesim_has_complete(null));
    assert_eq!(pesim_get_complete(null), PEsim_rs_MemReq::default());
    pesim_tick(null);
    pesim_print_stats(null);
    pesim_reset_stats(null);
    pesim_free(null);
}

#[test]
fn c_abi_direct_pim_commands_are_fire_and_forget() {
    const FGO_ALLOC_OFFSET: u64 = 13 * 64;
    const FGO_NOP_OFFSET: u64 = 0;

    let sim = new_pim_test_sim();
    assert!(!sim.is_null());

    let alloc = PESim_payload {
        dword_payload: [0x114514; 8],
        payload_sz_bytes: 8,
    };
    assert!(pesim_can_accept_pim_cmd(sim, FGO_ALLOC_OFFSET, alloc, true));
    assert!(pesim_enqueue_pim_cmd(sim, FGO_ALLOC_OFFSET, alloc, true));

    let nop = PESim_payload {
        dword_payload: [0; 8],
        payload_sz_bytes: 8,
    };
    assert!(pesim_can_accept_pim_cmd(sim, FGO_NOP_OFFSET, nop, true));
    assert!(pesim_enqueue_pim_cmd(sim, FGO_NOP_OFFSET, nop, true));

    for _ in 0..1_000 {
        pesim_tick(sim);
    }
    assert!(!pesim_has_complete(sim));
    pesim_free(sim);
}

#[test]
fn c_abi_pim_query_returns_packed_progress_on_the_next_tick() {
    const FGO_ALLOC_OFFSET: u64 = 13 * 64;
    const FGO_NOP_OFFSET: u64 = 0;
    const PIM_QUERY_OFFSET: u64 = 11 * 64;

    let sim = new_pim_test_sim();
    assert!(!sim.is_null());

    let alloc = PESim_payload {
        dword_payload: [0x114514; 8],
        payload_sz_bytes: 8,
    };
    assert!(pesim_enqueue_pim_cmd(sim, FGO_ALLOC_OFFSET, alloc, true));

    let command = PESim_payload {
        dword_payload: [0; 8],
        payload_sz_bytes: 8,
    };
    assert!(pesim_enqueue_pim_cmd(sim, FGO_NOP_OFFSET, command, true));
    for _ in 0..32 {
        pesim_tick(sim);
    }
    assert!(!pesim_has_complete(sim));

    assert!(pesim_can_accept_pim_cmd(
        sim,
        PIM_QUERY_OFFSET,
        command,
        false
    ));
    assert!(pesim_enqueue_pim_cmd(sim, PIM_QUERY_OFFSET, command, false));
    assert!(!pesim_has_complete(sim));

    pesim_tick(sim);
    assert!(pesim_has_complete(sim));
    let completed = pesim_get_complete(sim);
    assert_eq!(completed.addr, PIM_QUERY_OFFSET);
    assert_eq!(completed.payload_word0, (1_u64 << 32) | 1);
    assert!(!completed.is_write);
    assert!(completed.is_pim_query);
    assert!(!pesim_has_complete(sim));

    pesim_free(sim);
}
