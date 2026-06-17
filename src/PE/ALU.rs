/*
 * TODO #1:
 * Implement ALU function for EX stage
 * EX stage contain both EX-MEM-WB, just as what's being described in ISA-doc
 * This ALU should receive ALUop as input and produce output as ALU_out type which is an enum and
 * looks like:
 *      ALU_out{
 *      vec_out{lit: [i16; 8]},
 *      scalar_out{i32},
 *      NA
 *      }
 *
 *  MAC operation will perform sRS0_lit + vRS0_lit * vRS1_lit
 *  ReLU operation perform as:
 *  for i in 0..7:
 *      if vRS0_lit[i] < 0
 *          result[i] = 0;
 *      else
 *          result[i] = vRS0_lit[i]
 */
