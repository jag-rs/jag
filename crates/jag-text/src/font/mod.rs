pub mod face;
pub mod loader;
pub mod metrics;

pub use face::FontFace;
pub use loader::{FontCache, FontKey, load_system_default_font};
pub use metrics::{FontMetrics, ScaledFontMetrics};

use core::fmt;

/// Errors that can occur while working with fonts.
#[derive(Debug)]
pub enum FontError {
    Io(std::io::Error),
    InvalidFont,
}

impl fmt::Display for FontError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FontError::Io(err) => write!(f, "font I/O error: {err}"),
            FontError::InvalidFont => write!(f, "invalid font data"),
        }
    }
}

impl std::error::Error for FontError {}

impl From<std::io::Error> for FontError {
    fn from(err: std::io::Error) -> Self {
        FontError::Io(err)
    }
}

/// Convenient result alias for font-related operations.
pub type Result<T> = std::result::Result<T, FontError>;
