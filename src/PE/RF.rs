macro_rules! rfid_chk {
    ($val:expr, $max_rf:expr) => {
        if $val < $max_rf {
            $val
        } else {
            panic!("PErf: Trying to read {} is out of bound", $val,);
        }
    };
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
            vRF: [[0; 8]; 16],
            sRF: [0; 8],
        }
    }

    /*
     * TODO
     * rfid_chk() will only check if id is larger than max
     * It will not check if ID is equal to 0 and should return an empty
     * Task:
     * #1 Implement a better version of rfid_chk
     * #2 Implement Write functions for RF, details of write semantics can refer to src/PE/ISA-doc
     */
    pub fn read_vRF(&self, id: u8) -> [i16; 8] {
        if id == 0 {
            return [0; 8];
        } else {
            let id = rfid_chk!(id as usize, vRF_max);
            return self.vRF[id];
        }
    }
}
