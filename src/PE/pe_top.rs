/*
 * This directory describe the PE architecture for HBM-PIM liked PIM
 * A two cycle PE with no IF(directly receive instruction from host)
 *
 */

use crate::PE::ISSUE::ISSUE_EX_RF;

pub struct PE{
    issue_ex_rf: ISSUE_EX_RF
}
