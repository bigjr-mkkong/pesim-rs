# CGO Boot ABI

This document describes the host-visible memory contract consumed by
`CPU_boot_FSM` in `src/cpu/boot_fsm.rs`.

## Address Unit

The CGO boot ABI is expressed in flat-memory chunks. One chunk is one
`[u8; 16]` entry in `src/memory/flat_memory.rs`.

Host writes arrive as 64-byte cache lines. `Engine::mirror_host_write` maps the
global host address into an engine-local pseudo-bank address, and
`flat_mem::mirror_host_write` splits each cache line into four 16-byte chunks.

## Start Condition

`CGO_Start` does not itself carry a program address. It only arms the boot FSM
through `CPU_boot_FSM::set_on`.

On later engine ticks, `CPU_boot_FSM::tick` fetches the boot image from the
engine's mirrored flat memory. A repeated or otherwise irregular start is
reported by the engine and does not reset an already armed boot FSM.

## AGU Table

The AGU table begins at local chunk `0`.

There are 16 AGU descriptors. Each descriptor is:

```text
u32 base
u32 bound
```

Two descriptors are packed into each 16-byte chunk:

```text
chunk 0: entry 0 base, entry 0 bound, entry 1 base, entry 1 bound
chunk 1: entry 2 base, entry 2 bound, entry 3 base, entry 3 bound
...
chunk 7: entry 14 base, entry 14 bound, entry 15 base, entry 15 bound
```

The boot FSM reads chunks `0..8`, decodes them as CPU data words, validates
every `base + bound` range against the pseudo-bank size, and installs the
entries with `AGU_unit::insert`.

`bound` is relative to `base`. An AGU access is valid when:

```text
fatptr.offset < descriptor.bound
```

The translated chunk address is:

```text
descriptor.base + fatptr.offset
```

## Initial Fat-Pointer Registers

After installing the AGU table, the boot FSM initializes fat-pointer registers
`f0..f7` as:

```text
fN = { tag: N, offset: 0 }
```

The register does not contain the descriptor base. The tag selects an AGU
entry, and the offset is interpreted relative to that entry.

## Program Descriptor

AGU entry `0` is the program descriptor.

`CPU_boot_FSM::tick` reads entry `0` after loading the AGU table:

```text
program_base   = AGU[0].base
program_chunks = AGU[0].bound
```

`program_chunks` must be no larger than 512 chunks, which corresponds to 4096
16-bit instructions.

## Program Image

The program image is stored in flat-memory chunks starting at `program_base`.
The boot FSM reads `program_chunks` chunks and decodes each chunk as four
`u32` words. Each word contains two tightly packed 16-bit instructions:

```text
word bits  0..15: first instruction
word bits 16..31: second instruction
```

One 16-byte chunk therefore contains eight instructions. Padding with zero
encodes `NOP`.

`CPU_boot_FSM::load_program` decodes the image and flashes the resulting
instruction vector into IMEM with `IMEM::flash_in`.

## Timed Fetch Behavior

The boot FSM fetches both the AGU table and the program image through
`dram_portal`.

For each phase, `submit_chunk_reads` submits one timed read per chunk. The
request address is the local cache-line address:

```text
chunk_addr / PIM_ENTRIES_PER_CACHELINE
```

`phase_complete` waits until all submitted requests have completed and the PIM
portal has drained before the boot FSM consumes the mirrored flat-memory
contents.

## Fat-Pointer Header Pattern

A memory-resident fat pointer is encoded as one `u32` in the first four bytes of
a chunk:

```text
bits 31..28: AGU tag
bits 27..0 : relative offset
```

Because boot initializes `fN` to `{ tag: N, offset: 0 }`, a program can use
`FatPtrLD N, N` to load a real data pointer from the first chunk of AGU entry
`N`.

For a vector whose first usable data element is after such a header chunk, the
descriptor must cover both the header and the data:

```text
AGU[N]      = { base: header_chunk, bound: vector_length + 1 }
chunk[base] = encoded fat pointer { tag: N, offset: 1 }
data        = chunks base + 1 through base + vector_length
```

Using `bound = vector_length` with this pattern is incorrect because the last
data chunk at offset `vector_length` would fail the AGU bound check.

## Defining Functions

- `CPU_boot_FSM::set_on` arms the boot sequence.
- `CPU_boot_FSM::tick` defines the boot state order.
- `CPU_boot_FSM::submit_chunk_reads` defines the timed read request addresses.
- `CPU_boot_FSM::phase_complete` defines when a fetch phase may be consumed.
- `CPU_boot_FSM::load_agu_table` defines descriptor layout, validation, AGU
  installation, and initial fat-pointer registers.
- `CPU_boot_FSM::load_program` defines program fetch and instruction packing.
- `CPU_boot_FSM::decode_instruction` defines the 16-bit boot-time instruction
  decoder.
- `Engine::mirror_host_write` maps host writes into per-engine local chunks.
- `flat_mem::mirror_host_write` defines how host cache-line payload bytes become
  mirrored flat-memory chunks.
