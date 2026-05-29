use crate::memory::dramsim3_cxx_ffi::dramsim3_ffi::*;
use std::collections::HashMap;

struct mem_vis {
    pub pend_reads: i32,
    pub pend_writes: i32,
}

pub struct dramsim3_wrapper {
    ms: cxx::UniquePtr<dramsim3_ext>,
    vis: HashMap<u64, mem_vis>,
    ch: u64,
    ra: u64,
    bg: u64,
    ba: u64,
}

impl dramsim3_wrapper {
    pub fn new(cfg_path: &str, out_dir: &str, ch_: u64, ra_: u64, bg_: u64, ba_: u64) -> Self {
        dramsim3_wrapper {
            ms: create_sim(cfg_path, out_dir),
            vis: HashMap::new(),
            ch: ch_,
            ra: ra_,
            bg: bg_,
            ba: ba_,
        }
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
