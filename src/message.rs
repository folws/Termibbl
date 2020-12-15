use crate::{data, server::skribbl::SkribblState};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone, actix::Message)]
#[rtype(result = "()")]
pub enum ToClientMsg {
    NewMessage(data::Message),
    NewLine(data::Line),
    InitialState(InitialState),
    SkribblRoundEnd(SkribblState),
    SkribblRoundStart(Option<String>, SkribblState),
    GameOver(SkribblState),
    ClearCanvas,
    TimeChanged(u32),
    Kick(String),
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum ToServerMsg {
    NewMessage(data::Message),
    CommandMsg(data::CommandMsg),
    NewLine(data::Line),
    ClearCanvas,
    Play,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct InitialState {
    pub dimensions: (usize, usize),
    pub number_of_rounds: usize,
    pub skribbl_state: Option<SkribblState>,
    pub player_id: usize,
}
