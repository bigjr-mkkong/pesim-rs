#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]

use cpu::pipeline::CPU;
use std::ffi::{CStr, c_char};
use std::path::PathBuf;

#[cfg(not(test))]
pub const PIM_DSIM3_CFG_PATH: &str = "/gem5/ext/pesim/pesim-rs/cfg/DDR4_8Gb_x4_2400_pim.ini";
#[cfg(test)]
pub const PIM_DSIM3_CFG_PATH: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/cfg/DDR4_8Gb_x4_2400_pim.ini");
#[cfg(test)]
pub const PIM_PRECACT_DSIM3_CFG_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/cfg/DDR4_8Gb_x4_2400_pim_prec_act.ini"
);
#[cfg(not(test))]
pub const FALLBACK_DSIM3_CFG_PATH: &str = "/gem5/ext/pesim/pesim-rs/cfg/DDR4_8Gb_x4_2400.ini";
#[cfg(test)]
pub const FALLBACK_DSIM3_CFG_PATH: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/cfg/DDR4_8Gb_x4_2400.ini");
#[cfg(not(test))]
pub const DSIM3_OUT_DIR: &str = "/gem5/ext/pesim/pesim-rs/output";
#[cfg(test)]
pub const DSIM3_OUT_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/output");

fn dsim3_paths(config: impl Into<PathBuf>, output: impl Into<PathBuf>) -> (PathBuf, PathBuf) {
    let config_path = config.into();
    let out_dir = output.into();

    if !config_path.is_file() {
        panic!("cannot find DSIM3 config file: {}", config_path.display());
    }

    if !out_dir.exists() {
        std::fs::create_dir_all(&out_dir).unwrap_or_else(|error| {
            panic!(
                "cannot create DSIM3 output directory {}: {error}",
                out_dir.display()
            )
        });
    }

    (config_path, out_dir)
}

mod PE;
mod cpu;
mod errors;
pub mod memory;
mod sim_engine;

/*
 * #pragma once

#include <stdint.h>
#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif


typedef struct PEsim_rs_MemReq
{
    uint64_t addr;
    uint64_t issue_time;
    uint64_t payload_word0;
    bool is_write;
    bool is_pim_query;
}PEsim_rs_MemReq;

typedef struct PESim_payload{
    uint64_t dword_payload[8];
    uint32_t payload_sz_bytes;
}PESim_payload;

typedef struct PESim_config {
    const char *config_file;
    const char *output_dir;
    uint32_t controller_id;
    uint64_t controller_base;
    uint64_t controller_size;
    uint64_t pim_size;
} PESim_config;

typedef struct PESim_body PESim_body;

PESim_body *pesim_new(const PESim_config *config);
void pesim_free(PESim_body *sim);

void pesim_print_stats(PESim_body *sim);
void pesim_reset_stats(PESim_body *sim);

bool pesim_canAccept(PESim_body *sim, uint64_t addr, bool is_write);
bool pesim_enqueue_with_data(PESim_body *sim, uint64_t addr, PESim_payload payload, bool is_write);
bool pesim_can_accept_pim_cmd(PESim_body *sim, uint64_t offset, PESim_payload payload, bool is_write);
bool pesim_enqueue_pim_cmd(PESim_body *sim, uint64_t offset, PESim_payload payload, bool is_write);

double pesim_clock_period(PESim_body *sim);
unsigned int pesim_queue_size(PESim_body *sim);
unsigned int pesim_burst_size(PESim_body *sim);

bool pesim_has_complete(PESim_body *sim);
PEsim_rs_MemReq pesim_get_complete(PESim_body *sim);

void pesim_tick(PESim_body *sim);


#ifdef __cplusplus
}
#endif

*/

use crate::sim_engine::sim::{Sim, SimConfig};
use std::panic::{AssertUnwindSafe, catch_unwind};

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PEsim_rs_MemReq {
    pub addr: u64,
    pub issue_time: u64,
    pub payload_word0: u64,
    pub is_write: bool,
    pub is_pim_query: bool,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PESim_payload {
    pub dword_payload: [u64; 8],
    pub payload_sz_bytes: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct PESim_config {
    pub config_file: *const c_char,
    pub output_dir: *const c_char,
    pub controller_id: u32,
    pub controller_base: u64,
    pub controller_size: u64,
    pub pim_size: u64,
}

pub struct PESim_body {
    sim: Sim,
    ticks: u64,
    enqueued: u64,
    completions_returned: u64,
}

impl PESim_body {
    fn new(config: SimConfig) -> Self {
        Self {
            sim: Sim::from_config(config),
            ticks: 0,
            enqueued: 0,
            completions_returned: 0,
        }
    }
}

fn with_body_mut<T: Copy>(
    sim: *mut PESim_body,
    fallback: T,
    f: impl FnOnce(&mut PESim_body) -> T,
) -> T {
    if sim.is_null() {
        return fallback;
    }

    catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: Null was rejected above. The C API requires exclusive access
        // to PESim_body for the duration of every call taking this pointer.
        f(unsafe { &mut *sim })
    }))
    .unwrap_or(fallback)
}

#[unsafe(no_mangle)]
pub extern "C" fn pesim_new(config: *const PESim_config) -> *mut PESim_body {
    match catch_unwind(AssertUnwindSafe(|| {
        assert!(!config.is_null(), "PESim configuration pointer is null");
        // SAFETY: The caller promises that `config` points to a live C
        // configuration for the duration of this constructor call.
        let config = unsafe { &*config };
        assert!(
            !config.config_file.is_null(),
            "DRAMSim3 config path is null"
        );
        assert!(!config.output_dir.is_null(), "DRAMSim3 output path is null");
        // SAFETY: Both pointers are required to reference NUL-terminated C
        // strings. They are copied into owned PathBuf values immediately.
        let config_file = unsafe { CStr::from_ptr(config.config_file) }
            .to_str()
            .expect("DRAMSim3 config path must be valid UTF-8");
        let output_dir = unsafe { CStr::from_ptr(config.output_dir) }
            .to_str()
            .expect("DRAMSim3 output path must be valid UTF-8");
        let (config_file, output_dir) = dsim3_paths(config_file, output_dir);
        let sim_config = SimConfig {
            config_file,
            output_dir,
            controller_id: config.controller_id,
            controller_base: config.controller_base,
            controller_size: config.controller_size,
            pim_size: config.pim_size,
        };
        Box::into_raw(Box::new(PESim_body::new(sim_config)))
    })) {
        Ok(sim) => sim,
        Err(payload) => {
            let message = payload
                .downcast_ref::<&str>()
                .copied()
                .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
                .unwrap_or("non-string panic payload");

            eprintln!("pesim_new: PESim_body::new() panicked: {message}");
            std::ptr::null_mut()
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn pesim_free(sim: *mut PESim_body) {
    if sim.is_null() {
        return;
    }

    // SAFETY: pesim_new returns ownership of exactly one Box allocation. The
    // caller must pass that pointer to pesim_free at most once.
    unsafe {
        drop(Box::from_raw(sim));
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn pesim_print_stats(sim: *mut PESim_body) {
    with_body_mut(sim, (), |body| {
        body.sim.print_cgo_switch_stats();
        println!(
            "PESim stats: ticks={}, enqueued={}, completions_returned={}",
            body.ticks, body.enqueued, body.completions_returned
        );
    });
}

#[unsafe(no_mangle)]
pub extern "C" fn pesim_reset_stats(sim: *mut PESim_body) {
    with_body_mut(sim, (), |body| {
        body.ticks = 0;
        body.enqueued = 0;
        body.completions_returned = 0;
    });
}

#[unsafe(no_mangle)]
pub extern "C" fn pesim_canAccept(sim: *mut PESim_body, addr: u64, is_write: bool) -> bool {
    with_body_mut(sim, false, |body| body.sim.canAccept(addr, is_write))
}

#[unsafe(no_mangle)]
pub extern "C" fn pesim_enqueue_with_data(
    sim: *mut PESim_body,
    addr: u64,
    payload: PESim_payload,
    is_write: bool,
) -> bool {
    with_body_mut(sim, false, |body| {
        assert!(
            payload.payload_sz_bytes as usize <= std::mem::size_of_val(&payload.dword_payload),
            "PESim payload cannot exceed 64 bytes"
        );
        if !body.sim.canAccept(addr, is_write) {
            return false;
        }

        body.sim.enqueue_with_data(
            addr,
            payload.dword_payload,
            payload.payload_sz_bytes,
            is_write,
        );
        body.enqueued += 1;

        /*
         * For endieness testing
         */
        // if is_write && payload.payload_sz_bytes == 64{
        //     for (idx, i) in payload.dword_payload.iter().enumerate() {
        //         let lo32: u32 = *i as u32;
        //         let hi32: u32 = (*i>>32) as u32;
        //         println!("lo32 for dword {} is: {:x}", idx, lo32);
        //         println!("hi for dword {} is: {:x}", idx, hi32);
        //     }
        // }
        true
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn pesim_can_accept_pim_cmd(
    sim: *mut PESim_body,
    offset: u64,
    payload: PESim_payload,
    is_write: bool,
) -> bool {
    with_body_mut(sim, false, |body| {
        if payload.payload_sz_bytes as usize > std::mem::size_of_val(&payload.dword_payload) {
            return false;
        }
        body.sim.canAcceptPimCmdAccess(
            offset,
            payload.dword_payload,
            payload.payload_sz_bytes,
            is_write,
        )
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn pesim_enqueue_pim_cmd(
    sim: *mut PESim_body,
    offset: u64,
    payload: PESim_payload,
    is_write: bool,
) -> bool {
    with_body_mut(sim, false, |body| {
        if payload.payload_sz_bytes as usize > std::mem::size_of_val(&payload.dword_payload) {
            return false;
        }
        let accepted = body.sim.enqueuePimCmdAccess(
            offset,
            payload.dword_payload,
            payload.payload_sz_bytes,
            is_write,
        );
        body.enqueued += u64::from(accepted);
        accepted
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn pesim_clock_period(sim: *mut PESim_body) -> f64 {
    with_body_mut(sim, 0.0, |body| body.sim.clock_period())
}

#[unsafe(no_mangle)]
pub extern "C" fn pesim_queue_size(sim: *mut PESim_body) -> u32 {
    with_body_mut(sim, 0, |body| body.sim.queue_size())
}

#[unsafe(no_mangle)]
pub extern "C" fn pesim_burst_size(sim: *mut PESim_body) -> u32 {
    with_body_mut(sim, 0, |body| body.sim.burst_size())
}

#[unsafe(no_mangle)]
pub extern "C" fn pesim_has_complete(sim: *mut PESim_body) -> bool {
    with_body_mut(sim, false, |body| body.sim.hasComplete())
}

#[unsafe(no_mangle)]
pub extern "C" fn pesim_get_complete(sim: *mut PESim_body) -> PEsim_rs_MemReq {
    with_body_mut(sim, PEsim_rs_MemReq::default(), |body| {
        let Some(req) = body.sim.getComplete() else {
            return PEsim_rs_MemReq::default();
        };

        body.completions_returned += 1;
        PEsim_rs_MemReq {
            addr: req.get_addr(),
            issue_time: req.get_issue_time().unwrap_or(0),
            payload_word0: req.get_payload()[0],
            is_write: !req.is_read(),
            is_pim_query: req.is_read()
                && req.get_addr() == crate::sim_engine::request_router::PIM_QUERY_OFFSET,
        }
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn pesim_tick(sim: *mut PESim_body) {
    with_body_mut(sim, (), |body| {
        body.sim.tick();
        body.ticks += 1;
    });
}

#[cfg(test)]
#[path = "lib_test.rs"]
mod lib_test;

#[cfg(test)]
#[path = "pipeline-validation.rs"]
mod pipeline_validation;
