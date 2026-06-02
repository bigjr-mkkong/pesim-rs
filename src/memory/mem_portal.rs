pub struct dram_req {
    addr: u64,
    id: Option<u64>,
    is_read: bool,
    is_pim: bool,
}

impl dram_req {
    pub fn new(addr: u64, is_read_: bool, is_pim_: bool) -> Self {
        Self {
            addr,
            id: None,
            is_read: is_read_,
            is_pim: is_pim_,
        }
    }

    pub fn set_id(&mut self, new_id: u64) {
        self.id = Some(new_id)
    }

    pub fn get_addr(&self) -> u64 {
        self.addr
    }

    pub fn get_id(&self) -> Option<u64> {
        self.id
    }

    pub fn is_read(&self) -> bool {
        self.is_read
    }

    pub fn is_pim(&self) -> bool {
        self.is_pim
    }
}

pub enum portal_req{
    PIM_REQ{req: dram_req},
    HOST_REQ{req: dram_req}
}

#[derive(Clone, Copy)]
pub enum portal_mode{
    PIM,
    HOST
}

pub struct dram_portal {
    pimcpu_req: Vec<dram_req>,
    host_req: Vec<dram_req>,
    mode: portal_mode,
}

impl dram_portal{
    pub fn new() -> Self{
        Self{
            pimcpu_req: Vec::new(),
            host_req: Vec::new(),
            mode: portal_mode::HOST
        }
    }

    pub fn get_mode(&self) -> portal_mode {
        self.mode
    }

    pub fn set_mode(&mut self, new_mode: portal_mode) {
        self.mode = new_mode
    }


    pub fn submit(&mut self, req: portal_req) {
        match req {
            portal_req::PIM_REQ { req } => {
                self.pimcpu_req.push(req);
            },
            portal_req::HOST_REQ { req } => {
                self.host_req.push(req);
            }
        }
    }

    pub fn get_one_req(&mut self) -> Option<dram_req> {
        if let portal_mode::PIM = self.get_mode() {
            self.pimcpu_req.pop()
        } else {
            self.host_req.pop()
        }
    }
    
}
