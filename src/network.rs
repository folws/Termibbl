use std::{fmt, marker::PhantomData};

use bytes::{BufMut, BytesMut};
use serde::{Deserialize, Serialize};
use tokio_util::codec::{Decoder, Encoder};

use crate::data::{Draw, Username};

// +----------+--------------------------------+
// | len: u32 |          frame payload         |
// +----------+--------------------------------+
pub struct NetworkMessage<T> {
    __: PhantomData<T>,
}

impl<T> NetworkMessage<T> {
    pub fn new() -> Self {
        Self { __: PhantomData }
    }
}

impl<T> Encoder<T> for NetworkMessage<T>
where
    T: Serialize,
{
    type Error = bincode::Error;

    fn encode(&mut self, msg: T, buf: &mut BytesMut) -> Result<(), Self::Error> {
        let size: usize = bincode::serialized_size(&msg)? as usize;
        let msg = bincode::serialize(&msg)?;

        buf.reserve(size);
        // buf.put_u16(msg.len() as u16);
        buf.put(&msg[..]);

        Ok(())
    }
}

impl<T> Decoder for NetworkMessage<T>
where
    for<'de> T: Deserialize<'de>,
{
    type Item = T;
    type Error = bincode::Error;

    fn decode(&mut self, bytes: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        if bytes.is_empty() {
            Ok(None)
        } else {
            let decoded: T = bincode::deserialize(bytes)?;

            Ok(Some(decoded))
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ChatMessage {
    SystemMsg(String),
    UserMsg(Username, String),
}

impl ChatMessage {
    pub fn text(&self) -> &str {
        match self {
            ChatMessage::SystemMsg(msg) => &msg,
            ChatMessage::UserMsg(_, msg) => &msg,
        }
    }

    pub fn is_system(&self) -> bool {
        matches!(self, ChatMessage::SystemMsg(_))
    }

    pub fn username(&self) -> Option<&Username> {
        match self {
            ChatMessage::UserMsg(username, _) => Some(username),
            _ => None,
        }
    }
}

impl fmt::Display for ChatMessage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ChatMessage::SystemMsg(msg) => write!(f, "{}", msg),
            ChatMessage::UserMsg(user, msg) => write!(f, "{}: {}", user, msg),
        }
    }
}

/// Client -> Server
#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum ClientMsg {
    Chat(ChatMessage),
    Draw(Draw),
    JoinRoom(String),
    // Command(CommandMessage),
}

/// Server -> Client
#[derive(actix::Message, Debug, Serialize, Deserialize, Clone)]
#[rtype(result = "()")]
pub enum ServerMsg {
    // Game(GameAction),
// MatchMake,
// Disconnect,
}
