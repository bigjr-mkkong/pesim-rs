#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]

use memory::dramsim3_wrapper::dramsim3_wrapper as dsim3_wrapper;
use cpu::pipeline::CPU;
use cpu::pimcpu_types::{inst, fatptr_rf};

const DSIM3_CFG_PATH: &str = "/home/michael/Projects/playground/testprogram/pesim-rs/third-party/DRAMsim3/configs/DDR4_4Gb_x4_2400.ini";

const DSIM3_OUT_DIR: &str = "/home/michael/Projects/playground/testprogram/pesim-rs/output";

fn main() {
    let mut dsim3_inst: dsim3_wrapper =
        dsim3_wrapper::new(DSIM3_CFG_PATH, DSIM3_OUT_DIR, 0, 0, 0, 0);

    println!("Dramsim3 tCK: {}", dsim3_inst.get_TCK());

    let addr: u64 = 0x0;

    let is_write: bool = false;
    if dsim3_inst.WillAcceptTransaction(addr, is_write) == true {
        dsim3_inst.AddTransaction(addr, is_write, false);
    }

    loop {
        dsim3_inst.ClockTick();
        let pend_tr = dsim3_inst.get_pend_write(addr, false);
        if pend_tr == 0 {
            break;
        }
    }
    println!("Dramsim3 PING-PONG works");

    let mut pimcpu = CPU::new();

    // Register memory region entry with ID 0.
// Region starts at address 0 and has 16 entries of [u32; 4].
pimcpu.get_agu().insert(0, 0, 16);

// fregs[0] -> MEM[0]
// fregs[1] -> MEM[1]
pimcpu.get_RF().write_fregs(0, fatptr_rf::new(0, 0));
pimcpu.get_RF().write_fregs(1, fatptr_rf::new(0, 1));

// Initial memory:
// MEM[0] contains the value to load.
// MEM[1] is the store destination.
pimcpu.get_fmem().mem_write_data(0, &[123; 4]);
pimcpu.get_fmem().mem_write_data(1, &[0; 4]);

// Initial vRF:
// v3 intentionally starts as zero.
// If ST128 reads stale v3, it will store [0; 4] into MEM[1].
pimcpu.get_RF().write_vregs(3, [0; 4]);
pimcpu.get_RF().write_vregs(4, [0; 4]);

let prog: [inst; 3] = [
    // v3 = MEM[0] = [123; 4]
    inst::LD128 { rd: 3, frs: 0 },

    // Should store newly loaded v3 into MEM[1].
    // If load-to-store-data forwarding/stall is broken,
    // this stores stale v3 = [0; 4].
    inst::ST128 { rs: 3, frd: 1 },

    // Reload MEM[1] into v4.
    inst::LD128 { rd: 4, frs: 1 },
];

pimcpu.get_imem().flash_in(&prog);

for _cycle in 0..30 {
    pimcpu.tick();
}

println!("v3 = {:?}", pimcpu.get_RF().read_vregs(3));
println!("v4 = {:?}", pimcpu.get_RF().read_vregs(4));

assert_eq!(pimcpu.get_RF().read_vregs(3), [123; 4]);
assert_eq!(pimcpu.get_RF().read_vregs(4), [123; 4]);

}

// mod dramsim3_cxx_ffi;
// mod dramsim3_wrapper;
mod cpu;
mod errors;
mod memory;
