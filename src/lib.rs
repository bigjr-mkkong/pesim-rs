#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]

use cpu::pipeline::CPU;
use std::path::PathBuf;

use sim_engine::request_router::MEM_BEGIN;

#[cfg(not(test))]
pub const DSIM3_CFG_PATH: &str = "/gem5/ext/pesim/pesim-rs/cfg/DDR4_8Gb_x4_2400_pim.ini";
#[cfg(test)]
pub const DSIM3_CFG_PATH: &str =
    "/home/michael/Projects/pimtlb/gem5/ext/pesim/pesim-rs/cfg/DDR4_8Gb_x4_2400_pim.ini";

#[cfg(not(test))]
pub const DSIM3_OUT_DIR: &str = "/gem5/ext/pesim/pesim-rs/output";
#[cfg(test)]
pub const DSIM3_OUT_DIR: &str = "/home/michael/Projects/pimtlb/gem5/ext/pesim/pesim-rs/output";

fn dsim3_paths() -> (PathBuf, PathBuf) {
    let config_path = PathBuf::from(DSIM3_CFG_PATH);
    let out_dir = PathBuf::from(DSIM3_OUT_DIR);

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
mod memory;
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
    bool is_write;
}PEsim_rs_MemReq;

typedef struct PESim_payload{
    uint64_t dword_payload[8];
    uint32_t payload_sz_bytes;
}PESim_payload;

typedef struct PESim_body PESim_body;

PESim_body *pesim_new(void);
void pesim_free(PESim_body *sim);

void pesim_print_stats(PESim_body *sim);
void pesim_reset_stats(PESim_body *sim);

bool pesim_canAccept(PESim_body *sim, uint64_t addr, bool is_write);
bool pesim_enqueue_with_data(PESim_body *sim, uint64_t addr, PESim_payload payload, bool is_write);

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

use crate::sim_engine::sim::Sim;
use std::panic::{AssertUnwindSafe, catch_unwind};

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PEsim_rs_MemReq {
    pub addr: u64,
    pub issue_time: u64,
    pub is_write: bool,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PESim_payload {
    pub dword_payload: [u64; 8],
    pub payload_sz_bytes: u32,
}

pub struct PESim_body {
    sim: Sim,
    ticks: u64,
    enqueued: u64,
    completions_returned: u64,
}

impl PESim_body {
    fn new() -> Self {
        Self {
            sim: Sim::new(),
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
pub extern "C" fn pesim_new() -> *mut PESim_body {
    match catch_unwind(AssertUnwindSafe(|| {
        Box::into_raw(Box::new(PESim_body::new()))
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
    with_body_mut(sim, false, |body| {
        assert!(
            addr >= MEM_BEGIN,
            "gem5 request address must be at or above MEM_BEGIN"
        );
        body.sim.canAccept(addr - MEM_BEGIN, is_write)
    })
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
            addr >= MEM_BEGIN,
            "gem5 request address must be at or above MEM_BEGIN"
        );
        assert!(
            payload.payload_sz_bytes as usize <= std::mem::size_of_val(&payload.dword_payload),
            "PESim payload cannot exceed 64 bytes"
        );
        let sim_addr = addr - MEM_BEGIN;

        if !body.sim.canAccept(sim_addr, is_write) {
            return false;
        }

        body.sim.enqueue_with_data(
            sim_addr,
            payload.dword_payload,
            payload.payload_sz_bytes,
            is_write,
        );
        body.enqueued += 1;
        true
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
        // TODO: expose completion payload through the FFI result. OP_CGO_QUERY writes
        // its 0/1 result into dram_req.payload[0], but PEsim_rs_MemReq cannot return it yet.
        PEsim_rs_MemReq {
            addr: req.get_addr() + MEM_BEGIN,
            issue_time: req.get_issue_time().unwrap_or(0),
            is_write: !req.is_read(),
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
