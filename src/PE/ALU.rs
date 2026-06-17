use crate::PE::types::ALUop;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ALU_out {
    vec_out { lit: [i16; 8] },
    scalar_out { lit: i32 },
    NA,
}

fn zip_vector(lhs: [i16; 8], rhs: [i16; 8], op: impl Fn(i16, i16) -> i16) -> [i16; 8] {
    std::array::from_fn(|i| op(lhs[i], rhs[i]))
}

fn dot_product(lhs: [i16; 8], rhs: [i16; 8]) -> i32 {
    lhs.into_iter()
        .zip(rhs)
        .map(|(lhs, rhs)| i32::from(lhs) * i32::from(rhs))
        .sum()
}

pub fn ALU_comp(aluop: ALUop) -> ALU_out {
    match aluop {
        ALUop::ADD { vRS0_lit, vRS1_lit } => ALU_out::vec_out {
            lit: zip_vector(vRS0_lit, vRS1_lit, i16::wrapping_add),
        },
        ALUop::SUB { vRS0_lit, vRS1_lit } => ALU_out::vec_out {
            lit: zip_vector(vRS0_lit, vRS1_lit, i16::wrapping_sub),
        },
        ALUop::MUL { vRS0_lit, vRS1_lit } => ALU_out::vec_out {
            lit: zip_vector(vRS0_lit, vRS1_lit, i16::wrapping_mul),
        },
        ALUop::MAC {
            sRS0_lit,
            vRS0_lit,
            vRS1_lit,
        } => ALU_out::scalar_out {
            lit: sRS0_lit + dot_product(vRS0_lit, vRS1_lit),
        },
        ALUop::ReLU { vRS0_lit } => ALU_out::vec_out {
            lit: vRS0_lit.map(|lit| lit.max(0)),
        },
        ALUop::NOP => ALU_out::NA,
    }
}
