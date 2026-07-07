use crate::cpu::RF::arch_rf;
use crate::cpu::imem::IMEM;
use crate::cpu::pimcpu_types::{fatptr_rf, inst};
use crate::memory::AGU_unit::{AGU_ENTRY_COUNT, AGU_unit};
use crate::memory::flat_memory::{PIM_ENTRIES_PER_CACHELINE, cpu_flat_mem};
use crate::memory::mem_portal::{dram_portal, dram_req, portal_mode};
use crate::sim_engine::engine_alloc::PSEUDO_BANK_ENTRIES;

const DESCRIPTORS_PER_CHUNK: usize = 2;
const AGU_TABLE_CHUNKS: u32 = (AGU_ENTRY_COUNT / DESCRIPTORS_PER_CHUNK) as u32;
const INSTRUCTIONS_PER_CHUNK: u32 = 8;
const MAX_IMEM_INSTRUCTIONS: u32 = 4096;
const MAX_PROGRAM_CHUNKS: u32 = MAX_IMEM_INSTRUCTIONS / INSTRUCTIONS_PER_CHUNK;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CPU_boot_FSM_states {
    IDLE,
    DRAM2AGU_submit,
    DRAM2AGU_stall,
    ReadInsts,
    DRAM2IMEM_submit,
    DRAM2IMEM_stall,
    Finished,
}

pub struct CPU_boot_FSM {
    state: CPU_boot_FSM_states,
    pending_reqs: Vec<dram_req>,
    program_base: u32,
    program_chunks: u32,
    dram_port: dram_portal,
    ch: u64,
    ra: u64,
    bg: u64,
    ba: u64,
    pseudo_bank: u64,
}

impl CPU_boot_FSM {
    pub fn new(
        dram_port: dram_portal,
        ch: u64,
        ra: u64,
        bg: u64,
        ba: u64,
        pseudo_bank: u64,
    ) -> Self {
        Self {
            state: CPU_boot_FSM_states::IDLE,
            pending_reqs: Vec::new(),
            program_base: 0,
            program_chunks: 0,
            dram_port,
            ch,
            ra,
            bg,
            ba,
            pseudo_bank,
        }
    }

    /// Arms an idle controller. Returns false for every irregular repeated start.
    pub fn set_on(&mut self) -> bool {
        if self.state != CPU_boot_FSM_states::IDLE {
            return false;
        }

        self.state = CPU_boot_FSM_states::DRAM2AGU_submit;
        true
    }

    pub fn has_finished(&self) -> bool {
        self.state == CPU_boot_FSM_states::Finished
    }

    pub fn tick(
        &mut self,
        fmem: &cpu_flat_mem,
        agu: &mut AGU_unit,
        imem: &mut IMEM,
        rf: &mut arch_rf,
    ) {
        match self.state {
            CPU_boot_FSM_states::IDLE | CPU_boot_FSM_states::Finished => {}
            CPU_boot_FSM_states::DRAM2AGU_submit => {
                self.submit_chunk_reads(0, AGU_TABLE_CHUNKS);
                self.state = CPU_boot_FSM_states::DRAM2AGU_stall;
            }
            CPU_boot_FSM_states::DRAM2AGU_stall => {
                if self.phase_complete() {
                    self.load_agu_table(fmem, agu, rf);
                    self.state = CPU_boot_FSM_states::ReadInsts;
                }
            }
            CPU_boot_FSM_states::ReadInsts => {
                let (base, bound) = agu
                    .get_entry(0)
                    .unwrap_or_else(|| self.fatal("missing_program_descriptor", "AGU entry 0"));
                if bound > MAX_PROGRAM_CHUNKS {
                    self.fatal(
                        "program_too_large",
                        format_args!("chunks={bound} maximum={MAX_PROGRAM_CHUNKS}"),
                    );
                }

                self.program_base = base;
                self.program_chunks = bound;
                self.state = CPU_boot_FSM_states::DRAM2IMEM_submit;
            }
            CPU_boot_FSM_states::DRAM2IMEM_submit => {
                self.submit_chunk_reads(self.program_base, self.program_chunks);
                self.state = CPU_boot_FSM_states::DRAM2IMEM_stall;
            }
            CPU_boot_FSM_states::DRAM2IMEM_stall => {
                if self.phase_complete() {
                    self.load_program(fmem, imem);
                    self.state = CPU_boot_FSM_states::Finished;
                }
            }
        }
    }

    fn submit_chunk_reads(&mut self, first_chunk: u32, chunk_count: u32) {
        assert!(
            self.pending_reqs.is_empty(),
            "cannot submit a new boot phase with pending requests"
        );

        let end_chunk = first_chunk
            .checked_add(chunk_count)
            .unwrap_or_else(|| self.fatal("boot_range_overflow", "chunk range"));
        let requests = (first_chunk..end_chunk)
            .map(|chunk_addr| {
                dram_req::new(
                    u64::from(chunk_addr) / PIM_ENTRIES_PER_CACHELINE,
                    true,
                    true,
                )
            })
            .collect::<Vec<_>>();

        // dram_portal is stack-backed. Reverse enqueueing preserves ascending
        // logical chunk issue order when Engine drains it with pop().
        for req in requests.iter().rev() {
            self.dram_port.submit(req.clone());
        }
        self.pending_reqs = requests;
    }

    fn phase_complete(&mut self) -> bool {
        let mut idx = 0;
        while idx < self.pending_reqs.len() {
            if self
                .dram_port
                .take_completed(&self.pending_reqs[idx])
                .is_some()
            {
                self.pending_reqs.swap_remove(idx);
            } else {
                idx += 1;
            }
        }

        self.dram_port.req_drained_for_mode(portal_mode::PIM) && self.pending_reqs.is_empty()
    }

    /*
     * NB
     * Here is the AGU table loading rule.
     * The booter reads the AGU table from flat memory, installs every descriptor,
     * then initializes fptr registers 0..7 to {tag: register_id, offset: 0}.
     *
     * A useful vector header pattern is to put an encoded fat pointer in the
     * first chunk of the descriptor range, set that fat pointer to
     * {tag: same_entry, offset: 1}, set the descriptor bound to
     * vector_length + 1, then perform FatPtrLd N, N before the first data load.
     * Using vector_length as the bound would make the final data chunk fail the
     * AGU offset < bound check.
     */
    fn load_agu_table(&self, fmem: &cpu_flat_mem, agu: &mut AGU_unit, rf: &mut arch_rf) {
        let mut descriptors = [(0_u32, 0_u32); AGU_ENTRY_COUNT];

        for chunk_addr in 0..AGU_TABLE_CHUNKS {
            let words = fmem.mem_read_data(chunk_addr).unwrap_or_else(|| {
                self.fatal(
                    "boot_memory_type_mismatch",
                    format_args!("chunk={chunk_addr}"),
                )
            });
            let first_entry = chunk_addr as usize * DESCRIPTORS_PER_CHUNK;
            descriptors[first_entry] = (words[0], words[1]);
            descriptors[first_entry + 1] = (words[2], words[3]);
        }

        for (id, (base, bound)) in descriptors.iter().copied().enumerate() {
            let Some(end) = base.checked_add(bound) else {
                self.fatal(
                    "descriptor_range_overflow",
                    format_args!("entry={id} base={base} bound={bound}"),
                );
            };
            if u64::from(end) > PSEUDO_BANK_ENTRIES {
                self.fatal(
                    "descriptor_out_of_bounds",
                    format_args!(
                        "entry={id} base={base} bound={bound} valid_entries=0..{PSEUDO_BANK_ENTRIES}"
                    ),
                );
            }
        }

        for (id, (base, bound)) in descriptors.into_iter().enumerate() {
            agu.insert(id as u8, base, bound);
        }
        for id in 0_u8..8 {
            rf.write_fregs(id, fatptr_rf::new(id, 0));
        }
    }

    fn load_program(&self, fmem: &cpu_flat_mem, imem: &mut IMEM) {
        let end_chunk = self
            .program_base
            .checked_add(self.program_chunks)
            .unwrap_or_else(|| self.fatal("program_range_overflow", "instruction image"));
        let mut program = Vec::with_capacity(self.program_chunks as usize * 8);

        for chunk_addr in self.program_base..end_chunk {
            let words = fmem.mem_read_data(chunk_addr).unwrap_or_else(|| {
                self.fatal(
                    "boot_memory_type_mismatch",
                    format_args!("chunk={chunk_addr}"),
                )
            });

            for word in words {
                for encoded in [word as u16, (word >> 16) as u16] {
                    let instruction = Self::decode_instruction(encoded).unwrap_or_else(|opcode| {
                        self.fatal(
                            "invalid_instruction_opcode",
                            format_args!(
                                "chunk={chunk_addr} encoded={encoded:#06x} opcode={opcode:#x}"
                            ),
                        )
                    });
                    program.push(instruction);
                }
            }
        }

        imem.flash_in(&program);
    }

    fn decode_instruction(encoded: u16) -> Result<inst, u8> {
        let opcode = ((encoded >> 12) & 0xf) as u8;
        let field_a = ((encoded >> 9) & 0x7) as u8;
        let field_b = ((encoded >> 6) & 0x7) as u8;
        let field_c = ((encoded >> 3) & 0x7) as u8;
        let field_d = (encoded & 0x7) as u8;

        match opcode {
            0x0 => Ok(inst::NOP),
            0x1 => Ok(inst::ADD128 {
                rd: field_a,
                rs1: field_b,
                rs2: field_c,
            }),
            0x2 => Ok(inst::SUB128 {
                rd: field_a,
                rs1: field_b,
                rs2: field_c,
            }),
            0x3 => Ok(inst::MUL128 {
                rd: field_a,
                rs1: field_b,
                rs2: field_c,
            }),
            0x4 => Ok(inst::AND128 {
                rd: field_a,
                rs1: field_b,
                rs2: field_c,
            }),
            0x5 => Ok(inst::LD128 {
                rd: field_a,
                frs: field_b,
            }),
            0x6 => Ok(inst::ST128 {
                rs: field_a,
                frd: field_b,
            }),
            0x7 => Ok(inst::FatPtrLD {
                frd: field_a,
                frs: field_b,
            }),
            0x8 => Ok(inst::FatPtrST {
                frd: field_a,
                frs: field_b,
            }),
            0x9 => Ok(inst::FatPtrADD {
                frd: field_a,
                frs: field_b,
                rs1: field_c,
                imm_idx: field_d,
            }),
            0xa => Ok(inst::FatPtrSUB {
                frd: field_a,
                frs: field_b,
                rs1: field_c,
                imm_idx: field_d,
            }),
            0xb => Ok(inst::JUMP {
                inst_imm: encoded & 0x0fff,
            }),
            0xc => Ok(inst::EqualExit {
                rd: field_b,
                rs1: field_a,
            }),
            _ => Err(opcode),
        }
    }

    #[cold]
    fn fatal(&self, reason: &str, detail: impl std::fmt::Display) -> ! {
        eprintln!(
            "PIM_FATAL reason=cgo_boot_{reason} ch={} rank={} bank_group={} bank={} pseudo_bank={} detail={detail}",
            self.ch, self.ra, self.bg, self.ba, self.pseudo_bank
        );

        #[cfg(test)]
        panic!("CGO boot failed: {reason}");

        #[cfg(not(test))]
        std::process::abort();
    }
}


/*
 * FIXME
 * Move boot fsm testcase into boot_fsm_test.rs
 */
#[cfg(test)]
mod tests {
    use super::*;

    fn encode(opcode: u16, a: u16, b: u16, c: u16, d: u16) -> u16 {
        (opcode << 12) | (a << 9) | (b << 6) | (c << 3) | d
    }

    #[test]
    fn boot_waits_for_every_timed_response_and_preserves_issue_order() {
        let mut engine_port = dram_portal::new();
        let mut boot = CPU_boot_FSM::new(engine_port.clone(), 0, 0, 0, 1, 0);
        let mut fmem = cpu_flat_mem::new();
        let mut agu = AGU_unit::new();
        let mut imem = IMEM::new();
        let mut rf = arch_rf::new();

        // Entry zero describes one instruction chunk at chunk address eight.
        fmem.mem_write_data(0, &[8, 1, 0, 0]).unwrap();
        for chunk in 1..AGU_TABLE_CHUNKS {
            fmem.mem_write_data(chunk, &[0; 4]).unwrap();
        }
        let add = encode(0x1, 3, 1, 2, 0);
        let exit = encode(0xc, 3, 3, 0, 0);
        fmem.mem_write_data(8, &[u32::from(add) | (u32::from(exit) << 16), 0, 0, 0])
            .unwrap();

        assert!(boot.set_on());
        assert!(!boot.set_on(), "a repeated start must not reset boot");
        assert!(
            engine_port.get_one_req().is_none(),
            "arming boot must not submit reads until the next FSM tick"
        );
        boot.tick(&fmem, &mut agu, &mut imem, &mut rf);
        assert_eq!(boot.state, CPU_boot_FSM_states::DRAM2AGU_stall);

        let mut descriptor_reqs = Vec::new();
        while let Some(req) = engine_port.get_one_req() {
            descriptor_reqs.push(req);
        }
        assert_eq!(
            descriptor_reqs
                .iter()
                .map(dram_req::get_addr)
                .collect::<Vec<_>>(),
            vec![0, 0, 0, 0, 1, 1, 1, 1]
        );

        // An empty request vector is not enough: all timing responses are required.
        boot.tick(&fmem, &mut agu, &mut imem, &mut rf);
        assert_eq!(boot.state, CPU_boot_FSM_states::DRAM2AGU_stall);
        for req in descriptor_reqs.iter().take(7) {
            engine_port.complete(req.clone());
        }
        boot.tick(&fmem, &mut agu, &mut imem, &mut rf);
        assert_eq!(boot.state, CPU_boot_FSM_states::DRAM2AGU_stall);

        engine_port.complete(descriptor_reqs[7].clone());
        boot.tick(&fmem, &mut agu, &mut imem, &mut rf);
        assert_eq!(boot.state, CPU_boot_FSM_states::ReadInsts);
        assert_eq!(agu.get_entry(0), Some((8, 1)));
        for id in 0_u8..8 {
            assert_eq!(rf.read_fregs(id), Some(fatptr_rf::new(id, 0)));
        }

        boot.tick(&fmem, &mut agu, &mut imem, &mut rf);
        assert_eq!(boot.state, CPU_boot_FSM_states::DRAM2IMEM_submit);
        boot.tick(&fmem, &mut agu, &mut imem, &mut rf);
        assert_eq!(boot.state, CPU_boot_FSM_states::DRAM2IMEM_stall);

        let program_req = engine_port
            .get_one_req()
            .expect("one instruction chunk should issue one timed read");
        assert_eq!(program_req.get_addr(), 2);
        assert!(engine_port.get_one_req().is_none());

        boot.tick(&fmem, &mut agu, &mut imem, &mut rf);
        assert!(!boot.has_finished());
        engine_port.complete(program_req);
        boot.tick(&fmem, &mut agu, &mut imem, &mut rf);
        assert!(boot.has_finished());

        assert!(matches!(
            imem.read_inst(0),
            Some(inst::ADD128 {
                rd: 3,
                rs1: 1,
                rs2: 2
            })
        ));
        assert!(matches!(
            imem.read_inst(1),
            Some(inst::EqualExit { rd: 3, rs1: 3 })
        ));
        assert!(matches!(imem.read_inst(7), Some(inst::NOP)));
    }

    #[test]
    fn decoder_uses_the_documented_bit_layout() {
        assert!(matches!(
            CPU_boot_FSM::decode_instruction(encode(0x9, 1, 2, 3, 2)),
            Ok(inst::FatPtrADD {
                frd: 1,
                frs: 2,
                rs1: 3,
                imm_idx: 2
            })
        ));
        assert!(matches!(
            CPU_boot_FSM::decode_instruction(0xbabc),
            Ok(inst::JUMP { inst_imm: 0xabc })
        ));
        assert!(matches!(
            CPU_boot_FSM::decode_instruction(encode(0xc, 4, 5, 0, 0)),
            Ok(inst::EqualExit { rs1: 4, rd: 5 })
        ));
        assert_eq!(CPU_boot_FSM::decode_instruction(0xd000).err(), Some(0xd));
    }
}
