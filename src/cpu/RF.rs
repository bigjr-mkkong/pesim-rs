use crate::cpu::pimcpu_types::fatptr_rf;

struct arch_vregs {
    vregs_lit: [[u32; 4]; 8],
}

struct arch_fregs {
    fregs_lit: [Option<fatptr_rf>; 8],
}

pub struct arch_rf {
    pc: u16,
    vregs: arch_vregs,
    fregs: arch_fregs,
}

impl arch_vregs {
    pub const fn new() -> Self {
        Self {
            vregs_lit: [[0; 4]; 8],
        }
    }

    fn read(&self, idx: u8) -> [u32; 4] {
        assert!(idx < 8, "Failed to read VREGS: requested idx: {} >= 8", idx);

        self.vregs_lit[idx as usize]
    }

    fn write(&mut self, idx: u8, content: [u32; 4]) {
        assert!(
            idx < 8,
            "Failed to write VREGS: requested idx: {} >= 8",
            idx
        );

        if idx != 0 {
            self.vregs_lit[idx as usize] = content;
        }
    }
}

impl arch_fregs {
    pub const fn new() -> Self {
        Self {
            fregs_lit: [None; 8],
        }
    }

    fn read(&self, idx: u8) -> Option<fatptr_rf> {
        assert!(idx < 8, "Failed to read FREGS: requested idx: {} >= 8", idx);

        self.fregs_lit[idx as usize]
    }

    fn write(&mut self, idx: u8, new_fptr: fatptr_rf) {
        assert!(
            idx < 8,
            "Failed to write FREGS: requested idx: {} >= 8",
            idx
        );

        self.fregs_lit[idx as usize] = Some(new_fptr);
    }
}

impl arch_rf {
    pub const fn new() -> Self {
        Self {
            pc: 0,
            vregs: arch_vregs::new(),
            fregs: arch_fregs::new(),
        }
    }

    pub fn read_pc(&self) -> u16 {
        self.pc
    }

    pub fn write_pc(&mut self, new_pc: u16) {
        self.pc = new_pc;
    }

    pub fn read_vregs(&self, idx: u8) -> [u32; 4] {
        self.vregs.read(idx)
    }

    pub fn write_vregs(&mut self, idx: u8, content: [u32; 4]) {
        self.vregs.write(idx, content)
    }

    pub fn read_fregs(&self, idx: u8) -> Option<fatptr_rf> {
        self.fregs.read(idx)
    }

    pub fn write_fregs(&mut self, idx: u8, new_fptr: fatptr_rf) {
        self.fregs.write(idx, new_fptr)
    }
}
