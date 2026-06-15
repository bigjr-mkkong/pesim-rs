use crate::PE::pe_top::PE;
use crate::PE::types::{ALUop, MEMop, WBop, inst};

pub struct ISSUE_EX_RF {
    aluop: ALUop,
    memop: MEMop,
    wbop: WBop,
}

impl ISSUE_EX_RF {
    pub const fn new() -> Self {
        Self {
            aluop: ALUop::NOP,
            memop: MEMop::NOP,
            wbop: WBop::NOP,
        }
    }

    pub fn get_aluop(&self) -> ALUop {
        self.aluop
    }

    pub fn get_memop(&self) -> MEMop {
        self.memop
    }

    pub fn get_wbop(&self) -> WBop {
        self.wbop
    }
}

impl PE {
    pub fn issue_ex_eval(read_inst: inst) -> ISSUE_EX_RF {
        match read_inst {
            inst::NOP => ISSUE_EX_RF {
                aluop: ALUop::NOP,
                memop: MEMop::NOP,
                wbop: WBop::NOP,
            },
            _ => {
                todo!()
            }
        };

        todo!()
    }
}
