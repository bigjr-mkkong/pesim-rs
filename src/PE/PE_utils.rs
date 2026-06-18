use crate::PE::pe_top::PE;
use crate::PE::types::WBop;

impl PE {
    pub fn ex_bypass_get_vreg(&self, rs: u8, rs_lit: [i16; 8]) -> [i16; 8] {
        if rs == 0 {
            return rs_lit;
        }

        if self.ex_wb_forward_rf.is_valid() {
            if let WBop::VWrite { vRD } = self.ex_wb_forward_rf.get_wbop() {
                if vRD == rs {
                    if let Some(content) = self.ex_wb_forward_rf.get_v_result() {
                        return content;
                    }
                }
            }
        }

        rs_lit
    }

    pub fn ex_bypass_get_sreg(&self, rs: u8, rs_lit: i32) -> i32 {
        if rs == 0 {
            return rs_lit;
        }

        if self.ex_wb_forward_rf.is_valid() {
            if let WBop::SWrite { sRD } = self.ex_wb_forward_rf.get_wbop() {
                if sRD == rs {
                    if let Some(content) = self.ex_wb_forward_rf.get_s_result() {
                        return content;
                    }
                }
            }
        }

        rs_lit
    }
}
