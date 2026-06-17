macro_rules! rfid_chk {
    ($val:expr, $max_rf:expr, $rf_name:expr) => {{
        let id = $val as usize;

        if id >= $max_rf {
            panic!(
                "PErf: Trying to access {}[{}] is out of bound",
                $rf_name, id
            );
        }

        if id == 0 { None } else { Some(id) }
    }};
}

const vRF_max: usize = 16;
const sRF_max: usize = 8;

pub struct arch_rf {
    vRF: [[i16; 8]; vRF_max],
    sRF: [i32; sRF_max],
}

impl arch_rf {
    pub fn new() -> Self {
        Self {
            vRF: [[0; 8]; vRF_max],
            sRF: [0; sRF_max],
        }
    }

    pub fn read_vRF(&self, id: u8) -> [i16; 8] {
        match rfid_chk!(id, vRF_max, "vRF") {
            Some(id) => self.vRF[id],
            None => [0; 8],
        }
    }

    pub fn write_vRF(&mut self, id: u8, content: [i16; 8]) {
        if let Some(id) = rfid_chk!(id, vRF_max, "vRF") {
            self.vRF[id] = content;
        }
    }

    pub fn read_sRF(&self, id: u8) -> i32 {
        match rfid_chk!(id, sRF_max, "sRF") {
            Some(id) => self.sRF[id],
            None => 0,
        }
    }

    pub fn write_sRF(&mut self, id: u8, content: i32) {
        if let Some(id) = rfid_chk!(id, sRF_max, "sRF") {
            self.sRF[id] = content;
        }
    }
}
