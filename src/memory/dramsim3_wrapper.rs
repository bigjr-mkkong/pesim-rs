use crate::memory::dramsim3_cxx_ffi::dramsim3_ffi::*;
use crate::memory::mem_portal::dram_req;
use std::collections::{HashMap, VecDeque};

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

impl dramsim3_wrapper {
    pub fn new(cfg_path: &str, out_dir: &str, ch_: u64, ra_: u64, bg_: u64, ba_: u64) -> Self {
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

    fn get_req_id(&mut self) -> u64 {
        let id = self.req_id;
        self.req_id += 1;
        id
    }

    fn translate_addr(&mut self, addr: u64, is_pim: bool) -> u64 {
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

    fn queue_len(queue_map: &HashMap<u64, VecDeque<dram_req>>, addr: u64) -> i32 {
        queue_map.get(&addr).map_or(0, |queue| queue.len() as i32)
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
    ) {
        match queue_map.entry(addr) {
            std::collections::hash_map::Entry::Occupied(mut ent) => {
                let queue = ent.get_mut();

                if queue.pop_front().is_none() {
                    panic!(
                        "{} received an address with no pending request",
                        handler_name
                    );
                }

                if queue.is_empty() {
                    ent.remove();
                }
            }
            std::collections::hash_map::Entry::Vacant(_) => {
                panic!("{} received an invalid address: {}", handler_name, addr);
            }
        }
    }

    fn remove_pending(
        queue_map: &mut HashMap<u64, VecDeque<dram_req>>,
        addr: u64,
        id: u64,
    ) -> bool {
        match queue_map.entry(addr) {
            std::collections::hash_map::Entry::Occupied(mut ent) => {
                let queue = ent.get_mut();
                let pos = queue.iter().position(|r| {
                    let rid = r
                        .get_id()
                        .expect("Unexpected None id found in pending queue");

                    rid == id
                });

                match pos {
                    Some(index) => {
                        queue.remove(index);

                        if queue.is_empty() {
                            ent.remove();
                        }

                        true
                    }
                    None => false,
                }
            }
            std::collections::hash_map::Entry::Vacant(_) => false,
        }
    }

    fn read_handler(&mut self, addr: u64) {
        Self::pop_completed(&mut self.pend_read, addr, "read_handler");
    }

    fn write_handler(&mut self, addr: u64) {
        Self::pop_completed(&mut self.pend_write, addr, "write_handler");
    }

    pub fn ClockTick(&mut self) {
        dramsim3_ext::ClockTick(self.ms.pin_mut());
        let mem_evs = dramsim3_ext::take_events(self.ms.pin_mut());

        for events in &mem_evs {
            if events.is_write == true {
                self.write_handler(events.addr);
            } else {
                self.read_handler(events.addr);
            }
        }
    }

    pub fn get_TCK(&mut self) -> f64 {
        return dramsim3_ext::GetTCK(self.ms.pin_mut());
    }

    pub fn get_bus_bits(&mut self) -> i32 {
        return dramsim3_ext::GetBusBits(self.ms.pin_mut());
    }

    pub fn get_burst_length(&mut self) -> i32 {
        return dramsim3_ext::GetBurstLength(self.ms.pin_mut());
    }

    pub fn get_queue_size(&mut self) -> i32 {
        return dramsim3_ext::GetQueueSize(self.ms.pin_mut());
    }

    pub fn get_clock(&mut self) -> i32 {
        return dramsim3_ext::GetClock(self.ms.pin_mut());
    }

    pub fn get_clock_tick(&mut self) -> i32 {
        return dramsim3_ext::GetClock(self.ms.pin_mut());
    }

    pub fn get_pend_read(&mut self, addr: u64, is_pim: bool) -> i32 {
        let real_addr = self.translate_addr(addr, is_pim);
        Self::queue_len(&self.pend_read, real_addr)
    }

    pub fn get_pend_write(&mut self, addr: u64, is_pim: bool) -> i32 {
        let real_addr = self.translate_addr(addr, is_pim);
        Self::queue_len(&self.pend_write, real_addr)
    }

    pub fn try_commit_req(&mut self, req: dram_req) -> bool {
        let addr = self.translate_addr(req.get_addr(), req.is_pim());
        let id = req.get_id().expect("Cannot commit an unsubmitted request");

        if req.is_read() {
            Self::remove_pending(&mut self.pend_read, addr, id)
        } else {
            Self::remove_pending(&mut self.pend_write, addr, id)
        }
    }

    pub fn WillAcceptTransaction(&mut self, addr: u64, is_write: bool) -> bool {
        return dramsim3_ext::WillAcceptTransaction(self.ms.pin_mut(), addr, is_write);
    }

    pub fn AddTransaction(&mut self, addr: u64, is_write: bool, is_pim: bool) {
        let real_addr = self.translate_addr(addr, is_pim);
        let ret = dramsim3_ext::AddTransaction(self.ms.pin_mut(), real_addr, is_write, is_pim);

        if ret == true {
            let mut d_req = dram_req::new(addr, !is_write, is_pim);
            d_req.set_id(self.get_req_id());

            if is_write == true {
                Self::push_pending(&mut self.pend_write, real_addr, d_req);
            } else {
                Self::push_pending(&mut self.pend_read, real_addr, d_req);
            }
        } else {
            panic!("AddTransaction(): AddTransaction() Failed");
        }
    }
}
