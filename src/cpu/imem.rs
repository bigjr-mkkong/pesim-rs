use crate::cpu::pimcpu_types::inst;

pub struct IMEM {
    prog: Vec<inst>,
}

impl IMEM {
    pub fn new() -> Self {
        Self { prog: Vec::new() }
    }

    pub fn flash_in(&mut self, new_prog: &[inst]) {
        self.prog = new_prog.to_vec();
    }

    pub fn read_inst(&self, pc: u16) -> Option<inst> {
        self.prog.get(pc as usize).cloned()
    }
}
