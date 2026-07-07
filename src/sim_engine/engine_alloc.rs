use crate::sim_engine::sim::engine_cfg;
use std::collections::HashMap;
use std::ops::Range;

pub const LOGICAL_BANK_SZ: u64 = 0x4000_0000;
pub const PHY_BANK_SZ: u64 = 0x0400_0000;
pub const CACHELINE_SZ: u64 = 64;
pub const PSEUDO_BANKS_PER_LOGICAL_BANK: u64 = LOGICAL_BANK_SZ / PHY_BANK_SZ;
pub const PSEUDO_BANK_CACHELINES: u64 = PHY_BANK_SZ / CACHELINE_SZ;
pub const PSEUDO_BANK_ENTRIES: u64 = PHY_BANK_SZ / 16;

const _: () = assert!(LOGICAL_BANK_SZ % PHY_BANK_SZ == 0);
const _: () = assert!(PHY_BANK_SZ % CACHELINE_SZ == 0);
const _: () = assert!(PHY_BANK_SZ % 16 == 0);

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum EngineAllocKind {
    CGO,
    FGO,
}

#[derive(Clone, Copy)]
struct pseudo_bank_location {
    ch: u64,
    ra: u64,
    bg: u64,
    ba: u64,
    pb: u64,
}

pub struct engine_alloc {
    logical_banks: Vec<(u64, u64, u64, u64)>,
    pseudo_banks: Vec<pseudo_bank_location>,
    winner: Option<(u64, EngineAllocKind)>,
    table: HashMap<(u64, EngineAllocKind), Vec<engine_cfg>>,
}

impl engine_alloc {
    pub fn new(
        channels: Range<u64>,
        ranks: Range<u64>,
        bank_groups: Range<u64>,
        banks: Range<u64>,
    ) -> Self {
        let mut logical_banks = Vec::new();
        let mut pseudo_banks = Vec::new();
        for ch in channels {
            for ra in ranks.clone() {
                for bg in bank_groups.clone() {
                    for ba in banks.clone() {
                        logical_banks.push((ch, ra, bg, ba));
                        for pb in 0..PSEUDO_BANKS_PER_LOGICAL_BANK {
                            pseudo_banks.push(pseudo_bank_location { ch, ra, bg, ba, pb });
                        }
                    }
                }
            }
        }

        Self {
            logical_banks,
            pseudo_banks,
            winner: None,
            table: HashMap::new(),
        }
    }

    pub fn alloc_cgo(&mut self, asid: u64) -> Vec<engine_cfg> {
        self.alloc(asid, EngineAllocKind::CGO, |ch, ra, bg, ba, pb| {
            engine_cfg::CGO { ch, ra, bg, ba, pb }
        })
    }

    pub fn alloc_fgo(&mut self, asid: u64) -> Vec<engine_cfg> {
        self.alloc(asid, EngineAllocKind::FGO, |ch, ra, bg, ba, pb| {
            engine_cfg::FGO { ch, ra, bg, ba, pb }
        })
    }

    pub fn logical_banks(&self) -> Vec<(u64, u64, u64, u64)> {
        self.logical_banks.clone()
    }

    fn alloc(
        &mut self,
        asid: u64,
        kind: EngineAllocKind,
        make_cfg: impl Fn(u64, u64, u64, u64, u64) -> engine_cfg,
    ) -> Vec<engine_cfg> {
        let key = (asid, kind);

        if let Some(existing) = self.table.get(&key) {
            return existing.clone();
        }

        match self.winner {
            Some(winner) if winner != key => {
                self.table.insert(key, Vec::new());
                return Vec::new();
            }
            None => self.winner = Some(key),
            Some(_) => {}
        }

        let allocated = self
            .pseudo_banks
            .iter()
            .map(|location| {
                make_cfg(
                    location.ch,
                    location.ra,
                    location.bg,
                    location.ba,
                    location.pb,
                )
            })
            .collect::<Vec<_>>();

        self.table.insert(key, allocated.clone());
        println!("Allocated {} of engines", allocated.len());
        allocated
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allocation_uses_only_configured_coordinate_ranges() {
        let mut allocator = engine_alloc::new(1..3, 2..4, 3..5, 4..6);

        let allocated = allocator.alloc_cgo(1);

        assert_eq!(allocated.len(), 16 * PSEUDO_BANKS_PER_LOGICAL_BANK as usize);
        assert!(allocated.contains(&engine_cfg::CGO {
            ch: 1,
            ra: 2,
            bg: 3,
            ba: 4,
            pb: 0,
        }));
        assert!(allocated.contains(&engine_cfg::CGO {
            ch: 2,
            ra: 3,
            bg: 4,
            ba: 5,
            pb: PSEUDO_BANKS_PER_LOGICAL_BANK - 1,
        }));
        assert!(!allocated.contains(&engine_cfg::CGO {
            ch: 0,
            ra: 2,
            bg: 3,
            ba: 4,
            pb: 0,
        }));
    }

    #[test]
    fn allocation_kind_is_exclusive_even_for_the_same_asid() {
        let mut allocator = engine_alloc::new(0..1, 0..1, 0..1, 1..2);

        assert_eq!(
            allocator.alloc_fgo(7).len(),
            PSEUDO_BANKS_PER_LOGICAL_BANK as usize
        );
        assert!(allocator.alloc_cgo(7).is_empty());
        assert_eq!(
            allocator.alloc_fgo(7).len(),
            PSEUDO_BANKS_PER_LOGICAL_BANK as usize
        );
    }
}
