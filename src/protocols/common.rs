use crate::error::ErrorSignals;

const HEADER_IDENTIFIER: &str = "#";


const HEADER_PROTOCOL_DEF: &str = "protocol=";
const HEADER_PROTOCOL_DEF_LEN: usize = HEADER_PROTOCOL_DEF.len();
const HEADER_IGNORE_DEF: &str = "ignore=";


#[derive(Debug, PartialEq)]
pub(crate) enum Protocols {
    Can,
    Nil
}

impl From<&str> for Protocols {
    fn from(value: &str) -> Self {
        match value {
            "can" => Self::Can,
            _ => Self::Nil
        }
    }
}


pub(crate) fn type_protocol(root_header: &str) -> Result<Protocols, ErrorSignals> {
    let root_header = root_header.trim();
    if root_header.is_empty() {
        return Err(ErrorSignals::EmptyHeader);
    }
    if !root_header.starts_with(HEADER_IDENTIFIER) {
        return Err(ErrorSignals::InvalidHeader);
    }

    let header_cnt = &root_header[HEADER_IDENTIFIER.len()..];
    if HEADER_IGNORE_DEF == header_cnt {
        return Err(ErrorSignals::NoError);
    }

    if !header_cnt.starts_with(HEADER_PROTOCOL_DEF) {
        return Err(ErrorSignals::UnidentifiableHeader);
    }


    let header_val = &header_cnt[HEADER_PROTOCOL_DEF_LEN..];

    return Ok(header_val.trim().to_ascii_lowercase().as_str().into());
}