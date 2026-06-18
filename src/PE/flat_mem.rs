use rustc_hash::FxHashMap;

pub enum dram_entry {
    VDATA([i16; 8]),
    SDATA(i32),
}

pub struct flat_mem {
    mem: FxHashMap<u32, dram_entry>,
}

impl flat_mem {
    pub fn new() -> Self {
        Self {
            mem: FxHashMap::default(),
        }
    }

    pub fn mem_read_v(&self, addr: u32) -> Option<[i16; 8]> {
        let ent = self.mem.get(&addr).unwrap_or(&dram_entry::VDATA([0; 8]));
        match ent {
            dram_entry::VDATA(data) => Some(*data),
            dram_entry::SDATA(_) => {
                eprintln!("memory type error: Trying to read vector out of scalar memory");
                None
            }
        }
    }

    pub fn mem_write_v(&mut self, addr: u32, data: &[i16; 8]) -> Option<()> {
        match self.mem.entry(addr) {
            std::collections::hash_map::Entry::Occupied(mut ent) => {
                if let dram_entry::VDATA(_) = ent.get_mut() {
                    ent.insert(dram_entry::VDATA(*data));
                    Some(())
                } else {
                    eprintln!("memory type error: trying to write vector into scalar location");
                    None
                }
            }
            std::collections::hash_map::Entry::Vacant(ent) => {
                ent.insert(dram_entry::VDATA(*data));
                Some(())
            }
        }
    }

    pub fn mem_read_s(&self, addr: u32) -> Option<i32> {
        let ent = self.mem.get(&addr).unwrap_or(&dram_entry::SDATA(0));
        match ent {
            dram_entry::SDATA(data) => Some(*data),
            dram_entry::VDATA(_) => {
                eprintln!("memory type error: Trying to read scalar out of vector memory");
                None
            }
        }
    }

    pub fn mem_write_s(&mut self, addr: u32, data: i32) -> Option<()> {
        match self.mem.entry(addr) {
            std::collections::hash_map::Entry::Occupied(mut ent) => {
                if let dram_entry::SDATA(_) = ent.get_mut() {
                    ent.insert(dram_entry::SDATA(data));
                    Some(())
                } else {
                    eprintln!("memory type error: trying to write scalar into vector location");
                    None
                }
            }
            std::collections::hash_map::Entry::Vacant(ent) => {
                ent.insert(dram_entry::SDATA(data));
                Some(())
            }
        }
    }
}
