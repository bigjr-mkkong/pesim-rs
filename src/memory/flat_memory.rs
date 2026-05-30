/*
 * TODO:
 * In this file, implement a conceptual memory which handle read/write only
 *
 * The actual interface with DRAMSim3 should be little bit complex. Some arbeiter in between flat
 * model and dram timing should be established so it can communicate with simulator high level for
 * AddTransaction()
 *
 * Another thing is pe controller(the one receive PIM_start, PIM_pause and PIM_resume) shoud also 
 * talk with high level simulator. abstraction design in between worth for more thought
 *
 * Also, previous error handling should all use Result instead of eprintln!() with Option
 */

use rustc_hash::FxHashMap;
use crate::cpu::pimcpu_types::fatptr_rf;

pub enum dram_entry{
    DATA([u32; 4]),
    FPTR(fatptr_rf)
}

pub struct flat_mem{
    mem: FxHashMap<u32, dram_entry>,
}

impl flat_mem{
    pub fn new() -> Self{
        Self{
            mem: FxHashMap::default()
        }
    }

    pub fn mem_read_data(&self, addr: u32) -> Option<[u32; 4]> {
        let ent = self.mem.get(&addr).unwrap_or(&dram_entry::DATA([0; 4]));
        match ent{
            dram_entry::DATA(data) => {
                Some(*data)
            },
            dram_entry::FPTR(_) => {
                eprintln!("memory type error: Trying to read data out of fptr memory");
                None
            }
        }
    }

    pub fn mem_write_data(&mut self, addr: u32, data: &[u32; 4]) -> Option<()>{
        match self.mem.entry(addr) {
            std::collections::hash_map::Entry::Occupied(mut ent) => {
                if let dram_entry::DATA(_) = ent.get_mut() {
                    ent.insert(dram_entry::DATA(*data));
                    Some(())
                } else {
                    eprintln!("memory type error: trying to write data into fptr location");
                    None
                }
            },
            std::collections::hash_map::Entry::Vacant(ent) => {
                ent.insert(dram_entry::DATA(*data));
                Some(())
            }
        }
    }

    pub fn mem_read_fptr(&self, addr: u32) -> Option<fatptr_rf> {
        let ent = self.mem.get(&addr).expect("Cannot read fptr from uninitialized location");
        match ent{
            dram_entry::DATA(_) => {
                eprintln!("memory type error: Trying to read data out of fptr memory");
                None
            },
            dram_entry::FPTR(fptr) => {
                Some(*fptr)
            }
        }
    }

    pub fn mem_write_fptr(&mut self, addr: u32, fptr: &fatptr_rf) -> Option<()>{
        match self.mem.entry(addr) {
            std::collections::hash_map::Entry::Occupied(mut ent) => {
                if let dram_entry::FPTR(_) = ent.get_mut() {
                    ent.insert(dram_entry::FPTR(*fptr));
                    Some(())
                } else {
                    eprintln!("memory type error: trying to write data into fptr location");
                    None
                }
            },
            std::collections::hash_map::Entry::Vacant(ent) => {
                ent.insert(dram_entry::FPTR(*fptr));
                Some(())
            }
        }
    }
}
