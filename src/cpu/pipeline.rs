use crate::cpu::RF::arch_rf;
use crate::cpu::imem::IMEM;
use crate::cpu::pimcpu_types::{CPU_stages, arch_action};

use crate::cpu::AGU::{AGU_MEM_rf, AGU_stop_FSM};
use crate::cpu::EX::{EX_AGU_rf, EX_stop_FSM};
use crate::cpu::ID::{ID_EX_rf, ID_jump_FSM};
use crate::cpu::IF::IF_ID_rf;
use crate::cpu::MEM::{MEM_WB_RF, MEM_stop_FSM};
use crate::cpu::signal_scoreboard::{pipeline_action, sig_resolver, signal_reason};
use crate::memory::AGU_unit::AGU_unit;
use crate::memory::flat_memory::flat_mem;

pub const PC_TESTING: u16 = 0xffff;

pub struct CPU {
    imem: IMEM,
    RF: arch_rf,

    if_id_rf: IF_ID_rf,
    id_ex_rf: ID_EX_rf,
    ex_agu_rf: EX_AGU_rf,
    agu_mem_rf: AGU_MEM_rf,
    mem_wb_rf: MEM_WB_RF,
    pipeline_ctrl: sig_resolver,
    agu: AGU_unit,
    fmem: flat_mem,
}

impl CPU {
    pub fn new() -> Self {
        let mut pipeline_ctrl = sig_resolver::new();
        pipeline_ctrl.add_new_fsm(signal_reason::jump_resolution, Box::new(ID_jump_FSM::new()));
        pipeline_ctrl.add_new_fsm(signal_reason::prog_end, Box::new(EX_stop_FSM::new()));
        pipeline_ctrl.add_new_fsm(signal_reason::exception, Box::new(AGU_stop_FSM::new()));
        pipeline_ctrl.add_new_fsm(signal_reason::MEM_block, Box::new(MEM_stop_FSM::new()));

        Self {
            imem: IMEM::new(),
            RF: arch_rf::new(),
            if_id_rf: IF_ID_rf::new(),
            id_ex_rf: ID_EX_rf::new(),
            ex_agu_rf: EX_AGU_rf::new(),
            agu_mem_rf: AGU_MEM_rf::new(),
            mem_wb_rf: MEM_WB_RF::new(),
            pipeline_ctrl,
            agu: AGU_unit::new(),
            fmem: flat_mem::new(),
        }
    }

    pub fn get_RF(&mut self) -> &mut arch_rf {
        &mut self.RF
    }

    pub fn get_fmem(&mut self) -> &mut flat_mem {
        &mut self.fmem
    }

    pub fn tick(&mut self) {
        let (_, wb_sigreq, wb_archop) = self.eval_WB(&self.mem_wb_rf);
        self.pipeline_ctrl.submit_signal(Some(wb_sigreq));

        let (mem_wb_next, mem_sigreq, mem_archop) = self.eval_MEM(&self.agu_mem_rf, &self.fmem);
        self.pipeline_ctrl.submit_signal(Some(mem_sigreq));

        let (agu_mem_next, agu_sigreq, agu_archop) = self.eval_AGU(&self.ex_agu_rf, &self.agu);
        self.pipeline_ctrl.submit_signal(Some(agu_sigreq));

        let (ex_agu_next, ex_sigreq, ex_archop) = self.eval_EX(&self.id_ex_rf);
        self.pipeline_ctrl.submit_signal(Some(ex_sigreq));

        let (id_ex_next, id_sigreq, id_archop) = self.eval_ID(&self.if_id_rf, &self.RF);
        self.pipeline_ctrl.submit_signal(Some(id_sigreq));

        let (if_id_next, if_sigreq, if_archop) = self.eval_IF(&self.RF, &self.imem);
        self.pipeline_ctrl.submit_signal(Some(if_sigreq));

        let pipeline_op = self.pipeline_ctrl.get_decision();

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

        match stage_action(CPU_stages::MEM) {
            pipeline_action::Normal => self.mem_wb_rf = mem_wb_next,
            pipeline_action::Stall => {}
            pipeline_action::Flush | pipeline_action::END => self.mem_wb_rf.invalidate(),
        }

        match stage_action(CPU_stages::AGU) {
            pipeline_action::Normal => self.agu_mem_rf = agu_mem_next,
            pipeline_action::Stall => {}
            pipeline_action::Flush | pipeline_action::END => self.agu_mem_rf.invalidate(),
        }

        match stage_action(CPU_stages::EX) {
            pipeline_action::Normal => self.ex_agu_rf = ex_agu_next,
            pipeline_action::Stall => {}
            pipeline_action::Flush | pipeline_action::END => self.ex_agu_rf.invalidate(),
        }

        match stage_action(CPU_stages::ID) {
            pipeline_action::Normal => self.id_ex_rf = id_ex_next,
            pipeline_action::Stall => {}
            pipeline_action::Flush | pipeline_action::END => self.id_ex_rf.invalidate(),
        }

        match stage_action(CPU_stages::IF) {
            pipeline_action::Normal => self.if_id_rf = if_id_next,
            pipeline_action::Stall => {}
            pipeline_action::Flush | pipeline_action::END => self.if_id_rf.invalidate(),
        }
    }
}
