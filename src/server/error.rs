use std::sync::mpsc::SendError;

#[derive(Debug)]
pub enum ServerError {
    UserNotFound(String),
    SendError(String),
    WsError(tungstenite::error::Error),
    IOError(std::io::Error),
}

impl<T> From<SendError<T>> for ServerError {
    fn from(err: SendError<T>) -> Self {
        ServerError::SendError(err.to_string())
    }
}

impl From<tungstenite::error::Error> for ServerError {
    fn from(err: tungstenite::error::Error) -> Self {
        ServerError::WsError(err)
    }
}

impl From<std::io::Error> for ServerError {
    fn from(err: std::io::Error) -> Self {
        ServerError::IOError(err)
    }
}
