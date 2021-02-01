use super::{session::UserSession, skribbl::SkribblState, GameOpts};
use crate::{
    data,
    network::{ClientMsg, NetworkMessage, ServerMsg},
    StopSignal,
};
use actix::{io::FramedWrite, prelude::*};
use data::Username;
use futures_util::stream::StreamExt;
use log::{debug, error, info};
use nanoid::nanoid;
use std::net::SocketAddr;
use std::{cmp::min, collections::HashMap};
use tokio::net::{TcpListener, TcpStream};
use tokio_util::codec::{FramedRead, LinesCodec};

pub const ROUND_DURATION: u64 = 120;

#[derive(actix::Message)]
#[rtype(result = "Option<ServerResponse>")]
pub enum ServerEvent {
    /// Notify server of newly connected client.
    UserJoined(Username, SocketAddr, Addr<UserSession>),

    /// Notify server of disconnecting client. First argument holds client id.
    UserLeft(Username),

    /// Add client to matchmaking queue
    UserQueue(Username),
    // /// Add player to room
    // JoinRoom(Username, String)
}

pub enum UserEvent {
    JoinServer(),

    LeaveServer,

    Queue,

    LeaveQueue,
}

pub enum ServerResponse {
    AssignId(String),
}

pub struct Player {
    session: Addr<UserSession>,

    peer_addr: SocketAddr,
}

#[derive(Default)]
pub struct GameServer {
    /// holds the default game configuration
    default_game_opts: GameOpts,

    /// holds all player session actors connected by username
    connected_players: HashMap<Username, Player>,

    /// list of players searching for a game
    game_queue: Vec<Username>,

    /// hold game rooms by thier key
    game_rooms: HashMap<String, GameRoom>,
}

/// define actor for `GameServer`
impl Actor for GameServer {
    type Context = Context<Self>;

    fn started(&mut self, _ctx: &mut Self::Context) {
        // let state = match &mut self.game_state {
        //     GameState::Skribbl(state) => state,
        //     _ => return Ok(()),
        // };

        // let remaining_time = state.remaining_time();
        // let revealed_char_cnt = state.revealed_characters().len();

        // if remaining_time <= 0 {
        //     let old_word = state.current_word().to_string();
        //     if let Some(ref mut drawing_user) = state.player_states.get_mut(&state.drawing_user) {
        //         drawing_user.score += 50;
        //     }

        //     state.next_turn();
        //     let state = self.game_state.skribbl_state().unwrap().clone();
        //     self.lines.clear();
        //     tokio::try_join!(
        //         self.broadcast(ToClientMsg::SkribblStateChanged(state)),
        //         self.broadcast(ToClientMsg::ClearCanvas),
        //         self.broadcast_system_msg(format!("The word was: \"{}\"", old_word)),
        //     )?;
        // } else if remaining_time <= (ROUND_DURATION / 4) as u32 && revealed_char_cnt < 2
        //     || remaining_time <= (ROUND_DURATION / 2) as u32 && revealed_char_cnt < 1
        // {
        //     state.reveal_random_char();
        //     let state = state.clone();
        //     self.broadcast(ToClientMsg::SkribblStateChanged(state))
        //         .await?;
        // }

        // self.broadcast(ToClientMsg::TimeChanged(remaining_time as u32));
    }
}

/// handle stop event for game server
impl Handler<StopSignal> for GameServer {
    type Result = ();
    fn handle(&mut self, _: StopSignal, ctx: &mut Self::Context) -> Self::Result {
        ctx.stop();
    }
}

#[derive(actix::Message)]
#[rtype(result = "()")]
struct TcpConnect(pub TcpStream, pub std::net::SocketAddr);

/// Handle stream of TcpStream's
impl Handler<TcpConnect> for GameServer {
    type Result = ();

    fn handle(&mut self, msg: TcpConnect, ctx: &mut Context<Self>) {
        debug!("new client connection: {}", msg.1);

        let server_ref = ctx.address();
        let peer_addr = msg.1;
        let (r, w) = tokio::io::split(msg.0);

        ctx.spawn(
            async move {
                let mut read_stream_by_line = FramedRead::new(r, LinesCodec::new());
                let username = loop {
                    match read_stream_by_line.next().await {
                        Some(Ok(line)) => {
                            if !line.is_empty() {
                                break line;
                            }
                        }
                        // We didn't get a line so we return early here.
                        _ => {
                            error!(
                                "Failed to get username from {}. Client disconnected.",
                                peer_addr
                            );
                            return;
                        }
                    }
                };

                debug!("({:?}) recieved username <> {}", peer_addr, username);

                UserSession::create(move |ctx| {
                    UserSession::add_stream(
                        FramedRead::new(
                            read_stream_by_line.into_inner(),
                            NetworkMessage::<ClientMsg>::new(),
                        ),
                        ctx,
                    );

                    UserSession::new(
                        username.into(),
                        server_ref,
                        FramedWrite::new(w, NetworkMessage::<ServerMsg>::new(), ctx),
                        peer_addr,
                    )
                });
            }
            .into_actor(self),
        );
    }
}

impl Handler<ServerEvent> for GameServer {
    type Result = Option<ServerResponse>;

    fn handle(&mut self, msg: ServerEvent, _ctx: &mut Self::Context) -> Self::Result {
        match msg {
            ServerEvent::UserJoined(username, peer_addr, session) => {
                info!("{} joined the server", username);

                let id = self.add_player(username, Player { session, peer_addr });
                Some(ServerResponse::AssignId(id))
            }

            ServerEvent::UserLeft(username) => {
                self.remove_player(&username);
                None
            }

            ServerEvent::UserQueue(username) => {
                self.game_queue.push(username);
                None
            } // ServerEvent::JoinRoom( username, room_key) => {
              //     let player  = if let Some(player) = self.connected_players.get(&username) {
              //         player
              //     } else {
              //         return None;
              //     };

              //     if let Some(room) = self.game_rooms.get_mut(room_key) {
              // if let GameState::InGame(ref mut state) = self.game_state {
              // state.add_player(session.username.clone());
              // }
              //     }
              // //         let state = state.clone();
              // //         tokio::try_join!(
              // //             self.broadcast(ToClientMsg::SkribblStateChanged(state)),
              // //             self.broadcast_system_msg(format!("{} joined", session.username)),
              // //         )?;
              // //     }

              // //     let initial_state = InitialState {
              // //         lines: self.lines.clone(),
              // //         skribbl_state: self.game_state.skribbl_state().cloned(),
              // //         dimensions: self.dimensions,
              // //     };
              // //     session
              // //         .send(ToClientMsg::InitialState(initial_state))
              // //         .await?;
              // //     self.connected_players
              // //         .insert(session.username.clone(), session);
              // //     Ok(())

              //     None
              // }
        }
    }
}

impl GameServer {
    fn new(default_game_opts: GameOpts) -> Self {
        Self {
            default_game_opts,
            ..Default::default()
        }
    }

    // async fn on_new_message(&mut self, username: Username, msg: data::Message) -> Result<()> {
    //     let mut should_broadcast = true;
    //     match self.game_state {
    //         GameState::Skribbl(ref mut state) => {
    //             let can_guess = state.can_guess(&username);
    //             let remaining_time = state.remaining_time();
    //             let current_word = state.current_word().to_string();
    //             let noone_already_solved = state
    //                 .player_states
    //                 .iter()
    //                 .all(|(_, player)| !player.has_solved);

    //             if let Some(player_state) = state.player_states.get_mut(&username) {
    //                 if can_guess && msg.text().eq_ignore_ascii_case(&current_word) {
    //                     should_broadcast = false;
    //                     if noone_already_solved {
    //                         state.round_end_time -= remaining_time as u64 / 2;
    //                     }
    //                     player_state.on_solve(remaining_time);
    //                     let all_solved = state.did_all_solve();
    //                     if all_solved {
    //                         state.next_turn();
    //                     }
    //                     let state = state.clone();
    //                     tokio::try_join!(
    //                         self.broadcast(ToClientMsg::SkribblStateChanged(state)),
    //                         self.broadcast_system_msg(format!("{} guessed it!", username)),
    //                     )?;
    //                     if all_solved {
    //                         self.lines.clear();
    //                         tokio::try_join!(
    //                             self.broadcast(ToClientMsg::ClearCanvas),
    //                             self.broadcast_system_msg(format!(
    //                                 "The word was: \"{}\"",
    //                                 current_word
    //                             ))
    //                         )?;
    //                     }
    //                 } else if is_very_close_to(msg.text().to_string(), current_word.to_string()) {
    //                     should_broadcast = false;
    //                     if can_guess {
    //                         self.send_to(
    //                             &username,
    //                             ToClientMsg::NewMessage(Message::SystemMsg(
    //                                 "You're very close!".to_string(),
    //                             )),
    //                         )
    //                         .await?;
    //                     }
    //                 }
    //             }
    //         }
    //         GameState::FreeDraw => {
    //             if let Some(words) = &self.words {
    //                 let skribbl_state = SkribblState::new(
    //                     self.connected_players
    //                         .keys()
    //                         .cloned()
    //                         .collect::<Vec<Username>>(),
    //                     words.clone(),
    //                 );
    //                 self.game_state = GameState::Skribbl(skribbl_state.clone());
    //                 self.broadcast(ToClientMsg::SkribblStateChanged(skribbl_state))
    //                     .await?;
    //             }
    //         }
    //     }

    //     if should_broadcast {
    //         self.broadcast(ToClientMsg::NewMessage(msg)).await?;
    //     }

    //     Ok(())
    // }

    // async fn on_to_srv_msg(&mut self, username: Username, msg: ToServerMsg) -> Result<()> {
    //     match msg {
    //         ToServerMsg::CommandMsg(msg) => {
    //             self.on_command_msg(&username, &msg).await?;
    //         }
    //         ToServerMsg::NewMessage(message) => {
    //             self.on_new_message(username, message).await?;
    //         }
    //         ToServerMsg::NewLine(line) => {
    //             self.lines.push(line);
    //             self.broadcast(ToClientMsg::NewLine(line)).await?;
    //         }
    //         ToServerMsg::ClearCanvas => {
    //             self.lines.clear();
    //             self.broadcast(ToClientMsg::ClearCanvas).await?;
    //         }
    //     }
    //     Ok(())
    // }

    // /// send a Message::SystemMsg to all active sessions
    // async fn broadcast_system_msg(&self, msg: String) -> Result<()> {
    //     self.broadcast(ToClientMsg::NewMessage(Message::SystemMsg(msg)))
    //         .await?;
    //     Ok(())
    // }

    // /// send a ToClientMsg to a specific session
    // pub async fn send_to(&self, user: &Username, msg: ToClientMsg) -> Result<()> {
    //     self.connected_players
    //         .get(user)
    //         .ok_or(ServerError::UserNotFound(user.to_string()))?
    //         .send(msg)
    //         .await?;
    //     Ok(())
    // }

    // /// broadcast a ToClientMsg to all running sessions
    // async fn broadcast(&self, msg: ToClientMsg) -> Result<()> {
    //     futures_util::future::try_join_all(
    //         self.connected_players
    //             .iter()
    //             .map(|(_, session)| session.send(msg.clone())),
    //     )
    //     .await?;
    //     Ok(())
    // }

    fn add_player(&mut self, mut username: Username, player: Player) -> String {
        let id = nanoid!();
        debug!("({}): assigning id <> {}", player.peer_addr, id);

        username.set_identifier(id.clone());
        self.connected_players.insert(username, player);

        id
    }

    // fn get_room_with_player(&self, username: &Username) -> Option<&GameRoom> {
    //     self.game_rooms.get("main")
    // }

    fn remove_player(&mut self, username: &Username) {
        if let Some(player) = self.connected_players.remove(username) {
            player.session.do_send(StopSignal)
        }

        if let Some(room) = self.game_rooms.get_mut("main") {
            let state = match &mut room.state {
                GameState::InGame(state) => state,
                _ => return,
            };

            if state.is_drawing(username) {
                state.next_turn();
            }

            state.remove_user(username);

            // self.broadcast(ToClientMsg::SkribblStateChanged(state));
        }
    }

    /// start termibbl server actor
    pub fn start(server_listener: Box<TcpListener>, default_game_opts: GameOpts) -> Addr<Self> {
        GameServer::create(move |ctx| {
            // listen and handle incoming connections in async thread.
            ctx.add_message_stream(Box::leak(server_listener).incoming().map(|stream| {
                let st = stream.unwrap();
                let addr = st.peer_addr().unwrap();

                TcpConnect(st, addr)
            }));

            GameServer::new(default_game_opts)
        })
    }
}

#[derive(Debug)]
pub enum GameState {
    FreeDraw,
    Lobby,
    InGame(SkribblState),
}

impl GameState {
    fn skribbl_state(&self) -> Option<&SkribblState> {
        match self {
            GameState::InGame(state) => Some(state),
            _ => None,
        }
    }
}

pub struct GameRoom {
    /// hold the current game state
    state: GameState,
    game_opts: GameOpts,
    private: bool,
    // player_list: HashMap<username, >,
}

/// helpful functions for `GameServer`
impl GameServer {}

type Result<T> = std::result::Result<T, ServerError>;

#[derive(Debug)]
pub enum ServerError {
    UserNotFound(String),
    SendError(String),
    WsError(tungstenite::error::Error),
    IOError(std::io::Error),
}

impl<T> From<tokio::sync::mpsc::error::SendError<T>> for ServerError {
    fn from(err: tokio::sync::mpsc::error::SendError<T>) -> Self {
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

fn is_very_close_to(a: String, b: String) -> bool {
    return levenshtein_distance(a, b) <= 1;
}

fn levenshtein_distance(a: String, b: String) -> usize {
    let w1 = a.chars().collect::<Vec<_>>();
    let w2 = b.chars().collect::<Vec<_>>();

    let a_len = w1.len() + 1;
    let b_len = w2.len() + 1;

    let mut matrix = vec![vec![0]];

    for i in 1..a_len {
        matrix[0].push(i);
    }
    for j in 1..b_len {
        matrix.push(vec![j]);
    }

    for (j, i) in (1..b_len).flat_map(|j| (1..a_len).map(move |i| (j, i))) {
        let x: usize = if w1[i - 1].eq_ignore_ascii_case(&w2[j - 1]) {
            matrix[j - 1][i - 1]
        } else {
            1 + min(
                min(matrix[j][i - 1], matrix[j - 1][i]),
                matrix[j - 1][i - 1],
            )
        };
        matrix[j].push(x);
    }
    matrix[b_len - 1][a_len - 1]
}
