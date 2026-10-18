use strum_macros::EnumIter;


#[derive(EnumIter, Debug, Clone)]
pub enum KeyWords {
    Type,
    Def,
    Hash,
    Dot,
    Frame,
    Filter,
    Spec,
    Fn,
    Nil
}

#[derive(EnumIter, Debug, Clone)]
pub enum FrameKeyWords {
    Size,
    Id,
    Data,
    Ext,
    Fd,
    Filter,
    Rsp,
    SizeMark,
    Nil
}


impl From<KeyWords> for &str {
    fn from(value: KeyWords) -> Self {
        match value {
            KeyWords::Type => "type",
            KeyWords::Def => "def",
            KeyWords::Hash => "#",
            KeyWords::Dot => ".",
            KeyWords::Frame => "frame",
            KeyWords::Filter => "filter",
            KeyWords::Spec => "spec",
            KeyWords::Fn => "fn",
            KeyWords::Nil => "nil",
        }
    }
}

impl From<&str> for KeyWords {
    fn from(value: &str) -> Self {
        match value.to_lowercase().as_str() {
            "type" => KeyWords::Type,
            "def" => KeyWords::Def,
            "#" => KeyWords::Hash,
            "." => KeyWords::Dot,
            "frame" => KeyWords::Frame,
            "filter" => KeyWords::Filter,
            "spec" => KeyWords::Spec,
            "fn" => KeyWords::Fn,
            _ => KeyWords::Nil
        }
    }
}


impl From<FrameKeyWords> for &str {
    fn from(value: FrameKeyWords) -> Self {
        match value {
            FrameKeyWords::Size => "size",
            FrameKeyWords::Id => "id",
            FrameKeyWords::Data => "data",
            FrameKeyWords::Ext => "ext",
            FrameKeyWords::Fd => "fd",
            FrameKeyWords::Filter => "filter",
            FrameKeyWords::Rsp => "rsp",
            FrameKeyWords::SizeMark => "'",
            FrameKeyWords::Nil => "nil",
        }
    }
}

impl From<&str> for FrameKeyWords {
    fn from(value: &str) -> Self {
        match value.to_lowercase().as_str() {
            "size" => FrameKeyWords::Size,
            "id" => FrameKeyWords::Id,
            "data" => FrameKeyWords::Data,
            "ext" => FrameKeyWords::Ext,
            "fd" => FrameKeyWords::Fd,
            "filter" => FrameKeyWords::Filter,
            "rsp" => FrameKeyWords::Rsp,
            "'" => FrameKeyWords::SizeMark,
            _ => FrameKeyWords::Nil
        }
    }
}