use crate::cpu::pimcpu_types::fatptr_rf;
use rustc_hash::FxHashMap;

pub trait flatmem_builder {
    type Entry;

    fn default_first() -> Self::Entry;
    fn read_first(entry: &Self::Entry) -> Option<Self::First>;
    fn write_first(entry: &mut Self::Entry, data: Self::First) -> Option<()>;
    fn new_first(data: Self::First) -> Self::Entry;

    fn default_second() -> Self::Entry;
    fn read_second(entry: &Self::Entry) -> Option<Self::Second>;
    fn write_second(entry: &mut Self::Entry, data: Self::Second) -> Option<()>;
    fn new_second(data: Self::Second) -> Self::Entry;

    type First: Copy;
    type Second: Copy;
}

pub struct flat_mem<B: flatmem_builder> {
    mem: FxHashMap<u32, B::Entry>,
}

impl<B: flatmem_builder> flat_mem<B> {
    pub fn new() -> Self {
        Self {
            mem: FxHashMap::default(),
        }
    }

    fn read_first(&self, addr: u32) -> Option<B::First> {
        match self.mem.get(&addr) {
            Some(ent) => B::read_first(ent),
            None => B::read_first(&B::default_first()),
        }
    }

    fn write_first(&mut self, addr: u32, data: B::First) -> Option<()> {
        match self.mem.entry(addr) {
            std::collections::hash_map::Entry::Occupied(mut ent) => {
                B::write_first(ent.get_mut(), data)
            }
            std::collections::hash_map::Entry::Vacant(ent) => {
                ent.insert(B::new_first(data));
                Some(())
            }
        }
    }

    fn read_second(&self, addr: u32) -> Option<B::Second> {
        match self.mem.get(&addr) {
            Some(ent) => B::read_second(ent),
            None => B::read_second(&B::default_second()),
        }
    }

    fn write_second(&mut self, addr: u32, data: B::Second) -> Option<()> {
        match self.mem.entry(addr) {
            std::collections::hash_map::Entry::Occupied(mut ent) => {
                B::write_second(ent.get_mut(), data)
            }
            std::collections::hash_map::Entry::Vacant(ent) => {
                ent.insert(B::new_second(data));
                Some(())
            }
        }
    }
}

pub enum cpu_dram_entry {
    DATA([u32; 4]),
    FPTR(fatptr_rf),
}

pub struct cpu_flatmem;
pub type cpu_flat_mem = flat_mem<cpu_flatmem>;

impl flatmem_builder for cpu_flatmem {
    type Entry = cpu_dram_entry;
    type First = [u32; 4];
    type Second = fatptr_rf;

    fn default_first() -> Self::Entry {
        cpu_dram_entry::DATA([0; 4])
    }

    fn read_first(entry: &Self::Entry) -> Option<Self::First> {
        match entry {
            cpu_dram_entry::DATA(data) => Some(*data),
            cpu_dram_entry::FPTR(_) => {
                eprintln!("memory type error: Trying to read data out of fptr memory");
                None
            }
        }
    }

    fn write_first(entry: &mut Self::Entry, data: Self::First) -> Option<()> {
        match entry {
            cpu_dram_entry::DATA(slot) => {
                *slot = data;
                Some(())
            }
            cpu_dram_entry::FPTR(_) => {
                eprintln!("memory type error: trying to write data into fptr location");
                None
            }
        }
    }

    fn new_first(data: Self::First) -> Self::Entry {
        cpu_dram_entry::DATA(data)
    }

    fn default_second() -> Self::Entry {
        panic!("Cannot read fptr from uninitialized location")
    }

    fn read_second(entry: &Self::Entry) -> Option<Self::Second> {
        match entry {
            cpu_dram_entry::DATA(_) => {
                eprintln!("memory type error: Trying to read data out of fptr memory");
                None
            }
            cpu_dram_entry::FPTR(fptr) => Some(*fptr),
        }
    }

    fn write_second(entry: &mut Self::Entry, data: Self::Second) -> Option<()> {
        match entry {
            cpu_dram_entry::FPTR(slot) => {
                *slot = data;
                Some(())
            }
            cpu_dram_entry::DATA(_) => {
                eprintln!("memory type error: trying to write data into fptr location");
                None
            }
        }
    }

    fn new_second(data: Self::Second) -> Self::Entry {
        cpu_dram_entry::FPTR(data)
    }
}

impl cpu_flat_mem {
    pub fn mem_read_data(&self, addr: u32) -> Option<[u32; 4]> {
        self.read_first(addr)
    }

    pub fn mem_write_data(&mut self, addr: u32, data: &[u32; 4]) -> Option<()> {
        self.write_first(addr, *data)
    }

    pub fn mem_read_fptr(&self, addr: u32) -> Option<fatptr_rf> {
        self.read_second(addr)
    }

    pub fn mem_write_fptr(&mut self, addr: u32, fptr: &fatptr_rf) -> Option<()> {
        self.write_second(addr, *fptr)
    }
}

pub enum pe_dram_entry {
    VDATA([i16; 8]),
    SDATA(i32),
}

pub struct pe_flatmem;
pub type pe_flat_mem = flat_mem<pe_flatmem>;

impl flatmem_builder for pe_flatmem {
    type Entry = pe_dram_entry;
    type First = [i16; 8];
    type Second = i32;

    fn default_first() -> Self::Entry {
        pe_dram_entry::VDATA([0; 8])
    }

    fn read_first(entry: &Self::Entry) -> Option<Self::First> {
        match entry {
            pe_dram_entry::VDATA(data) => Some(*data),
            pe_dram_entry::SDATA(_) => {
                eprintln!("memory type error: Trying to read vector out of scalar memory");
                None
            }
        }
    }

    fn write_first(entry: &mut Self::Entry, data: Self::First) -> Option<()> {
        match entry {
            pe_dram_entry::VDATA(slot) => {
                *slot = data;
                Some(())
            }
            pe_dram_entry::SDATA(_) => {
                eprintln!("memory type error: trying to write vector into scalar location");
                None
            }
        }
    }

    fn new_first(data: Self::First) -> Self::Entry {
        pe_dram_entry::VDATA(data)
    }

    fn default_second() -> Self::Entry {
        pe_dram_entry::SDATA(0)
    }

    fn read_second(entry: &Self::Entry) -> Option<Self::Second> {
        match entry {
            pe_dram_entry::SDATA(data) => Some(*data),
            pe_dram_entry::VDATA(_) => {
                eprintln!("memory type error: Trying to read scalar out of vector memory");
                None
            }
        }
    }

    fn write_second(entry: &mut Self::Entry, data: Self::Second) -> Option<()> {
        match entry {
            pe_dram_entry::SDATA(slot) => {
                *slot = data;
                Some(())
            }
            pe_dram_entry::VDATA(_) => {
                eprintln!("memory type error: trying to write scalar into vector location");
                None
            }
        }
    }

    fn new_second(data: Self::Second) -> Self::Entry {
        pe_dram_entry::SDATA(data)
    }
}

impl pe_flat_mem {
    pub fn mem_read_v(&self, addr: u32) -> Option<[i16; 8]> {
        self.read_first(addr)
    }

    pub fn mem_write_v(&mut self, addr: u32, data: &[i16; 8]) -> Option<()> {
        self.write_first(addr, *data)
    }

    pub fn mem_read_s(&self, addr: u32) -> Option<i32> {
        self.read_second(addr)
    }

    pub fn mem_write_s(&mut self, addr: u32, data: i32) -> Option<()> {
        self.write_second(addr, data)
    }
}
