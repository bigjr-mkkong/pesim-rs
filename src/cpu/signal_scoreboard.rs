use crate::cpu::pimcpu_types::CPU_stages;
use std::collections::{BTreeMap, HashMap, HashSet};

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
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
        self.get_rank().cmp(&other.get_rank())
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

    pub fn get_reason(&self) -> signal_reason {
        self.sig_reason
    }
}

pub trait SigFSM: SigFSMClone {
    fn reason(&self) -> signal_reason;

    // action is the arbitration class used to rank this signal.  The actual
    // per-stage operation bundle returned by get_ops may contain mixed actions.
    // action should return Normal when reaching the finish state.
    fn action(&self) -> pipeline_action;

    fn get_ops(&self) -> HashMap<CPU_stages, pipeline_action>;
    fn advance_winner(&mut self) -> bool;
    fn handle_blocked(&mut self) {}
}

#[derive(Clone)]
struct active_signal {
    issuer_stage: CPU_stages,
    fsm: Box<dyn SigFSM>,
    target_stages: Option<HashSet<CPU_stages>>,
}

pub struct sig_resolver {
    fsm_menu: HashMap<signal_reason, Box<dyn SigFSM>>,
    active_sig: BTreeMap<(u8, CPU_stages, pipeline_action, signal_reason), active_signal>,
    last_winner_reason: Option<signal_reason>,
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

fn signal_priority(sig_reason: signal_reason) -> u8 {
    match sig_reason {
        signal_reason::no_reason => 0,
        signal_reason::jump_resolution => 10,
        signal_reason::RAW_resolution => 20,
        signal_reason::external_pause => 30,
        signal_reason::MEM_block => 40,
        signal_reason::exception => 50,
        signal_reason::prog_end => 50,
    }
}

impl sig_resolver {
    pub fn new() -> Self {
        Self {
            fsm_menu: HashMap::new(),
            active_sig: BTreeMap::new(),
            last_winner_reason: None,
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
        let Some(req) = sig_req else {
            return;
        };

        if req.sig_reason == signal_reason::no_reason {
            return;
        }

        if let Some(template_fsm) = self.fsm_menu.get(&req.sig_reason) {
            let new_fsm = template_fsm.clone();
            let act = new_fsm.action();

            if act == pipeline_action::Normal {
                return;
            }

            let key = (
                signal_priority(req.sig_reason),
                req.issuer_stage,
                act,
                req.sig_reason,
            );
            self.active_sig.entry(key).or_insert_with(|| active_signal {
                issuer_stage: req.issuer_stage,
                fsm: new_fsm,
                target_stages: req.target_stages,
            });
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

        let winner_key = self.active_sig.keys().next_back().copied();
        self.last_winner_reason = None;

        if let Some(winner_key) = winner_key {
            if let Some(champ_sig) = self.active_sig.get_mut(&winner_key) {
                self.last_winner_reason = Some(champ_sig.fsm.reason());
                let mut winner_ops = champ_sig.fsm.get_ops();

                if let Some(target_stages) = &champ_sig.target_stages {
                    winner_ops.retain(|stage, _| target_stages.contains(stage));
                }

                ret.extend(winner_ops);
                champ_sig.fsm.advance_winner();
            }

            for (key, active) in self.active_sig.iter_mut() {
                if *key != winner_key {
                    active.fsm.handle_blocked();
                }
            }
        }

        ret
    }

    fn update_active(&mut self) {
        let active_sig = std::mem::take(&mut self.active_sig);

        self.active_sig = active_sig
            .into_iter()
            .filter_map(|(_, active)| {
                let act = active.fsm.action();
                if act == pipeline_action::Normal {
                    None
                } else {
                    Some((
                        (
                            signal_priority(active.fsm.reason()),
                            active.issuer_stage,
                            act,
                            active.fsm.reason(),
                        ),
                        active,
                    ))
                }
            })
            .collect();
    }

    pub fn get_decision(&mut self) -> HashMap<CPU_stages, pipeline_action> {
        let ret = self.collect_result();
        self.update_active();

        return ret;
    }

    pub fn last_winner_reason(&self) -> Option<signal_reason> {
        self.last_winner_reason
    }

    pub fn has_active_signal(&self, sig_reason: signal_reason) -> bool {
        self.active_sig
            .values()
            .any(|active| active.fsm.reason() == sig_reason)
    }

    pub fn clear_active_signal(&mut self, sig_reason: signal_reason) {
        self.active_sig
            .retain(|_, active| active.fsm.reason() != sig_reason);

        if self.last_winner_reason == Some(sig_reason) {
            self.last_winner_reason = None;
        }
    }
}

#[derive(Clone, Copy)]
pub struct ExternalPause_FSM;

impl SigFSM for ExternalPause_FSM {
    fn reason(&self) -> signal_reason {
        signal_reason::external_pause
    }

    fn action(&self) -> pipeline_action {
        pipeline_action::Stall
    }

    fn get_ops(&self) -> HashMap<CPU_stages, pipeline_action> {
        HashMap::from([
            (CPU_stages::ID, pipeline_action::Stall),
            (CPU_stages::EX, pipeline_action::Stall),
            (CPU_stages::AGU, pipeline_action::Stall),
            (CPU_stages::MEM, pipeline_action::Stall),
            (CPU_stages::WB, pipeline_action::Stall),
        ])
    }

    fn advance_winner(&mut self) -> bool {
        true
    }

    fn handle_blocked(&mut self) {}
}

impl ExternalPause_FSM {
    pub const fn new() -> Self {
        Self
    }
}
