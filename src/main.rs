#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]

use cpu::pipeline::CPU;
use memory::dramsim3_wrapper::dramsim3_wrapper as dsim3_wrapper;

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

    let _pimcpu = CPU::new();
}

mod cpu;
mod errors;
mod memory;
mod sim_engine;
