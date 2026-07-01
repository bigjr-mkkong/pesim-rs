use crate::memory::dramsim3_cxx_ffi::dramsim3_ffi::*;
use crate::memory::mem_portal::dram_req;
use std::collections::{HashMap, VecDeque};
use std::path::Path;

pub struct dramsim3_wrapper {
    ms: cxx::UniquePtr<dramsim3_ext>,
    pend_read: HashMap<u64, VecDeque<dram_req>>,
    pend_write: HashMap<u64, VecDeque<dram_req>>,
    ch: u64,
    ra: u64,
    bg: u64,
    ba: u64,
    /*
     * Unique request ID to track precise complete order for each request
     */
    req_id: u64,
}

// Each wrapper owns a distinct DRAMsim3 instance. Sim ticks different wrappers
// on different threads, but never shares one wrapper between threads concurrently.
unsafe impl Send for dramsim3_wrapper {}

impl dramsim3_wrapper {
    pub fn new(
        cfg_path: impl AsRef<Path>,
        out_dir: impl AsRef<Path>,
        ch_: u64,
        ra_: u64,
        bg_: u64,
        ba_: u64,
    ) -> Self {
        let cfg_path = cfg_path
            .as_ref()
            .to_str()
            .expect("DRAMSim3 configuration path must be valid UTF-8");
        let out_dir = out_dir
            .as_ref()
            .to_str()
            .expect("DRAMSim3 output path must be valid UTF-8");

        dramsim3_wrapper {
            ms: create_sim(cfg_path, out_dir),
            pend_read: HashMap::new(),
            pend_write: HashMap::new(),
            ch: ch_,
            ra: ra_,
            bg: bg_,
            ba: ba_,
            req_id: 0,
        }
    }

    pub(crate) fn get_req_id(&mut self) -> u64 {
        let id = self.req_id;
        self.req_id += 1;
        id
    }

    fn request_addr_to_dram_addr(&mut self, addr: u64, is_pim: bool) -> u64 {
        if !is_pim {
            return addr;
        }

        let mut addr_bulk: local_addr_bulk = local_addr_bulk {
            channel: self.ch,
            rank: self.ra,
            bank_group: self.bg,
            bank: self.ba,
            bank_local_addr: 0,
            global_addr: 0,
            row: 0,
            column: 0,
        };
        addr_bulk.bank_local_addr = addr;

        dramsim3_ext::BankLocalToGlobalAddr(self.ms.pin_mut(), &addr_bulk)
    }

    pub fn global_addr_to_local_components(&mut self, addr: u64) -> local_addr_bulk {
        dramsim3_ext::GlobalToLocalAddr(self.ms.pin_mut(), addr)
    }

    fn push_pending(queue_map: &mut HashMap<u64, VecDeque<dram_req>>, addr: u64, req: dram_req) {
        if req.get_id().is_none() {
            panic!("Cannot add this req: request id is missing");
        }

        queue_map.entry(addr).or_default().push_back(req);
    }

    fn pop_completed(
        queue_map: &mut HashMap<u64, VecDeque<dram_req>>,
        addr: u64,
        handler_name: &str,
    ) -> dram_req {
        match queue_map.entry(addr) {
            std::collections::hash_map::Entry::Occupied(mut ent) => {
                let queue = ent.get_mut();
                let req = queue.pop_front().unwrap_or_else(|| {
                    panic!(
                        "{} received an address with no pending request",
                        handler_name
                    )
                });

                if queue.is_empty() {
                    ent.remove();
                }

                req
            }
            std::collections::hash_map::Entry::Vacant(_) => {
                panic!("{} received an invalid address: {}", handler_name, addr);
            }
        }
    }

    pub fn ClockTick(&mut self) -> Vec<dram_req> {
        dramsim3_ext::ClockTick(self.ms.pin_mut());
        let mem_evs = dramsim3_ext::take_events(self.ms.pin_mut());
        let mut completed = Vec::new();

        for events in &mem_evs {
            if events.is_write {
                completed.push(Self::pop_completed(
                    &mut self.pend_write,
                    events.addr,
                    "write completion",
                ));
            } else {
                completed.push(Self::pop_completed(
                    &mut self.pend_read,
                    events.addr,
                    "read completion",
                ));
            }
        }

        completed
    }

    pub fn get_TCK(&mut self) -> f64 {
        dramsim3_ext::GetTCK(self.ms.pin_mut())
    }

    pub fn get_bus_bits(&mut self) -> i32 {
        dramsim3_ext::GetBusBits(self.ms.pin_mut())
    }

    pub fn get_burst_length(&mut self) -> i32 {
        dramsim3_ext::GetBurstLength(self.ms.pin_mut())
    }

    pub fn get_queue_size(&mut self) -> i32 {
        dramsim3_ext::GetQueueSize(self.ms.pin_mut())
    }

    pub fn get_channels(&mut self) -> u64 {
        dramsim3_ext::GetChannels(self.ms.pin_mut())
    }

    pub fn get_ranks(&mut self) -> u64 {
        dramsim3_ext::GetRanks(self.ms.pin_mut())
    }

    pub fn get_bankgroups_per_rank(&mut self) -> u64 {
        dramsim3_ext::GetBankgroupsPerRank(self.ms.pin_mut())
    }

    pub fn get_banks_per_bg(&mut self) -> u64 {
        dramsim3_ext::GetBanksPerBG(self.ms.pin_mut())
    }

    pub fn get_clock_tick(&mut self) -> i32 {
        dramsim3_ext::GetClock(self.ms.pin_mut())
    }

    pub fn GetPimMode(&mut self) -> bool {
        dramsim3_ext::GetPimMode(self.ms.pin_mut())
    }

    pub fn SetPimMode(&mut self, new_mode: bool) {
        if self.GetPimMode() != new_mode {
            dramsim3_ext::SetPimMode(self.ms.pin_mut(), new_mode);
        }
    }

    pub fn is_drained(&self) -> bool {
        self.pend_read.is_empty() && self.pend_write.is_empty()
    }

    pub fn WillAcceptTransaction(&mut self, addr: u64, is_write: bool) -> bool {
        dramsim3_ext::WillAcceptTransaction(self.ms.pin_mut(), addr, is_write)
    }

    pub fn AddTransactionReq(&mut self, req: dram_req) {
        req.assert_legal_for_issue();

        let real_addr = self.request_addr_to_dram_addr(req.get_addr(), req.is_pim());
        let is_write = !req.is_read();
        let ret =
            dramsim3_ext::AddTransaction(self.ms.pin_mut(), real_addr, is_write, req.is_pim());

        if ret {
            if is_write {
                Self::push_pending(&mut self.pend_write, real_addr, req);
            } else {
                Self::push_pending(&mut self.pend_read, real_addr, req);
            }
        } else {
            panic!("AddTransaction(): AddTransaction() Failed");
        }
    }
}
