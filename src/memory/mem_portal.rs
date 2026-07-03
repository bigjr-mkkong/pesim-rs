use std::cell::RefCell;
use std::rc::Rc;

pub type cacheline_payload = [u64; 8];

#[derive(Clone)]
pub struct dram_req {
    addr: u64,
    payload: cacheline_payload,
    id: Option<u64>,
    issue_time: Option<u64>,
    is_read: bool,
    is_pim: bool,
}

impl dram_req {
    pub fn new(addr: u64, is_read_: bool, is_pim_: bool) -> Self {
        Self::new_with_payload(addr, [0; 8], is_read_, is_pim_)
    }

    pub fn new_with_payload(
        addr: u64,
        payload: cacheline_payload,
        is_read_: bool,
        is_pim_: bool,
    ) -> Self {
        Self {
            addr,
            payload,
            id: None,
            issue_time: None,
            is_read: is_read_,
            is_pim: is_pim_,
        }
    }

    pub(crate) fn set_id(&mut self, new_id: u64) {
        if self.id.is_some() {
            panic!("Cannot overwrite request id");
        }

        self.id = Some(new_id)
    }

    pub(crate) fn set_issue_time(&mut self, issue_time: u64) {
        if self.issue_time.is_some() {
            panic!("Cannot overwrite request issue time");
        }

        self.issue_time = Some(issue_time);
    }

    pub fn get_addr(&self) -> u64 {
        self.addr
    }

    pub fn get_payload(&self) -> &cacheline_payload {
        &self.payload
    }

    pub(crate) fn set_payload_word0(&mut self, value: u64) {
        self.payload[0] = value;
    }

    pub fn get_id(&self) -> Option<u64> {
        self.id
    }

    pub fn get_issue_time(&self) -> Option<u64> {
        self.issue_time
    }

    pub(crate) fn assert_legal_for_issue(&self) {
        if self.id.is_none() {
            panic!("Cannot issue request: request id is missing");
        }

        if self.issue_time.is_none() {
            panic!("Cannot issue request: issue time is missing");
        }
    }

    pub fn is_read(&self) -> bool {
        self.is_read
    }

    pub fn is_pim(&self) -> bool {
        self.is_pim
    }

    fn matches_completion(&self, other: &dram_req) -> bool {
        self.addr == other.addr && self.is_read == other.is_read && self.is_pim == other.is_pim
    }
}

#[derive(Clone, Copy)]
pub enum portal_mode {
    PIM,
    HOST,
}

#[derive(Clone)]
pub struct dram_portal {
    simcpu_req: Rc<RefCell<Vec<dram_req>>>,
    host_req: Rc<RefCell<Vec<dram_req>>>,
    simcpu_resp: Rc<RefCell<Vec<dram_req>>>,
    host_resp: Rc<RefCell<Vec<dram_req>>>,
    mode: Rc<RefCell<portal_mode>>,
}

// A dram_portal is only shared inside one Engine (between that Engine and its CPU or PE).
// Sim moves each whole Engine to a scoped worker thread for tick(), so portal clones
// are not accessed from multiple threads at the same time. Keeping Rc<RefCell<_>>
// avoids unnecessary locking on this thread-local path.
unsafe impl Send for dram_portal {}

impl dram_portal {
    pub fn new() -> Self {
        Self {
            simcpu_req: Rc::new(RefCell::new(Vec::new())),
            host_req: Rc::new(RefCell::new(Vec::new())),
            simcpu_resp: Rc::new(RefCell::new(Vec::new())),
            host_resp: Rc::new(RefCell::new(Vec::new())),
            mode: Rc::new(RefCell::new(portal_mode::PIM)),
        }
    }

    pub fn get_mode(&self) -> portal_mode {
        *self.mode.borrow()
    }

    pub fn set_mode(&mut self, new_mode: portal_mode) {
        *self.mode.borrow_mut() = new_mode;
    }

    pub fn req_drained_for_mode(&self, mode: portal_mode) -> bool {
        match mode {
            portal_mode::PIM => self.simcpu_req.borrow().is_empty(),
            portal_mode::HOST => self.host_req.borrow().is_empty(),
        }
    }

    pub fn submit(&mut self, req: dram_req) {
        if req.is_pim() {
            self.simcpu_req.borrow_mut().push(req);
        } else {
            self.host_req.borrow_mut().push(req);
        }
    }

    pub fn get_one_req(&mut self) -> Option<dram_req> {
        if let portal_mode::PIM = self.get_mode() {
            self.simcpu_req.borrow_mut().pop()
        } else {
            self.host_req.borrow_mut().pop()
        }
    }

    pub fn complete(&mut self, req: dram_req) {
        if req.is_pim() {
            self.simcpu_resp.borrow_mut().push(req);
        } else {
            self.host_resp.borrow_mut().push(req);
        }
    }

    fn take_response(&mut self, is_pim: bool, expected: Option<&dram_req>) -> Option<dram_req> {
        let resp = if is_pim {
            &self.simcpu_resp
        } else {
            &self.host_resp
        };
        let mut resp = resp.borrow_mut();

        match expected {
            Some(req) => resp
                .iter()
                .position(|done| req.matches_completion(done))
                .map(|idx| resp.remove(idx)),
            None => resp.pop(),
        }
    }

    pub fn take_completed(&mut self, req: &dram_req) -> Option<dram_req> {
        self.take_response(req.is_pim(), Some(req))
    }

    pub fn take_host_completed(&mut self) -> Option<dram_req> {
        self.take_response(false, None)
    }
}
