use crate::sim_engine::sim::engine_cfg;
use std::collections::HashMap;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum EngineAllocKind {
    Cgo,
    Fgo,
}

pub struct engine_alloc {
    max_ch: u64,
    max_ra: u64,
    max_bg: u64,
    max_ba: u64,
    winner: Option<u64>,
    table: HashMap<(u64, EngineAllocKind), Vec<engine_cfg>>,
}

impl engine_alloc {
    pub fn new(max_ch: u64, max_ra: u64, max_bg: u64, max_ba: u64) -> Self {
        Self {
            max_ch,
            max_ra,
            max_bg,
            max_ba,
            winner: None,
            table: HashMap::new(),
        }
    }

    pub fn alloc_cgo(&mut self, asid: u64) -> Vec<engine_cfg> {
        self.alloc(asid, EngineAllocKind::Cgo, |ch, ra, bg, ba| {
            engine_cfg::CGO { ch, ra, bg, ba }
        })
    }

    pub fn alloc_fgo(&mut self, asid: u64) -> Vec<engine_cfg> {
        self.alloc(asid, EngineAllocKind::Fgo, |ch, ra, bg, ba| {
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
        for ch in 0..self.max_ch {
            for ra in 0..self.max_ra {
                for bg in 0..self.max_bg {
                    for ba in 0..self.max_ba {
                        allocated.push(make_cfg(ch, ra, bg, ba));
                    }
                }
            }
        }

        self.table.insert(key, allocated.clone());
        allocated
    }
}
