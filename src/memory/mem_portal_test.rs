use crate::memory::mem_portal::*;

#[test]
fn take_completed_preserves_expected_response_matching() {
    let mut portal = dram_portal::new();
    let expected_pim = dram_req::new(0x10, true, true);
    let other_pim = dram_req::new(0x20, true, true);
    let same_addr_host = dram_req::new(0x10, true, false);

    portal.complete(other_pim.clone());
    portal.complete(expected_pim.clone());
    portal.complete(same_addr_host.clone());

    let completed = portal
        .take_completed(&expected_pim)
        .expect("expected matching PIM response");
    assert_eq!(completed.get_addr(), expected_pim.get_addr());
    assert_eq!(completed.is_read(), expected_pim.is_read());
    assert_eq!(completed.is_pim(), expected_pim.is_pim());

    assert!(portal.take_completed(&expected_pim).is_none());
    assert!(portal.take_completed(&other_pim).is_some());
    assert!(portal.take_completed(&same_addr_host).is_some());
}
