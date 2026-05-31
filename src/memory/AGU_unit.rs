use crate::cpu::pimcpu_types::fatptr_rf;
use std::collections::HashMap;

pub const IDX_BITS: usize = 4;
pub const BOUND_BITS: usize = 28;

#[macro_export]
macro_rules! check_bound {
    ($val:expr, $bits:expr) => {
        if $val < (1 << $bits) {
            $val
        } else {
            panic!(
                "AGU: entry number {} is larger than the allowed limit of {} bits (max {})",
                $val,
                $bits,
                (1 << $bits) - 1
            );
        }
    };
}

pub enum AGU_entry {
    NA,
    Ent { base: u32, bound: u32 },
}

pub struct AGU_unit {
    table: HashMap<u8, AGU_entry>,
}

impl AGU_unit {
    pub fn new() -> Self {
        Self {
            table: HashMap::new(),
        }
    }

    pub fn insert(&mut self, id: u8, base: u32, bound: u32) {
        let bound = check_bound!(bound, BOUND_BITS);
        let id = check_bound!(id, IDX_BITS);

        match self.table.entry(id) {
            std::collections::hash_map::Entry::Occupied(mut ent) => {
                eprintln!("Trying to rewrite to existed AGU entry");
                ent.insert(AGU_entry::Ent {
                    base: base,
                    bound: bound,
                });
            }
            std::collections::hash_map::Entry::Vacant(ent) => {
                ent.insert(AGU_entry::Ent {
                    base: base,
                    bound: bound,
                });
            }
        }
    }

    pub fn accept(&self, fptr: fatptr_rf) -> bool {
        let idx = fptr.get_idx();
        let offset = fptr.get_offset();

        if let Some(ent) = self.table.get(&idx) {
            match ent {
                AGU_entry::Ent { base, bound } => *bound > offset,
                AGU_entry::NA => false,
            }
        } else {
            false
        }
    }

    pub fn addition(&self, old_fptr: fatptr_rf, vec_rf: [u32; 4], idx: u8) -> Option<fatptr_rf> {
        if idx >= 4 {
            return None;
        }
        let rs2 = vec_rf[idx as usize];
        let tag = old_fptr.get_idx();
        let old_offset = old_fptr.get_offset();

        let new_offset = old_offset + rs2;

        let new_fptr = fatptr_rf::new(tag, new_offset);

        if self.accept(new_fptr) {
            Some(new_fptr)
        } else {
            None
        }
    }

    pub fn subtraction(&self, old_fptr: fatptr_rf, vec_rf: [u32; 4], idx: u8) -> Option<fatptr_rf> {
        if idx >= 4 {
            return None;
        }

        let rs2 = vec_rf[idx as usize];
        let tag = old_fptr.get_idx();
        let old_offset = old_fptr.get_offset();

        if old_offset <= rs2 {
            return None;
        }

        let new_offset = old_offset - rs2;

        let new_fptr = fatptr_rf::new(tag, new_offset);

        if self.accept(new_fptr) {
            Some(new_fptr)
        } else {
            None
        }
    }

    pub fn translate(&self, fptr: fatptr_rf) -> Option<u32> {
        let idx = fptr.get_idx();
        let offset = fptr.get_offset();

        if !self.accept(fptr) {
            return None;
        } else {
            if let Some(ent) = self.table.get(&idx) {
                match ent {
                    AGU_entry::NA => None,
                    AGU_entry::Ent { base, bound } => Some(base + offset),
                }
            } else {
                None
            }
        }
    }
}
