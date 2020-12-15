pub type Result<T> = std::result::Result<T, ErrorKind>;

/// Wrapper for all errors that can occur in `Termibbl`.
#[derive(Debug)]
pub enum ErrorKind {
    SendError(String),
    CrosstermError(crossterm::ErrorKind),
    IOError(std::io::Error),
    WebSocketError(tungstenite::error::Error),
}

impl<T> From<tokio::sync::mpsc::error::SendError<T>> for ErrorKind {
    fn from(e: tokio::sync::mpsc::error::SendError<T>) -> Self {
        ErrorKind::SendError(e.to_string())
    }
}
impl From<crossterm::ErrorKind> for ErrorKind {
    fn from(e: crossterm::ErrorKind) -> Self {
        ErrorKind::CrosstermError(e)
    }
}

impl From<std::io::Error> for ErrorKind {
    fn from(e: std::io::Error) -> Self {
        ErrorKind::IOError(e)
    }
}

impl From<tungstenite::error::Error> for ErrorKind {
    fn from(e: tungstenite::error::Error) -> Self {
        ErrorKind::WebSocketError(e)
    }
}
