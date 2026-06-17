/*
 * TODO #1
 * This is the place where eval_EX() need to be implemented
 * eval_EX() will take ISSUE_EX_RF and produce the arch_op and signal just as what's being used in
 * src/cpu
 *
 * However, PE is a simplified model which only contain two stage CPU. In this case, the only signal
 * PE need to handle is the MEM_stop.
 *
 * MEM_stop signal will block the previous ISSUE state until it finished
 *
 * To implement bypass, EX stage need to bypass it's previous result into itself for next cycle. In
 * this case, we need a similiar mechanism as src/cpu/WB.rs which create a pseudo pipeline register
 * file and store the WB data only for bypass logic
 *
 * Task:
 * Implement eval_EX() function which mirror the idea from src/cpu/EX.rs and MEM.rs and WB.rs implementation
 */
