use crate::cpu::RF::arch_rf;
use crate::cpu::imem::IMEM;
use crate::cpu::pimcpu_types::{CPU_stages, arch_action};
use std::cell::RefCell;
use std::rc::Rc;

use crate::cpu::AGU::{AGU_MEM_rf, AGU_stop_FSM};
use crate::cpu::EX::{EX_AGU_rf, EX_stop_FSM, RAW_resolution_FSM};
use crate::cpu::ID::{ID_EX_rf, ID_jump_FSM};
use crate::cpu::IF::IF_ID_rf;
use crate::cpu::MEM::{MEM_WB_RF, MEM_stop_FSM};
use crate::cpu::signal_scoreboard::{
    ExternalPause_FSM, pipeline_action, sig_resolver, signal_reason, signal_req,
};
use crate::memory::AGU_unit::AGU_unit;
use crate::memory::flat_memory::cpu_flat_mem;
use crate::memory::mem_portal::dram_portal;

pub const PC_TESTING: u16 = 0xffff;

pub struct CPU {
    pub(crate) imem: IMEM,
    pub(crate) RF: arch_rf,

    pub(crate) if_id_rf: IF_ID_rf,
    pub(crate) id_ex_rf: ID_EX_rf,
    pub(crate) ex_agu_rf: EX_AGU_rf,
    pub(crate) agu_mem_rf: AGU_MEM_rf,
    pub(crate) mem_wb_rf: MEM_WB_RF,
    pub(crate) wb_forward_rf: MEM_WB_RF,
    pub(crate) pipeline_ctrl: sig_resolver,
    pub(crate) agu: AGU_unit,
    pub(crate) fmem: cpu_flat_mem,
    ext_pause_requested: bool,
    ready4ext_sig: bool,
    pause_ready_delay_cycles: u64,
    resume_ready_delay_cycles: u64,
    ext_signal_delay_remaining: u64,
    pause_ready_delay_started: bool,
}

impl CPU {
    fn build(mem_stop_fsm: MEM_stop_FSM) -> Self {
        let mut pipeline_ctrl = sig_resolver::new();
        pipeline_ctrl.add_new_fsm(signal_reason::jump_resolution, Box::new(ID_jump_FSM::new()));
        pipeline_ctrl.add_new_fsm(signal_reason::prog_end, Box::new(EX_stop_FSM::new()));
        pipeline_ctrl.add_new_fsm(signal_reason::exception, Box::new(AGU_stop_FSM::new()));
        pipeline_ctrl.add_new_fsm(signal_reason::mem_block_kind(), Box::new(mem_stop_fsm));
        pipeline_ctrl.add_new_fsm(
            signal_reason::external_pause,
            Box::new(ExternalPause_FSM::new()),
        );
        pipeline_ctrl.add_new_fsm(
            signal_reason::RAW_resolution,
            Box::new(RAW_resolution_FSM::new()),
        );

        Self {
            imem: IMEM::new(),
            RF: arch_rf::new(),
            if_id_rf: IF_ID_rf::new(),
            id_ex_rf: ID_EX_rf::new(),
            ex_agu_rf: EX_AGU_rf::new(),
            agu_mem_rf: AGU_MEM_rf::new(),
            mem_wb_rf: MEM_WB_RF::new(),
            wb_forward_rf: MEM_WB_RF::new(),
            pipeline_ctrl,
            agu: AGU_unit::new(),
            fmem: cpu_flat_mem::new(),
            ext_pause_requested: false,
            ready4ext_sig: true,
            pause_ready_delay_cycles: 0,
            resume_ready_delay_cycles: 0,
            ext_signal_delay_remaining: 0,
            pause_ready_delay_started: false,
        }
    }

    pub fn new() -> Self {
        Self::build(MEM_stop_FSM::new())
    }

    pub fn new_with_dram_port(dram_port: dram_portal) -> Self {
        Self::build(MEM_stop_FSM::new_with_dram_port(dram_port))
    }

    pub fn get_RF(&mut self) -> &mut arch_rf {
        &mut self.RF
    }

    pub fn get_fmem(&mut self) -> &mut cpu_flat_mem {
        &mut self.fmem
    }

    pub fn get_imem(&mut self) -> &mut IMEM {
        &mut self.imem
    }

    pub fn get_agu(&mut self) -> &mut AGU_unit {
        &mut self.agu
    }

    pub fn signal_pause(&mut self) {
        self.ext_pause_requested = true;
        self.ready4ext_sig = false;
        self.pause_ready_delay_started = false;
        self.ext_signal_delay_remaining = 0;
    }

    pub fn signal_resume(&mut self) {
        self.ext_pause_requested = false;
        self.pause_ready_delay_started = false;
        self.ext_signal_delay_remaining = self.resume_ready_delay_cycles;
        self.ready4ext_sig = self.resume_ready_delay_cycles == 0;
        self.pipeline_ctrl
            .clear_active_signal(signal_reason::external_pause);
    }

    // This function will let engine to set fast-switch parameter or regular PREC+ACT
    pub fn set_external_signal_delays(&mut self, pause_cycles: u64, resume_cycles: u64) {
        self.pause_ready_delay_cycles = pause_cycles;
        self.resume_ready_delay_cycles = resume_cycles;
    }

    pub fn ready4signal(&self) -> bool {
        self.ready4ext_sig
    }

    fn update_extsig_rdy(&mut self, winner_reason: Option<signal_reason>) {
        if self.ext_signal_delay_remaining > 0 {
            self.ext_signal_delay_remaining -= 1;
            self.ready4ext_sig = false;
            return;
        }

        if self.ext_pause_requested {
            if !self.pause_ready_delay_started {
                if winner_reason == Some(signal_reason::external_pause) {
                    self.pause_ready_delay_started = true;
                    self.ext_signal_delay_remaining = self.pause_ready_delay_cycles;
                } else {
                    self.ready4ext_sig = false;
                    return;
                }
            }

            if self.ext_signal_delay_remaining > 0 {
                self.ext_signal_delay_remaining -= 1;
                self.ready4ext_sig = false;
            } else {
                self.ready4ext_sig = true;
            }
        } else {
            self.ready4ext_sig = true;
        }
    }

    fn maybe_pause_stage_result(
        &self,
        sig_req: signal_req,
        arch_ops: Vec<arch_action>,
        issuer: CPU_stages,
    ) -> (signal_req, Vec<arch_action>) {
        match sig_req.get_reason() {
            signal_reason::exception | signal_reason::prog_end => (sig_req, arch_ops),
            _ if self.ext_pause_requested => {
                let masked_arch_ops = match issuer {
                    CPU_stages::IF => [arch_action::HoldPC].to_vec(),
                    _ => [arch_action::DoNothing].to_vec(),
                };

                (
                    signal_req::new(signal_reason::external_pause, issuer, None),
                    masked_arch_ops,
                )
            }
            _ => (sig_req, arch_ops),
        }
    }

    pub fn tick(&mut self) {
        let (_, wb_sigreq, wb_archop) = self.eval_WB(&self.mem_wb_rf);
        self.pipeline_ctrl.submit_signal(Some(wb_sigreq));

        let (mem_wb_next, mem_sigreq, mem_archop) = self.eval_MEM(&self.agu_mem_rf, &self.fmem);
        self.pipeline_ctrl.submit_signal(Some(mem_sigreq));

        let (agu_mem_next, agu_sigreq, agu_archop) = self.eval_AGU(&self.ex_agu_rf, &self.agu);
        let (agu_sigreq, agu_archop) =
            self.maybe_pause_stage_result(agu_sigreq, agu_archop, CPU_stages::AGU);
        self.pipeline_ctrl.submit_signal(Some(agu_sigreq));

        let (ex_agu_next, ex_sigreq, ex_archop) = self.eval_EX(&self.id_ex_rf);
        let (ex_sigreq, ex_archop) =
            self.maybe_pause_stage_result(ex_sigreq, ex_archop, CPU_stages::EX);
        self.pipeline_ctrl.submit_signal(Some(ex_sigreq));

        let (id_ex_next, id_sigreq, id_archop) = self.eval_ID(&self.if_id_rf, &self.RF);
        let (id_sigreq, id_archop) =
            self.maybe_pause_stage_result(id_sigreq, id_archop, CPU_stages::ID);
        self.pipeline_ctrl.submit_signal(Some(id_sigreq));

        let (if_id_next, if_sigreq, if_archop) = self.eval_IF(&self.RF, &self.imem);
        let (if_sigreq, if_archop) =
            self.maybe_pause_stage_result(if_sigreq, if_archop, CPU_stages::IF);
        self.pipeline_ctrl.submit_signal(Some(if_sigreq));

        let pipeline_op = self.pipeline_ctrl.get_decision();
        let winner_reason = self.pipeline_ctrl.last_winner_reason();

        // This function will update self.ready4sig() according to defined delay cycle
        // It introduced a fixed cycle delay after MEM has finished.
        // This is to simulate the PREC+ACT delay when switch between PIM and MEM
        self.update_extsig_rdy(winner_reason);

        let stage_action = |stage| {
            pipeline_op
                .get(&stage)
                .copied()
                .unwrap_or(pipeline_action::Normal)
        };

        let mut arch_ops = Vec::new();
        let mut collect_stage_ops = |stage, ops: Vec<arch_action>| {
            if stage_action(stage) == pipeline_action::Normal {
                arch_ops.extend(ops);
            }
        };

        collect_stage_ops(CPU_stages::WB, wb_archop);
        collect_stage_ops(CPU_stages::MEM, mem_archop);
        collect_stage_ops(CPU_stages::AGU, agu_archop);
        collect_stage_ops(CPU_stages::EX, ex_archop);
        collect_stage_ops(CPU_stages::ID, id_archop);
        collect_stage_ops(CPU_stages::IF, if_archop);

        self.arch_update(arch_ops);

        /*
         * Update to stage register XY should consider the action for both X and Y
         *  X           Y           pipeline_action
         *  z           Stall       Stall
         *  Normal      Normal      Normal
         *  Stall       Normal      Flush
         *  z           Flush       Flush
         *  Flush       z           Flush
         */

        let stage_op = |producer_act, consumer_act| match (producer_act, consumer_act) {
            (_, pipeline_action::Stall) => pipeline_action::Stall,

            (pipeline_action::Normal, pipeline_action::Normal) => pipeline_action::Normal,

            (pipeline_action::Stall, pipeline_action::Normal) => pipeline_action::Flush,

            (_, pipeline_action::Flush | pipeline_action::END) => pipeline_action::Flush,

            (pipeline_action::Flush | pipeline_action::END, _) => pipeline_action::Flush,
        };

        match stage_op(stage_action(CPU_stages::MEM), stage_action(CPU_stages::WB)) {
            pipeline_action::Normal => {
                self.mem_wb_rf = mem_wb_next;
                if self.mem_wb_rf.is_valid() {
                    self.wb_forward_rf = self.mem_wb_rf;
                }
            }
            pipeline_action::Stall => {}
            pipeline_action::Flush | pipeline_action::END => self.mem_wb_rf.invalidate(),
        }

        match stage_op(stage_action(CPU_stages::AGU), stage_action(CPU_stages::MEM)) {
            pipeline_action::Normal => self.agu_mem_rf = agu_mem_next,
            pipeline_action::Stall => {}
            pipeline_action::Flush | pipeline_action::END => self.agu_mem_rf.invalidate(),
        }

        match stage_op(stage_action(CPU_stages::EX), stage_action(CPU_stages::AGU)) {
            pipeline_action::Normal => self.ex_agu_rf = ex_agu_next,
            pipeline_action::Stall => {}
            pipeline_action::Flush | pipeline_action::END => self.ex_agu_rf.invalidate(),
        }

        match stage_op(stage_action(CPU_stages::ID), stage_action(CPU_stages::EX)) {
            pipeline_action::Normal => self.id_ex_rf = id_ex_next,
            pipeline_action::Stall => {}
            pipeline_action::Flush | pipeline_action::END => self.id_ex_rf.invalidate(),
        }

        match stage_op(stage_action(CPU_stages::IF), stage_action(CPU_stages::ID)) {
            pipeline_action::Normal => self.if_id_rf = if_id_next,
            pipeline_action::Stall => {}
            pipeline_action::Flush | pipeline_action::END => self.if_id_rf.invalidate(),
        }
    }
}
