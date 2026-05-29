use crate::cpu::pimcpu_types::CPU_stages;
use std::collections::{BTreeMap, HashMap, HashSet};

#[derive(Copy, Clone, PartialEq, Eq, Hash)]
pub enum signal_reason {
    jump_resolution,
    RAW_resolution,
    MEM_block,
    external_pause,
    exception,
    prog_end,
    no_reason,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum pipeline_action {
    Normal,
    Flush,
    Stall,
    END,
}

impl pipeline_action {
    fn get_rank(&self) -> u8 {
        match self {
            pipeline_action::Normal => 1,
            pipeline_action::Flush => 2,
            pipeline_action::Stall => 3,
            pipeline_action::END => 4,
        }
    }
}

impl PartialOrd for pipeline_action {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for pipeline_action {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.get_rank().cmp(&&other.get_rank())
    }
}

pub struct signal_req {
    sig_reason: signal_reason,
    issuer_stage: CPU_stages,
    target_stages: Option<HashSet<CPU_stages>>,
}

impl signal_req {
    pub fn new(
        reason: signal_reason,
        issuer: CPU_stages,
        target: Option<HashSet<CPU_stages>>,
    ) -> Self {
        Self {
            sig_reason: reason,
            issuer_stage: issuer,
            target_stages: target,
        }
    }
}

pub trait SigFSM: SigFSMClone {
    fn reason(&self) -> signal_reason;

    //action should return Normal when reaching the finish state
    fn action(&self) -> pipeline_action;

    fn get_ops(&self) -> HashMap<CPU_stages, pipeline_action>;
    fn advance_winner(&mut self) -> bool;
    fn handle_blocked(&mut self) {}
}

pub struct sig_resolver {
    fsm_menu: HashMap<signal_reason, Box<dyn SigFSM>>,
    active_sig: BTreeMap<(CPU_stages, pipeline_action), Box<dyn SigFSM>>,
}

pub trait SigFSMClone {
    fn clone_box(&self) -> Box<dyn SigFSM>;
}

impl<T> SigFSMClone for T
where
    T: 'static + SigFSM + Clone,
{
    fn clone_box(&self) -> Box<dyn SigFSM> {
        Box::new(self.clone())
    }
}

impl Clone for Box<dyn SigFSM> {
    fn clone(&self) -> Box<dyn SigFSM> {
        self.clone_box()
    }
}

impl sig_resolver {
    pub fn new() -> Self {
        Self {
            fsm_menu: HashMap::new(),
            active_sig: BTreeMap::new(),
        }
    }

    pub fn add_new_fsm(&mut self, sig_reason: signal_reason, new_fsm: Box<dyn SigFSM>) -> bool {
        match self.fsm_menu.entry(sig_reason) {
            std::collections::hash_map::Entry::Occupied(_) => false,
            std::collections::hash_map::Entry::Vacant(v) => {
                v.insert(new_fsm);
                true
            }
        }
    }

    pub fn submit_signal(&mut self, sig_req: Option<signal_req>) {
        if let Some(req) = sig_req {
            if let Some(template_fsm) = self.fsm_menu.get(&req.sig_reason) {
                let new_fsm = template_fsm.clone();
                let act = new_fsm.action();
                let key = (req.issuer_stage, act);
                self.active_sig.insert(key, new_fsm);
            }
        }
    }

    fn collect_result(&mut self) -> HashMap<CPU_stages, pipeline_action> {
        let mut ret = HashMap::from([
            (CPU_stages::IF, pipeline_action::Normal),
            (CPU_stages::ID, pipeline_action::Normal),
            (CPU_stages::EX, pipeline_action::Normal),
            (CPU_stages::AGU, pipeline_action::Normal),
            (CPU_stages::MEM, pipeline_action::Normal),
            (CPU_stages::WB, pipeline_action::Normal),
        ]);

        let mut iter = self.active_sig.iter_mut();
        let result = if let Some((_, champ_fsm)) = iter.next_back() {
            let ret = champ_fsm.get_ops();
            champ_fsm.advance_winner();
            Some(ret)
        } else {
            None
        };

        ret.extend(result.into_iter().flatten());

        for ((_, _), _fsm) in iter {
            _fsm.handle_blocked();
        }

        ret
    }

    fn update_active(&mut self) {
        self.active_sig
            .retain(|_, fsm| fsm.action() != pipeline_action::Normal);
    }

    pub fn get_decision(&mut self) -> HashMap<CPU_stages, pipeline_action> {
        let ret = self.collect_result();
        self.update_active();

        return ret;
    }
}
