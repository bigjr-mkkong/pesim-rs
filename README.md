## Cycle-accurate bank level PE simulator for HBM-PIM / UPMEM liked architecture

### Clone:
```
git clone git@github.com:bigjr-mkkong/pesim-rs.git
```

### Build:

Refer to instructions in third-party/ReadMeFirst to clone modified DRAMSim3, then run:

```
cargo build --release --lib
```

### Test:

After Build the project, run:

```
cargo test
```

To see test result


## Next step:
1. Integration test with gem5
2. Figure out actual memory mapping
3. Test with simple payload
