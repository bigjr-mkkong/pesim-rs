#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]

use cpu::pipeline::CPU;
use sim_engine::engine_test::engine_runs_pim_load_through_mem_fsm_and_dram_portal;

pub const DSIM3_CFG_PATH: &str = "/home/michael/Projects/playground/testprogram/pesim-rs/third-party/DRAMsim3/configs/DDR4_4Gb_x4_2400.ini";

pub const DSIM3_OUT_DIR: &str = "/home/michael/Projects/playground/testprogram/pesim-rs/output";

fn main() {
    engine_runs_pim_load_through_mem_fsm_and_dram_portal();
    println!("Finished");
}

mod cpu;
mod errors;
mod memory;
mod sim_engine;
