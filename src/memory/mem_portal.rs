pub struct dram_req{
    addr: u64,
    id: Option<u64>,
    is_read: bool,
    is_pim: bool
}

impl dram_req{
    pub fn new(addr: u64, is_read_: bool, is_pim_: bool) -> Self{
        Self{
            addr: 0,
            id: None,
            is_read: is_read_,
            is_pim: is_pim_
        }
    }

    pub fn set_id(&mut self, new_id: u64) {
        self.id = Some(new_id)
    }

    pub fn get_addr(&self) -> u64{
        self.addr
    }

    pub fn get_id(&self) -> Option<u64> {
        self.id
    }

    pub fn is_read(&self) -> bool{
        self.is_read
    }

    pub fn is_pim(&self) -> bool{
        self.is_pim
    }
}

struct dram_portal{

}
