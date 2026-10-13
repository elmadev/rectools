mod reader;

pub mod across;
pub mod lev;
pub mod lgr;
pub mod pic;
pub mod rec;
pub mod res;
pub mod wav;

use std::fmt;
use std::sync::Arc;

// What a level is drawn with: an LGR, or Across's graphics
pub enum Graphics {
    Elma(Box<lgr::Lgr>),
    // With the level's descriptor, which picks the ground
    Across(Arc<across::gfx::AcrossGraphics>, u8),
}

impl Graphics {
    pub fn is_across(&self) -> bool {
        matches!(self, Graphics::Across(..))
    }

    pub fn palette(&self) -> &[u8; 768] {
        match self {
            Graphics::Elma(lgr) => &lgr.palette,
            Graphics::Across(gfx, _) => &gfx.palette,
        }
    }
}

// Why a file couldn't be read, with the byte it went wrong at when that is known
#[derive(Debug, Clone, PartialEq)]
pub enum Error {
    // The file ends before what it says is there, which would start at byte `at`
    Eof { at: usize },
    Invalid { message: String, at: Option<usize> },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Eof { at } => write!(f, "unexpected end of file (at byte {at})"),
            Error::Invalid { message, at: None } => write!(f, "{message}"),
            Error::Invalid {
                message,
                at: Some(at),
            } => write!(f, "{message} (at byte {at})"),
        }
    }
}

impl std::error::Error for Error {}

impl Error {
    pub(crate) fn invalid(message: impl Into<String>) -> Error {
        Error::Invalid {
            message: message.into(),
            at: None,
        }
    }
}

// An error where no byte is to blame, like a text file's or a whole file's
pub(crate) fn invalid<T>(message: impl Into<String>) -> Result<T, Error> {
    Err(Error::Invalid {
        message: message.into(),
        at: None,
    })
}
