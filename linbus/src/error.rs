use std::fmt;

#[derive(Debug)]
pub enum LinbusError {
    Io(std::io::Error),
    Nix(nix::Error),
    AuthFailed(String),
    ProtocolError(String),
    NameRequestFailed(u32),
    MethodError { name: String, message: String },
    Timeout,
    InvalidSignature(String),
}

impl fmt::Display for LinbusError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "I/O error: {}", e),
            Self::Nix(e) => write!(f, "nix error: {}", e),
            Self::AuthFailed(s) => write!(f, "auth failed: {}", s),
            Self::ProtocolError(s) => write!(f, "protocol error: {}", s),
            Self::NameRequestFailed(code) => write!(f, "RequestName failed (code={})", code),
            Self::MethodError { name, message } => write!(f, "D-Bus error {}: {}", name, message),
            Self::Timeout => write!(f, "timeout"),
            Self::InvalidSignature(s) => write!(f, "invalid signature: {}", s),
        }
    }
}

impl std::error::Error for LinbusError {}

impl From<std::io::Error> for LinbusError {
    fn from(e: std::io::Error) -> Self { Self::Io(e) }
}

impl From<nix::Error> for LinbusError {
    fn from(e: nix::Error) -> Self { Self::Nix(e) }
}
