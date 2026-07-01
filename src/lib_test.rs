use super::*;

#[test]
fn c_abi_drives_request_to_completion() {
    let sim = pesim_new();
    assert!(!sim.is_null());
    assert!(pesim_clock_period(sim) > 0.0);
    assert!(pesim_queue_size(sim) > 0);
    assert!(pesim_burst_size(sim) > 0);

    let addr = MEM_BEGIN + 0x100;
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
    let sim = pesim_new();
    assert!(!sim.is_null());

    let addr = MEM_BEGIN + (1_u64 << 32) + 0x100;
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
    assert!(!pesim_has_complete(null));
    assert_eq!(pesim_get_complete(null), PEsim_rs_MemReq::default());
    pesim_tick(null);
    pesim_print_stats(null);
    pesim_reset_stats(null);
    pesim_free(null);
}
