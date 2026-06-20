#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]

use cpu::pipeline::CPU;

pub const DSIM3_CFG_PATH: &str =
    "/home/michael/Projects/playground/testprogram/pesim-rs/cfg/DDR4_8Gb_x4_2400_pim.ini";

pub const DSIM3_OUT_DIR: &str = "/home/michael/Projects/playground/testprogram/pesim-rs/output";

fn main() {
    println!("WIP");
}

mod PE;
mod cpu;
mod errors;
mod memory;
mod sim_engine;
