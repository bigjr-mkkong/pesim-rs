use core::fmt;
use heapless::String;

pub type TinyString = String<64>;
pub type CommentString = String<128>;

#[derive(Debug, Clone)]
pub struct ErrorInfo {
    pub expected: Option<TinyString>,
    pub actual: Option<TinyString>,
    pub part: Option<TinyString>,
    pub comment: Option<CommentString>,
}

impl ErrorInfo {
    pub fn new() -> Self {
        Self {
            expected: None,
            actual: None,
            part: None,
            comment: None,
        }
    }

    pub fn expected(mut self, expected: &str) -> Self {
        let mut s = TinyString::new();
        s.push_str(expected)
            .expect("expected string too long for ErrorInfo");
        self.expected = Some(s);
        self
    }

    pub fn actual(mut self, actual: &str) -> Self {
        let mut s = TinyString::new();
        s.push_str(actual)
            .expect("actual string too long for ErrorInfo");
        self.actual = Some(s);
        self
    }

    pub fn part(mut self, part: &str) -> Self {
        let mut s = TinyString::new();
        s.push_str(part)
            .expect("part string too long for ErrorInfo");
        self.part = Some(s);
        self
    }

    pub fn comment(mut self, comment: &str) -> Self {
        let mut s = CommentString::new();
        s.push_str(comment)
            .expect("comment string too long for ErrorInfo");
        self.comment = Some(s);
        self
    }
}

impl Default for ErrorInfo {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for ErrorInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(part) = &self.part {
            write!(f, "[{}] ", part)?;
        }

        if let Some(expected) = &self.expected {
            write!(f, "expected: {}", expected)?;
        }

        if let Some(actual) = &self.actual {
            if self.expected.is_some() {
                write!(f, ", ")?;
            }
            write!(f, "actual: {}", actual)?;
        }

        if let Some(comment) = &self.comment {
            if self.expected.is_some() || self.actual.is_some() {
                write!(f, " | ")?;
            }
            write!(f, "{}", comment)?;
        }

        Ok(())
    }
}

#[derive(Debug, Clone)]
pub enum RF_error {
    Rs_OOB(ErrorInfo),
    Rd_OOB(ErrorInfo),
    Idx_OOB(ErrorInfo),
}

#[derive(Debug, Clone)]
pub enum IMEM_error {
    PC_OOB(ErrorInfo),
}

#[derive(Debug, Clone)]
pub enum DramError {
    WriteTypeError(ErrorInfo),
    ReadTypeError(ErrorInfo),
}

pub enum AGU_error {
    FPTR_uninit(ErrorInfo),
    FPTR_OOB(ErrorInfo),
}

impl fmt::Display for RF_error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RF_error::Idx_OOB(info) => {
                write!(f, "Instruction IDX field out of bounds: {}", info)
            }
            RF_error::Rs_OOB(info) => {
                write!(f, "Instruction RS field out of bounds: {}", info)
            }
            RF_error::Rd_OOB(info) => {
                write!(f, "Instruction RD field out of bounds: {}", info)
            }
        }
    }
}

impl fmt::Display for DramError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DramError::WriteTypeError(info) => {
                write!(f, "DRAM write type error: {}", info)
            }
            DramError::ReadTypeError(info) => {
                write!(f, "DRAM read type error: {}", info)
            }
        }
    }
}
