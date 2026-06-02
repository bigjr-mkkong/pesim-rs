use crate::memory::dramsim3_cxx_ffi::dramsim3_ffi::*;
use crate::memory::mem_portal::{dram_req};
use std::collections::{HashMap, VecDeque};

struct mem_vis {
    pub pend_reads: i32,
    pub pend_writes: i32,
}

/*
 * TODO
 * Move from current mixed vis + pending_list into:
 * pend_read: HashMap<u64, VecDequeu<dram_req>>
 * pend_write: HashMap<u64, VecDequeu<dram_req>>
 *
 * Re implement following functions:
 *  read_handler()
 *  write_handler()
 *  get_pend_read()
 *  get_pend_write()
 *  try_commit_req()
 *  AddTransaction()
 *
 *  based on the above chanes in pending request tracking
 *
 *  Do not modify callback function interface, it's by design only receive the address. To support
 *  reqeust unique ID, you can default assume request for each type(read/write) in one address are
 *  commited in order
 *
 *
 */

pub struct dramsim3_wrapper {
    ms: cxx::UniquePtr<dramsim3_ext>,
    vis: HashMap<u64, mem_vis>,
    pending_list: HashMap<u64, VecDeque<dram_req>>,
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
            vis: HashMap::new(),
            pending_list: HashMap::new(),
            ch: ch_,
            ra: ra_,
            bg: bg_,
            ba: ba_,
            req_id: 0
        }
    }

    fn get_req_id(&mut self) -> u64{
        let id = self.req_id;
        self.req_id += 1;
        id
    }

    fn read_handler(&mut self, addr: u64) {
        match self.vis.get_mut(&addr) {
            Some(vec_rec) => {
                if vec_rec.pend_reads <= 0 {
                    panic!("read_handler received an address with pend_reads <= 0");
                } else {
                    vec_rec.pend_reads -= 1;
                }
            }
            None => {
                panic!("read_handler received an invalid address: {}", addr);
            }
        }
    }

    fn write_handler(&mut self, addr: u64) {
        match self.vis.get_mut(&addr) {
            Some(vec_rec) => {
                if vec_rec.pend_writes <= 0 {
                    panic!("write_handler received an address with pend_writes <= 0");
                } else {
                    vec_rec.pend_writes -= 1;
                }
            }
            None => {
                panic!("write_handler received an invalid address: {}", addr);
            }
        }
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
        let real_addr: u64;
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
        if is_pim == true {
            addr_bulk.bank_local_addr = addr;
            real_addr = dramsim3_ext::BankLocalToGlobalAddr(self.ms.pin_mut(), &addr_bulk);
        } else {
            real_addr = addr;
        }

        match &mut self.vis.get(&real_addr) {
            Some(vis_rec) => return vis_rec.pend_reads,
            None => return 0,
        };
    }

    pub fn get_pend_write(&mut self, addr: u64, is_pim: bool) -> i32 {
        let real_addr: u64;
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
        if is_pim == true {
            addr_bulk.bank_local_addr = addr;
            real_addr = dramsim3_ext::BankLocalToGlobalAddr(self.ms.pin_mut(), &addr_bulk);
        } else {
            real_addr = addr;
        }

        match &mut self.vis.get(&real_addr) {
            Some(vis_rec) => return vis_rec.pend_writes,
            None => return 0,
        };
    }

    fn add_pending_list(&mut self, req: dram_req) {
        if let Some(_) = req.get_id() {
            panic!("Cannot add this req: already added");
        }

        let addr = req.get_addr();
        match self.pending_list.entry(addr) {
            std::collections::hash_map::Entry::Occupied(mut ent) => {
                ent.get_mut().push_back(req)
            },
            std::collections::hash_map::Entry::Vacant(mut ent) => {
                ent.insert(VecDeque::<dram_req>::from([
                        req
                ]));
            }
        }
    }

    pub fn try_commit_req(&mut self, req: dram_req) -> bool {
            let addr = req.get_addr();

    let id = req
        .get_id()
        .expect("Cannot commit an unsubmitted request");

    match self.pending_list.entry(addr) {
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

    pub fn WillAcceptTransaction(&mut self, addr: u64, is_write: bool) -> bool {
        return dramsim3_ext::WillAcceptTransaction(self.ms.pin_mut(), addr, is_write);
    }

    pub fn AddTransaction(&mut self, addr: u64, is_write: bool, is_pim: bool) {
        let real_addr: u64;
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
        if is_pim == true {
            addr_bulk.bank_local_addr = addr;
            real_addr = dramsim3_ext::BankLocalToGlobalAddr(self.ms.pin_mut(), &addr_bulk);
        } else {
            real_addr = addr;
        }

        let ret = dramsim3_ext::AddTransaction(self.ms.pin_mut(), real_addr, is_write, is_pim);

        if ret == true {
            if is_write == true {
                self.vis.insert(
                    real_addr,
                    mem_vis {
                        pend_reads: 0,
                        pend_writes: 1,
                    },
                );
                let mut d_req = dram_req::new(addr, is_write, is_pim);
                d_req.set_id(self.get_req_id());

                self.add_pending_list(d_req);
            } else {
                self.vis.insert(
                    real_addr,
                    mem_vis {
                        pend_reads: 1,
                        pend_writes: 0,
                    },
                );
            }
        } else {
            panic!("AddTransaction(): AddTransaction() Failed");
        }
    }
}
