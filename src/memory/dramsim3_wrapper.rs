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
    pseudo_bank: Option<u64>,
    pim_bank_local_base: u64,
    pim_bank_local_size: Option<u64>,
    host_addr_base: u64,
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
        Self::new_with_address_base(cfg_path, out_dir, ch_, ra_, bg_, ba_, 0)
    }

    pub fn new_with_address_base(
        cfg_path: impl AsRef<Path>,
        out_dir: impl AsRef<Path>,
        ch_: u64,
        ra_: u64,
        bg_: u64,
        ba_: u64,
        host_addr_base: u64,
    ) -> Self {
        Self::build(
            cfg_path,
            out_dir,
            ch_,
            ra_,
            bg_,
            ba_,
            None,
            0,
            None,
            host_addr_base,
        )
    }

    pub fn new_for_pseudo_bank(
        cfg_path: impl AsRef<Path>,
        out_dir: impl AsRef<Path>,
        ch_: u64,
        ra_: u64,
        bg_: u64,
        ba_: u64,
        pseudo_bank: u64,
        pim_bank_local_base: u64,
        pim_bank_local_size: u64,
        host_addr_base: u64,
    ) -> Self {
        Self::build(
            cfg_path,
            out_dir,
            ch_,
            ra_,
            bg_,
            ba_,
            Some(pseudo_bank),
            pim_bank_local_base,
            Some(pim_bank_local_size),
            host_addr_base,
        )
    }

    fn build(
        cfg_path: impl AsRef<Path>,
        out_dir: impl AsRef<Path>,
        ch_: u64,
        ra_: u64,
        bg_: u64,
        ba_: u64,
        pseudo_bank: Option<u64>,
        pim_bank_local_base: u64,
        pim_bank_local_size: Option<u64>,
        host_addr_base: u64,
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
            pseudo_bank,
            pim_bank_local_base,
            pim_bank_local_size,
            host_addr_base,
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
            return addr.checked_sub(self.host_addr_base).unwrap_or_else(|| {
                panic!(
                    "host address {addr:#x} is below controller base {:#x}",
                    self.host_addr_base
                )
            });
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
        if let Some(size) = self.pim_bank_local_size
            && addr >= size
        {
            self.fatal_pim_oob(addr, size);
        }
        addr_bulk.bank_local_addr = self
            .pim_bank_local_base
            .checked_add(addr)
            .unwrap_or_else(|| self.fatal_pim_oob(addr, self.pim_bank_local_size.unwrap_or(0)));

        dramsim3_ext::BankLocalToGlobalAddr(self.ms.pin_mut(), &addr_bulk)
    }

    #[cfg(test)]
    pub(crate) fn request_addr_to_dram_addr_for_test(&mut self, addr: u64, is_pim: bool) -> u64 {
        self.request_addr_to_dram_addr(addr, is_pim)
    }

    #[cold]
    fn fatal_pim_oob(&self, addr: u64, size: u64) -> ! {
        eprintln!(
            "PIM_FATAL reason=timing_address_out_of_bounds ch={} rank={} bank_group={} bank={} pseudo_bank={} cacheline={} valid_cachelines=0..{}",
            self.ch,
            self.ra,
            self.bg,
            self.ba,
            self.pseudo_bank.unwrap_or(0),
            addr,
            size
        );

        #[cfg(test)]
        panic!("PIM timing address is outside its pseudo bank");

        #[cfg(not(test))]
        std::process::abort();
    }

    pub fn global_addr_to_local_components(&mut self, addr: u64) -> local_addr_bulk {
        let local = addr.checked_sub(self.host_addr_base).unwrap_or_else(|| {
            panic!(
                "host address {addr:#x} is below controller base {:#x}",
                self.host_addr_base
            )
        });
        dramsim3_ext::GlobalToLocalAddr(self.ms.pin_mut(), local)
    }

    pub fn exact_local_to_global_addr(
        &mut self,
        channel: u64,
        rank: u64,
        bank_group: u64,
        bank: u64,
        row: u64,
        column: u64,
    ) -> u64 {
        let local_addr = local_addr_bulk {
            channel,
            rank,
            bank_group,
            bank,
            row,
            column,
            global_addr: 0,
            bank_local_addr: 0,
        };

        dramsim3_ext::ExactLocalToGlobalAddr(self.ms.pin_mut(), &local_addr)
            .checked_add(self.host_addr_base)
            .expect("controller-global address overflow")
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
        let local_addr = local_addr_bulk {
            channel: 0,
            rank: 0,
            bank_group: 0,
            bank: 3,
            row: 0,
            column: 0,
            global_addr: 0,
            bank_local_addr: 0,
        };
        let addr = dramsim3_ext::BankLocalToGlobalAddr(self.ms.pin_mut(), &local_addr);

        println!("Translated address: 0x{:x}", addr);
        dramsim3_ext::GetBurstLength(self.ms.pin_mut())
    }

    pub fn get_queue_size(&mut self) -> i32 {
        dramsim3_ext::GetQueueSize(self.ms.pin_mut())
    }

    pub fn get_near_switch_latency(&mut self) -> i32 {
        dramsim3_ext::GetNearSwitchLatency(self.ms.pin_mut())
    }

    pub fn get_pim_switch_enabled(&mut self) -> bool {
        dramsim3_ext::GetPimSwitchEnabled(self.ms.pin_mut())
    }

    pub fn get_capacity_bytes(&mut self) -> u64 {
        dramsim3_ext::GetCapacityBytes(self.ms.pin_mut())
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

    pub fn request_pause(&mut self) {
        dramsim3_ext::RequestPause(self.ms.pin_mut());
    }

    pub fn is_pause_requested(&mut self) -> bool {
        dramsim3_ext::IsPauseRequested(self.ms.pin_mut())
    }

    pub fn is_pause_ready(&mut self) -> bool {
        dramsim3_ext::IsPauseReady(self.ms.pin_mut())
    }

    pub fn commit_paused_mode(&mut self, new_mode: bool) {
        dramsim3_ext::CommitPausedMode(self.ms.pin_mut(), new_mode);
    }

    pub fn cancel_pause(&mut self) {
        dramsim3_ext::CancelPause(self.ms.pin_mut());
    }

    pub fn pause_parked_transactions(&mut self) -> u64 {
        dramsim3_ext::GetPauseParkedTransactions(self.ms.pin_mut())
    }

    pub fn pause_promoted_transactions(&mut self) -> u64 {
        dramsim3_ext::GetPausePromotedTransactions(self.ms.pin_mut())
    }

    /*
     * FIXME
     * This function is a newly added function for dramsim3
     * I would like to see independent testcase to verify if this function to behave as what it
     * suppose to be.
     *
     * Testcase can be push n requests into dsim3, then verify before n requests all returned,
     * is_drained is always false, and after they returned is_drained is always true.
     */
    pub fn is_drained(&mut self) -> bool {
        self.pend_read.is_empty()
            && self.pend_write.is_empty()
            && dramsim3_ext::IsDrained(self.ms.pin_mut())
    }

    pub fn WillAcceptTransaction(&mut self, addr: u64, is_write: bool) -> bool {
        let local = self.request_addr_to_dram_addr(addr, false);
        dramsim3_ext::WillAcceptTransaction(self.ms.pin_mut(), local, is_write)
    }

    pub fn WillAcceptTransactionReq(&mut self, req: &dram_req) -> bool {
        let real_addr = self.request_addr_to_dram_addr(req.get_addr(), req.is_pim());
        dramsim3_ext::WillAcceptTransaction(self.ms.pin_mut(), real_addr, !req.is_read())
    }

    pub fn AddTransactionReq(&mut self, req: dram_req) {
        req.assert_legal_for_issue();

        let real_addr = self.request_addr_to_dram_addr(req.get_addr(), req.is_pim());
        let is_write = !req.is_read();
        let ret = dramsim3_ext::AddTransaction(
            self.ms.pin_mut(),
            real_addr,
            is_write,
            req.use_pim_timing_context(),
        );

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
