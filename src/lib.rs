#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]

use cpu::pipeline::CPU;
use std::path::PathBuf;

pub const DSIM3_CFG_PATH: &str = "cfg/DDR4_8Gb_x4_2400_pim.ini";
pub const DSIM3_OUT_DIR: &str = "output";

fn dsim3_paths() -> (PathBuf, PathBuf) {
    let executable = std::env::current_exe()
        .unwrap_or_else(|error| panic!("cannot locate the PESim runtime artifact: {error}"));
    let executable_dir = executable.parent().unwrap_or_else(|| {
        panic!(
            "PESim runtime artifact has no parent: {}",
            executable.display()
        )
    });

    // A Rust static library becomes part of its final executable. Resolve assets from that
    // runtime artifact; walking ancestors also handles Cargo test binaries in target/*/deps.
    for base_dir in executable_dir.ancestors() {
        let config_path = base_dir.join(DSIM3_CFG_PATH);
        if config_path.is_file() {
            return (config_path, base_dir.join(DSIM3_OUT_DIR));
        }
    }

    panic!(
        "cannot find {DSIM3_CFG_PATH} relative to PESim runtime artifact {}",
        executable.display()
    );
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

typedef struct PESim_cacheline{
    uint64_t dword_payload[8];
}PESim_cacheline;

typedef struct PESim_body PESim_body;

PESim_body *pesim_new(void);
void pesim_free(PESim_body *sim);

void pesim_print_stats(PESim_body *sim);
void pesim_reset_stats(PESim_body *sim);

bool pesim_canAccept(PESim_body *sim, uint64_t addr, bool is_write);
bool pesim_enqueue_with_data(PESim_body *sim, uint64_t addr, PESim_cacheline payload, bool is_write);

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
pub struct PESim_cacheline {
    pub dword_payload: [u64; 8],
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

fn with_body_mut_void(sim: *mut PESim_body, f: impl FnOnce(&mut PESim_body)) {
    if sim.is_null() {
        return;
    }

    let _ = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: Same ownership contract as with_body_mut.
        f(unsafe { &mut *sim });
    }));
}

#[unsafe(no_mangle)]
pub extern "C" fn pesim_new() -> *mut PESim_body {
    catch_unwind(AssertUnwindSafe(|| {
        Box::into_raw(Box::new(PESim_body::new()))
    }))
    .unwrap_or(std::ptr::null_mut())
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
    with_body_mut_void(sim, |body| {
        println!(
            "PESim stats: ticks={}, enqueued={}, completions_returned={}",
            body.ticks, body.enqueued, body.completions_returned
        );
    });
}

#[unsafe(no_mangle)]
pub extern "C" fn pesim_reset_stats(sim: *mut PESim_body) {
    with_body_mut_void(sim, |body| {
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
    payload: PESim_cacheline,
    is_write: bool,
) -> bool {
    with_body_mut(sim, false, |body| {
        if !body.sim.canAccept(addr, is_write) {
            return false;
        }

        body.sim
            .enqueue_with_data(addr, payload.dword_payload, is_write);
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
        PEsim_rs_MemReq {
            addr: req.get_addr(),
            issue_time: req.get_issue_time().unwrap_or(0),
            is_write: !req.is_read(),
        }
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn pesim_tick(sim: *mut PESim_body) {
    with_body_mut_void(sim, |body| {
        body.sim.tick();
        body.ticks += 1;
    });
}

#[cfg(test)]
#[path = "lib_test.rs"]
mod lib_test;
