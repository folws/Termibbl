use crate::{data, network::ChatMessage, server::skribbl::SkribblState};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum ToClientMsg {
    NewMessage(data::Message),
    NewLine(data::Line),
    InitialState(InitialState),
    SkribblStateChanged(SkribblState),
    GameOver(SkribblState),
    ClearCanvas,
    TimeChanged(u32),
}
#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum ToServerMsg {
    NewMessage(data::Message),
    CommandMsg(data::CommandMsg),
    NewLine(data::Line),
    ClearCanvas,
}
/// Client -> Server
#[derive(actix::Message, Debug, Serialize, Deserialize, Clone)]
#[rtype(result = "()")]
pub enum ClientMsg {
    Chat(ChatMessage),
    Draw(data::Draw),
    JoinRoom(String),
    // Command(CommandMessage),
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct InitialState {
    pub lines: Vec<data::Line>,
    pub dimensions: (usize, usize),
    pub skribbl_state: Option<SkribblState>,
}
