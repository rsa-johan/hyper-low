/// Errors returned by the compiler. `Display` gives a user-facing message.
#[derive(Debug)]
pub enum ErrorSignals {
    /// Any other error; file-level errors are wrapped here with the file name prefixed.
    Generic(String),
    /// A directory was expected.
    NotDir,

    /// Header line does not start with `#`; reported to callers inside [`At`](Self::At).
    InvalidHeader,

    /// Header is neither `#protocol=` nor `#ignore=`; reported to callers inside [`At`](Self::At).
    UnidentifiableHeader,
    /// Empty header line; reported to callers inside [`At`](Self::At).
    EmptyHeader,

    /// Not currently returned.
    RootFileNotFound,
    /// Not currently returned.
    UnsupportedProtocol,
    /// The input was skipped by an `#ignore=` header; not a failure.
    NoError,

    /// Parse or validation error at a 1-based source line.
    At {
        /// 1-based line number.
        line: usize,
        /// Error message.
        msg: String,
    },
}

impl ErrorSignals {
    pub(crate) fn at(line: usize, msg: impl Into<String>) -> Self {
        Self::At { line, msg: msg.into() }
    }

    /// Prefixes the message with a file name; `NoError` (ignored file) passes through.
    pub(crate) fn in_file(self, name: &str) -> Self {
        match self {
            Self::NoError => self,
            e => Self::Generic(format!("{name}: {e}")),
        }
    }
}

impl std::fmt::Display for ErrorSignals {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Generic(s) => write!(f, "{s}"),
            Self::NotDir => write!(f, "not a directory"),
            Self::InvalidHeader => write!(f, "invalid header (expected `#protocol=<name>`)"),
            Self::UnidentifiableHeader => write!(f, "unidentifiable header"),
            Self::EmptyHeader => write!(f, "empty header"),
            Self::RootFileNotFound => write!(f, "root file not found"),
            Self::UnsupportedProtocol => write!(f, "unsupported protocol (only `can` is supported)"),
            Self::NoError => write!(f, "ignored"),
            Self::At { line, msg } => write!(f, "line {line}: {msg}"),
        }
    }
}

impl std::error::Error for ErrorSignals {}


// impl From<ErrorSignals> for String {
//     fn from(value: ErrorSignals) -> Self {
//         match value {
//             ErrorSignals::InvalidHeader => "Invalid root file header found".to_string(),
//             ErrorSignals::UnidentifiableHeader => "UnidentifiableHeader".to_string(),
//             ErrorSignals::NotDir => "NotDir".to_string()
//             _ => "nil".to_string()
//         }
//     }
// }