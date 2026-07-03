use crate::sim_engine::sim::engine_cfg;
use std::collections::HashMap;
use std::ops::Range;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum EngineAllocKind {
    CGO,
    FGO,
}

pub struct engine_alloc {
    channels: Range<u64>,
    ranks: Range<u64>,
    bank_groups: Range<u64>,
    banks: Range<u64>,
    winner: Option<u64>,
    table: HashMap<(u64, EngineAllocKind), Vec<engine_cfg>>,
}

impl engine_alloc {
    pub fn new(
        channels: Range<u64>,
        ranks: Range<u64>,
        bank_groups: Range<u64>,
        banks: Range<u64>,
    ) -> Self {
        Self {
            channels,
            ranks,
            bank_groups,
            banks,
            winner: None,
            table: HashMap::new(),
        }
    }

    pub fn alloc_cgo(&mut self, asid: u64) -> Vec<engine_cfg> {
        self.alloc(asid, EngineAllocKind::CGO, |ch, ra, bg, ba| {
            engine_cfg::CGO { ch, ra, bg, ba }
        })
    }

    pub fn alloc_fgo(&mut self, asid: u64) -> Vec<engine_cfg> {
        self.alloc(asid, EngineAllocKind::FGO, |ch, ra, bg, ba| {
            engine_cfg::FGO { ch, ra, bg, ba }
        })
    }

    fn alloc(
        &mut self,
        asid: u64,
        kind: EngineAllocKind,
        make_cfg: impl Fn(u64, u64, u64, u64) -> engine_cfg,
    ) -> Vec<engine_cfg> {
        let key = (asid, kind);

        if let Some(existing) = self.table.get(&key) {
            return existing.clone();
        }

        match self.winner {
            Some(winner) if winner != asid => {
                self.table.insert(key, Vec::new());
                return Vec::new();
            }
            None => self.winner = Some(asid),
            Some(_) => {}
        }

        let mut allocated = Vec::new();
        for ch in self.channels.clone() {
            for ra in self.ranks.clone() {
                for bg in self.bank_groups.clone() {
                    for ba in self.banks.clone() {
                        allocated.push(make_cfg(ch, ra, bg, ba));
                    }
                }
            }
        }

        self.table.insert(key, allocated.clone());
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

        assert_eq!(allocated.len(), 16);
        assert!(allocated.contains(&engine_cfg::CGO {
            ch: 1,
            ra: 2,
            bg: 3,
            ba: 4,
        }));
        assert!(allocated.contains(&engine_cfg::CGO {
            ch: 2,
            ra: 3,
            bg: 4,
            ba: 5,
        }));
        assert!(!allocated.contains(&engine_cfg::CGO {
            ch: 0,
            ra: 2,
            bg: 3,
            ba: 4,
        }));
    }
}
