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

pub struct AGU_entry {
    offset_bound: Option<u32>,
}

impl AGU_entry {
    pub fn new() -> Self {
        Self { offset_bound: None }
    }

    pub fn get_bound(&self) -> u32 {
        if let Some(oft_bound) = self.offset_bound {
            check_bound!(oft_bound, BOUND_BITS)
        } else {
            panic!("AGU: Attempt to read from uninitialized AGU entry");
        }
    }

    pub fn update(&mut self, new_bound: u32) {
        if let Some(_) = self.offset_bound {
            eprintln!("Trying to update existed AGU entry: not a typical operation");
        } else {
            self.offset_bound = Some(check_bound!(new_bound, BOUND_BITS));
        }
    }
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

    pub fn insert(&mut self, id: u8, bound: u32) {
        let bound = check_bound!(bound, BOUND_BITS);
        let id = check_bound!(id, IDX_BITS);

        match self.table.entry(id) {
            std::collections::hash_map::Entry::Occupied(mut ent) => {
                eprintln!("Trying to rewrite to existed AGU entry");
                ent.get_mut().update(bound);
            }
            std::collections::hash_map::Entry::Vacant(ent) => {
                let mut bound_ent: AGU_entry = AGU_entry::new();
                bound_ent.update(bound);
                ent.insert(bound_ent);
            }
        }
    }

    pub fn accept(&self, fptr: fatptr_rf) -> bool {
        let idx = fptr.get_idx();
        let offset = fptr.get_offset();

        if let Some(bound) = self.table.get(&idx) {
            if bound.get_bound() >= offset {
                true
            } else {
                false
            }
        } else {
            false
        }
    }

    /*
     * TODO
     * Perform addition, then check
     */
    pub fn addition(&self, old_fptr: fatptr_rf, vec_rf: [u32; 4], idx: u8) -> Option<fatptr_rf> {
        todo!()
    }
}
