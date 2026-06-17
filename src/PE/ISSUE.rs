use crate::PE::RF::arch_rf;
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
    pub fn issue_ex_eval(read_inst: inst, arf: &arch_rf) -> ISSUE_EX_RF {
        match read_inst {
            inst::NOP => ISSUE_EX_RF {
                aluop: ALUop::NOP,
                memop: MEMop::NOP,
                wbop: WBop::NOP,
            },
            inst::LD128 { vRD, addr } => ISSUE_EX_RF {
                aluop: ALUop::NOP,
                memop: MEMop::ReadV { addr: addr },
                wbop: WBop::VWrite { vRD: vRD },
            },
            inst::ST128 { vRS, addr } => {
                let data = arf.read_vRF(vRS);
                ISSUE_EX_RF {
                    aluop: ALUop::NOP,
                    memop: MEMop::WriteV { addr, data: data },
                    wbop: WBop::NOP,
                }
            }
            inst::LD32 { sRD, addr } => ISSUE_EX_RF {
                aluop: ALUop::NOP,
                memop: MEMop::ReadS { addr: addr },
                wbop: WBop::SWrite { sRD: sRD },
            },
            inst::ST32 { sRS, addr } => {
                let data = arf.read_sRF(sRS);
                ISSUE_EX_RF {
                    aluop: ALUop::NOP,
                    memop: MEMop::WriteS { addr, data: data },
                    wbop: WBop::NOP,
                }
            }
            inst::ADD128 { vRD, vRS0, vRS1 } => {
                let rs0_lit = arf.read_vRF(vRS0);
                let rs1_lit = arf.read_vRF(vRS1);
                ISSUE_EX_RF {
                    aluop: ALUop::ADD {
                        vRS0_lit: rs0_lit,
                        vRS1_lit: rs1_lit,
                    },
                    memop: MEMop::NOP,
                    wbop: WBop::VWrite { vRD: vRD },
                }
            }
            inst::SUB128 { vRD, vRS0, vRS1 } => {
                let rs0_lit = arf.read_vRF(vRS0);
                let rs1_lit = arf.read_vRF(vRS1);
                ISSUE_EX_RF {
                    aluop: ALUop::SUB {
                        vRS0_lit: rs0_lit,
                        vRS1_lit: rs1_lit,
                    },
                    memop: MEMop::NOP,
                    wbop: WBop::VWrite { vRD: vRD },
                }
            }
            inst::MUL128 { vRD, vRS0, vRS1 } => {
                let rs0_lit = arf.read_vRF(vRS0);
                let rs1_lit = arf.read_vRF(vRS1);
                ISSUE_EX_RF {
                    aluop: ALUop::MUL {
                        vRS0_lit: rs0_lit,
                        vRS1_lit: rs1_lit,
                    },
                    memop: MEMop::NOP,
                    wbop: WBop::VWrite { vRD: vRD },
                }
            }
            inst::MAC128 {
                sRD,
                sRS0,
                vRS0,
                vRS1,
            } => {
                let srs0_lit = arf.read_sRF(sRS0);
                let vrs0_lit = arf.read_vRF(vRS0);
                let vrs1_lit = arf.read_vRF(vRS1);
                ISSUE_EX_RF {
                    aluop: ALUop::MAC {
                        sRS0_lit: srs0_lit,
                        vRS0_lit: vrs0_lit,
                        vRS1_lit: vrs1_lit,
                    },
                    memop: MEMop::NOP,
                    wbop: WBop::SWrite { sRD: sRD },
                }
            }
            inst::ReLU { vRD, vRS0 } => {
                let vrs0_lit = arf.read_vRF(vRS0);
                ISSUE_EX_RF {
                    aluop: ALUop::ReLU { vRS0_lit: vrs0_lit },
                    memop: MEMop::NOP,
                    wbop: WBop::VWrite { vRD: vRD },
                }
            }
        };

        todo!()
    }
}
