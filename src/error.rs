use std::fmt;

/// Convenient result alias used throughout the crate.
pub type Result<T = (), E = Error> = std::result::Result<T, E>;

/// Every error love2drs can produce.
#[derive(Debug)]
pub enum Error {
    /// A file could not be read or written.
    Io {
        /// The path that was being accessed.
        path: String,
        /// The underlying I/O error.
        source: std::io::Error,
    },
    /// An image could not be decoded or encoded.
    Image(String),
    /// A font could not be parsed.
    Font(String),
    /// Audio device or decoding failure.
    Audio(String),
    /// GPU / window setup failure, or a shader that failed to compile.
    Graphics(String),
    /// The window or event loop could not be created.
    Window(String),
    /// Any error raised by game code (see [`Error::custom`]).
    Custom(String),
}

impl Error {
    /// Wraps any displayable value as an [`Error::Custom`].
    pub fn custom(msg: impl fmt::Display) -> Self {
        Error::Custom(msg.to_string())
    }

    pub(crate) fn io(path: impl AsRef<std::path::Path>, source: std::io::Error) -> Self {
        Error::Io {
            path: path.as_ref().display().to_string(),
            source,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io { path, source } => write!(f, "could not access '{path}': {source}"),
            Error::Image(m) => write!(f, "image error: {m}"),
            Error::Font(m) => write!(f, "font error: {m}"),
            Error::Audio(m) => write!(f, "audio error: {m}"),
            Error::Graphics(m) => write!(f, "graphics error: {m}"),
            Error::Window(m) => write!(f, "window error: {m}"),
            Error::Custom(m) => f.write_str(m),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

impl From<image::ImageError> for Error {
    fn from(e: image::ImageError) -> Self {
        Error::Image(e.to_string())
    }
}
