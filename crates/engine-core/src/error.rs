#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CliError {
    Usage,
}

impl CliError {
    pub fn exit_code(self) -> i32 {
        match self {
            Self::Usage => 2,
        }
    }
}
