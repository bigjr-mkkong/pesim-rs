use crate::cpu::pimcpu_types::fatptr_rf;
use rustc_hash::FxHashMap;
use std::cell::Cell;

pub const PIM_ENTRY_SIZE_BYTES: u64 = 16;
pub const PIM_ENTRIES_PER_CACHELINE: u64 = 64 / PIM_ENTRY_SIZE_BYTES;
const HOST_CACHELINE_ENTRIES: usize = PIM_ENTRIES_PER_CACHELINE as usize;

#[derive(Clone, Copy)]
pub enum MemEntryData {
    Untyped([u8; 16]),
    CPU_Data([u32; 4]),
    CPU_FatPtr(fatptr_rf),
    PE_Vector([i16; 8]),
    PE_Scalar(i32),
}

pub struct mem_entry {
    data: Cell<MemEntryData>,
}

impl mem_entry {
    fn new(data: MemEntryData) -> Self {
        Self {
            data: Cell::new(data),
        }
    }

    fn mirror_host_bytes(&self, backing: [u8; 16]) {
        let data = match self.data.get() {
            MemEntryData::Untyped(_) => MemEntryData::Untyped(backing),
            MemEntryData::CPU_Data(_) => MemEntryData::CPU_Data(decode_CPU_Data(backing)),
            MemEntryData::CPU_FatPtr(_) => MemEntryData::CPU_FatPtr(decode_CPU_FatPtr(backing)),
            MemEntryData::PE_Vector(_) => MemEntryData::PE_Vector(decode_PE_Vector(backing)),
            MemEntryData::PE_Scalar(_) => MemEntryData::PE_Scalar(decode_PE_Scalar(backing)),
        };
        self.data.set(data);
    }
}

fn decode_CPU_Data(backing: [u8; 16]) -> [u32; 4] {
    std::array::from_fn(|idx| {
        let start = idx * std::mem::size_of::<u32>();
        u32::from_le_bytes(backing[start..start + 4].try_into().unwrap())
    })
}

fn decode_CPU_FatPtr(backing: [u8; 16]) -> fatptr_rf {
    let encoded = u32::from_le_bytes(backing[..4].try_into().unwrap());
    fatptr_rf::new((encoded >> 28) as u8, encoded & 0x0fff_ffff)
}

fn decode_PE_Vector(backing: [u8; 16]) -> [i16; 8] {
    std::array::from_fn(|idx| {
        let start = idx * std::mem::size_of::<i16>();
        i16::from_le_bytes(backing[start..start + 2].try_into().unwrap())
    })
}

fn decode_PE_Scalar(backing: [u8; 16]) -> i32 {
    i32::from_le_bytes(backing[..4].try_into().unwrap())
}

pub trait flatmem_builder {
    type Entry;

    fn default_first() -> Option<Self::First>;
    fn read_first(entry: &Self::Entry) -> Option<Self::First>;
    fn write_first(entry: &mut Self::Entry, data: Self::First) -> Option<()>;
    fn new_first(data: Self::First) -> Self::Entry;

    fn default_second() -> Option<Self::Second>;
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
            None => B::default_first(),
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
            None => B::default_second(),
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

impl<B> flat_mem<B>
where
    B: flatmem_builder<Entry = mem_entry>,
{
    pub fn mirror_host_write(&mut self, first_entry: u32, payload: &[u64; 8]) {
        for entry_offset in 0..HOST_CACHELINE_ENTRIES {
            let mut backing = [0; 16];
            let first_dword = entry_offset * 2;
            backing[..8].copy_from_slice(&payload[first_dword].to_le_bytes());
            backing[8..].copy_from_slice(&payload[first_dword + 1].to_le_bytes());

            // for (idx, dat) in backing.iter().enumerate() {
            //     println!("bytes {} is {}", idx, dat);
            // }

            let addr = first_entry
                .checked_add(entry_offset as u32)
                .expect("host cacheline exceeds PIM flat-memory address space");

            match self.mem.entry(addr) {
                std::collections::hash_map::Entry::Occupied(ent) => {
                    ent.get().mirror_host_bytes(backing);
                }
                std::collections::hash_map::Entry::Vacant(ent) => {
                    ent.insert(mem_entry::new(MemEntryData::Untyped(backing)));
                }
            }
        }
    }
}

pub struct cpu_flatmem;
pub type cpu_flat_mem = flat_mem<cpu_flatmem>;

impl flatmem_builder for cpu_flatmem {
    type Entry = mem_entry;
    type First = [u32; 4];
    type Second = fatptr_rf;

    fn default_first() -> Option<Self::First> {
        Some([0; 4])
    }

    fn read_first(entry: &Self::Entry) -> Option<Self::First> {
        match entry.data.get() {
            MemEntryData::Untyped(backing) => {
                let data = decode_CPU_Data(backing);
                entry.data.set(MemEntryData::CPU_Data(data));
                Some(data)
            }
            MemEntryData::CPU_Data(data) => Some(data),
            _ => {
                eprintln!("memory type error: trying to read CPU data from a non-CPU-data entry");
                None
            }
        }
    }

    fn write_first(entry: &mut Self::Entry, data: Self::First) -> Option<()> {
        match entry.data.get() {
            MemEntryData::Untyped(_) | MemEntryData::CPU_Data(_) => {
                entry.data.set(MemEntryData::CPU_Data(data));
                Some(())
            }
            _ => {
                eprintln!("memory type error: trying to write CPU data into a different type");
                None
            }
        }
    }

    fn new_first(data: Self::First) -> Self::Entry {
        mem_entry::new(MemEntryData::CPU_Data(data))
    }

    fn default_second() -> Option<Self::Second> {
        eprintln!("memory safety error: trying to read a fat pointer from uninitialized memory");
        None
    }

    fn read_second(entry: &Self::Entry) -> Option<Self::Second> {
        match entry.data.get() {
            MemEntryData::Untyped(backing) => {
                let fptr = decode_CPU_FatPtr(backing);
                entry.data.set(MemEntryData::CPU_FatPtr(fptr));
                Some(fptr)
            }
            MemEntryData::CPU_FatPtr(fptr) => Some(fptr),
            _ => {
                eprintln!(
                    "memory type error: trying to read a CPU fat pointer from a different type"
                );
                None
            }
        }
    }

    fn write_second(entry: &mut Self::Entry, data: Self::Second) -> Option<()> {
        match entry.data.get() {
            MemEntryData::Untyped(_) | MemEntryData::CPU_FatPtr(_) => {
                entry.data.set(MemEntryData::CPU_FatPtr(data));
                Some(())
            }
            _ => {
                eprintln!(
                    "memory type error: trying to write a CPU fat pointer into a different type"
                );
                None
            }
        }
    }

    fn new_second(data: Self::Second) -> Self::Entry {
        mem_entry::new(MemEntryData::CPU_FatPtr(data))
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

pub struct pe_flatmem;
pub type pe_flat_mem = flat_mem<pe_flatmem>;

impl flatmem_builder for pe_flatmem {
    type Entry = mem_entry;
    type First = [i16; 8];
    type Second = i32;

    fn default_first() -> Option<Self::First> {
        Some([0; 8])
    }

    fn read_first(entry: &Self::Entry) -> Option<Self::First> {
        match entry.data.get() {
            MemEntryData::Untyped(backing) => {
                let data = decode_PE_Vector(backing);
                entry.data.set(MemEntryData::PE_Vector(data));
                Some(data)
            }
            MemEntryData::PE_Vector(data) => Some(data),
            _ => {
                eprintln!("memory type error: trying to read a PE vector from a different type");
                None
            }
        }
    }

    fn write_first(entry: &mut Self::Entry, data: Self::First) -> Option<()> {
        match entry.data.get() {
            MemEntryData::Untyped(_) | MemEntryData::PE_Vector(_) => {
                entry.data.set(MemEntryData::PE_Vector(data));
                Some(())
            }
            _ => {
                eprintln!("memory type error: trying to write a PE vector into a different type");
                None
            }
        }
    }

    fn new_first(data: Self::First) -> Self::Entry {
        mem_entry::new(MemEntryData::PE_Vector(data))
    }

    fn default_second() -> Option<Self::Second> {
        Some(0)
    }

    fn read_second(entry: &Self::Entry) -> Option<Self::Second> {
        match entry.data.get() {
            MemEntryData::Untyped(backing) => {
                let data = decode_PE_Scalar(backing);
                entry.data.set(MemEntryData::PE_Scalar(data));
                Some(data)
            }
            MemEntryData::PE_Scalar(data) => Some(data),
            _ => {
                eprintln!("memory type error: trying to read a PE scalar from a different type");
                None
            }
        }
    }

    fn write_second(entry: &mut Self::Entry, data: Self::Second) -> Option<()> {
        match entry.data.get() {
            MemEntryData::Untyped(_) | MemEntryData::PE_Scalar(_) => {
                entry.data.set(MemEntryData::PE_Scalar(data));
                Some(())
            }
            _ => {
                eprintln!("memory type error: trying to write a PE scalar into a different type");
                None
            }
        }
    }

    fn new_second(data: Self::Second) -> Self::Entry {
        mem_entry::new(MemEntryData::PE_Scalar(data))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_cacheline_splits_into_little_endian_CPU_Data_entries() {
        let mut memory = cpu_flat_mem::new();
        let payload = [
            0x1122_3344_5566_7788,
            0x3fd7_d81c_0000_0002,
            0x0000_0014_0000_0014,
            0,
            0,
            0,
            0,
            0,
        ];

        memory.mirror_host_write(4, &payload);

        assert_eq!(
            memory.mem_read_data(4),
            Some([0x5566_7788, 0x1122_3344, 2, 0x3fd7_d81c])
        );
        assert_eq!(memory.mem_read_data(5), Some([0x14, 0x14, 0, 0]));
    }

    #[test]
    fn absent_read_returns_zero_without_claiming_an_entry_type() {
        let mut memory = pe_flat_mem::new();

        assert_eq!(memory.mem_read_v(9), Some([0; 8]));
        assert_eq!(memory.mem_write_s(9, 42), Some(()));
        assert_eq!(memory.mem_read_s(9), Some(42));
        assert_eq!(memory.mem_read_v(9), None);
    }

    #[test]
    fn absent_CPU_reads_follow_data_and_fat_pointer_defaults_without_claiming() {
        let mut memory = cpu_flat_mem::new();

        assert_eq!(memory.mem_read_data(9), Some([0; 4]));
        assert_eq!(memory.mem_read_fptr(9), None);

        let fptr = fatptr_rf::new(2, 7);
        assert_eq!(memory.mem_write_fptr(9, &fptr), Some(()));
        assert_eq!(memory.mem_read_fptr(9), Some(fptr));
        assert_eq!(memory.mem_read_data(9), None);
    }

    #[test]
    fn host_overwrite_preserves_an_established_entry_type() {
        let mut memory = pe_flat_mem::new();
        let mut first_payload = [0; 8];
        first_payload[0] = u64::from_le_bytes([1, 0, 2, 0, 3, 0, 4, 0]);
        first_payload[1] = u64::from_le_bytes([5, 0, 6, 0, 7, 0, 8, 0]);
        memory.mirror_host_write(0, &first_payload);
        assert_eq!(memory.mem_read_v(0), Some([1, 2, 3, 4, 5, 6, 7, 8]));

        let mut second_payload = [0; 8];
        second_payload[0] = u64::from_le_bytes([8, 0, 7, 0, 6, 0, 5, 0]);
        second_payload[1] = u64::from_le_bytes([4, 0, 3, 0, 2, 0, 1, 0]);
        memory.mirror_host_write(0, &second_payload);

        assert_eq!(memory.mem_read_v(0), Some([8, 7, 6, 5, 4, 3, 2, 1]));
        assert_eq!(memory.mem_read_s(0), None);
    }

    #[test]
    fn host_bytes_decode_the_CPU_FatPtr_layout() {
        let mut memory = cpu_flat_mem::new();
        let mut payload = [0; 8];
        payload[0] = ((3_u64) << 28) | 0x0123_4567;
        memory.mirror_host_write(12, &payload);

        let fptr = memory
            .mem_read_fptr(12)
            .expect("host-initialized fat pointer should decode");
        assert_eq!(fptr.get_idx(), 3);
        assert_eq!(fptr.get_offset(), 0x0123_4567);
    }
}
