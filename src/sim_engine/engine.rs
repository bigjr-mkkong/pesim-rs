use crate::CPU;
use crate::memory::dramsim3_wrapper::dramsim3_wrapper;
use crate::memory::mem_portal::{dram_req, dram_portal};

enum EngineMode{
    PIM,
    HOST
}

struct Engine{
    sim_cpu: CPU,
    dram_port: dram_portal,
    mode: EngineMode
}

impl Engine{
    pub fn new() -> Self{
        Self{
            sim_cpu: CPU::new(),
            dram_port: dram_portal::new(),
            mode: EngineMode::HOST
        }
    }

}
