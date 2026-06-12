use crate::sim_engine::sim::{Sim, engine_cfg, SimMode};

/*
 * TODO
 * Finish all tests below
 * It's hard to test correctness as simulator is more like a timing simulator instead of function
 * simulator.
 * In this case, to verify correctness, log the finish time for requests and if finish time is
 * positive(>1), mark this request as correct
 */

#[test]
fn sim_hostonly_noengine() {
    /*
     * This test will create a Sim with no engines inside, and it will keep receive memory traces
     * and handle them as host request
     */
}

#[test]
fn sim_pimonly() {
    /*
     * This test will create a Sim with only one engine inside, and run a simple vecadd program
     * No host request will be made
     */
}

#[test]
fn sim_multithread_pimonly() {
    /*
     * This test will create multiple engines and run vecadd on both of them.
     * No host request will be made
     */
}

#[test]
fn sim_pim_host_together() {
    /*
     * This test will create one engine and run vecadd on it.
     * It will also push fake host request into host-only area
     */
}

#[test]
fn sim_pim_host_concurrent() {
    /*
     * This test will create one engine and run vecadd on it.
     * It will also push fake host request into engine's queue
     */
}
