extern crate actix;
extern crate tokio;

use crate::{
    data, data::Username, message::InitialState, message::ToClientMsg, message::ToServerMsg,
};
use actix::prelude::*;
use constants::ROUND_DURATION;
use data::Either;
use error::ServerError;
use futures_util::{stream::StreamExt, TryStreamExt};
use log::{debug, info};
use rand::Rng;
use session::{JoinRoom, ServerUser, UserSession};
use skribbl::Skribbl;
use std::cell::RefCell;
use std::iter::Cycle;
use std::vec::IntoIter;
use std::{
    collections::{BinaryHeap, HashMap, VecDeque},
    net::SocketAddr,
    time::Duration,
};
use std::{io::Read, path::PathBuf};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::accept_async;

mod error;
mod session;
pub(crate) mod skribbl;

pub mod constants {
    pub const ROUND_DURATION: u64 = 120;
    pub const ROUNDS: usize = 3;
}

pub type Result<T> = std::result::Result<T, ServerError>;

fn read_words_file(path: &PathBuf) -> Result<Vec<String>> {
    let mut file = std::fs::File::open(path)?;
    let mut words = String::new();

    info!("reading words from file");

    file.read_to_string(&mut words)?;
    debug!("read {} words from file", words.len());

    Ok(words
        .lines()
        .map(|x| x.trim().to_string())
        .filter(|x| !x.is_empty())
        .collect::<Vec<String>>())
}

/// Define tcp server that will accept incoming tcp connection and create
/// game room and user actors.
#[actix_rt::main]
pub async fn run(
    port: u32,
    display_public_ip: bool,
    word_file: Option<PathBuf>,
    default_dimensions: (usize, usize),
    default_round_duration: u64,
    default_number_of_rounds: usize,
) -> Result<()> {
    // display public ip
    if display_public_ip {
        actix::spawn(async move {
            if let Ok(res) = reqwest::get("http://ifconfig.me").await {
                if let Ok(ip) = res.text().await {
                    println!("Your public IP is {}:{}", ip, port);
                    info!(
                        "You can find out your private IP by running \"ip addr\" in the terminal"
                    );
                }
            }
        });
    }

    let default_words = word_file.map(|path| read_words_file(&path).unwrap());
    let default_game_opts = GameOpts {
        dimensions: default_dimensions,
        words: Either::Left(default_words.unwrap_or_else(Vec::new)),
        number_of_rounds: default_number_of_rounds,

        round_duration: default_round_duration,
    };

    // start tcp listener on given port
    let addr = format!("127.0.0.1:{}", port);

    let server_listener = Box::new(
        TcpListener::bind(&addr)
            .await
            .expect("Could not start webserver (could not bind)"),
    );

    info!("Running Termibbl server on {}...", addr);

    // start termibbl server actor
    let server = GameServer::new(default_game_opts).start();
    let serv = server.clone();

    // listen and handle incoming connections in async thread.
    actix::spawn(async move {
        Box::leak(server_listener)
            .incoming()
            .map_err(|e| println!("error occured, {:?}", e))
            .for_each(|socket| async {
                actix::spawn(handle_connection(socket.unwrap(), server.clone()));
            })
            .await;
    });

    tokio::signal::ctrl_c().await.unwrap();
    println!("Ctrl-C received. Stopping..");

    // gracefully exit
    serv.do_send(StopSignal);
    Ok(())
}

/// handle incoming connections, and start `UserSession` actor on success.
async fn handle_connection(stream: TcpStream, server: Addr<GameServer>) {
    let peer_addr = stream.peer_addr().unwrap();

    info!("new client connection: {}", peer_addr);

    let ws_stream = accept_async(stream)
        .await
        .expect("error whilst websocket handshake");

    debug!("({}) Connection upgraded to websocket", peer_addr);

    let (ws_write, mut ws_read) = ws_stream.split();

    // first, wait for the client to send his username
    if let Some(username) = ws_read
        .next()
        .await
        .ok_or_else(|| info!("no username recieved from client {}.", peer_addr))
        .and_then(|val| {
            val.map_err(|e| eprintln!("error occured whilst trying to recieve username, {:?}", e))
                .map(|msg| {
                    if let tungstenite::Message::Text(username) = msg {
                        username.trim().to_owned()
                    } else {
                        "".to_owned()
                    }
                })
        })
        .ok()
        .filter(|val| !val.is_empty())
        .map(String::into)
    {
        info!("({}) client joined the server as {}.", peer_addr, username);

        // then, create a session actor and send that session to the server's main thread (actor)
        UserSession::create(move |ctx| {
            UserSession::add_stream(ws_read.map(session::WebSocketMessage), ctx);
            UserSession::new(server, username, ws_write, peer_addr)
        });
    } else {
        // close connection
        ws_write
            .reunite(ws_read)
            .unwrap()
            .close(None)
            .await
            .unwrap()
    }
}

#[derive(Message)]
#[rtype(result = "()")]
struct StopSignal;

struct ServerRoom {
    addr: Addr<GameRoom>,
    user_count: usize,
}

impl Eq for ServerRoom {}

impl Ord for ServerRoom {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.user_count.cmp(&other.user_count).reverse()
    }
}

impl PartialOrd for ServerRoom {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for ServerRoom {
    fn eq(&self, other: &Self) -> bool {
        self.user_count == other.user_count
    }
}

pub type PlayerId = usize;

pub struct GameServer {
    default_game_opts: GameOpts,
    queue: VecDeque<usize>,
    users: HashMap<usize, ServerUser>,
    private_rooms: HashMap<String, GameRoom>,
    rooms: BinaryHeap<ServerRoom>,
}

impl Actor for GameServer {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        ctx.run_interval(Duration::from_secs(2), |this, ctx| {
            if this.queue.is_empty() {
                return;
            }

            if this.rooms.is_empty() || this.queue.len() > 3 {
                // no game room or if enough users in the queue, start a new game
                this.spawn_room(ctx);
            }

            // for each user in the in queue, and put them in game
            while let Some(user_id) = this.queue.pop_front() {
                let mut room = this.rooms.pop().unwrap();
                let user = this.users.get_mut(&user_id).unwrap();

                // for now send user to first game room found
                room.addr
                    .do_send(GameSessionEvent::Connect(user_id, user.clone()));
                room.user_count += 1;

                // put room back into heap
                this.rooms.push(room)
            }
        });
    }

    fn stopped(&mut self, _: &mut Self::Context) {
        info!("Stopping termibbl server");

        self.rooms
            .iter()
            .for_each(|room| room.addr.do_send(StopSignal))
    }
}

/// Helper functions for `GameServer`
impl GameServer {
    pub fn new(default_game_opts: GameOpts) -> Self {
        Self {
            default_game_opts,
            queue: VecDeque::new(),
            users: HashMap::new(),
            private_rooms: HashMap::new(),
            rooms: BinaryHeap::new(),
        }
    }

    fn spawn_room(&mut self, ctx: &mut Context<Self>) {
        // create a new game room actor with default opts,
        debug!("Spawning a new game room session from default opts.");

        let addr = GameRoom::new(ctx.address(), self.default_game_opts.clone()).start();
        self.rooms.push(ServerRoom {
            addr,
            user_count: 0,
        });
    }
}

/// Handle stop signal for game server.
impl Handler<StopSignal> for GameServer {
    type Result = ();

    fn handle(&mut self, _: StopSignal, ctx: &mut Self::Context) -> Self::Result {
        ctx.stop();
    }
}

#[derive(Message)]
#[rtype(result = "usize")]
pub enum ServerRequest {
    Connect(ServerUser),
    Disconnect(usize),
    FindRoom { id: usize, peer_addr: SocketAddr },
    SendMessage(),
}

impl ToString for ServerRequest {
    fn to_string(&self) -> String {
        match self {
            Self::Connect(_) => "Connect".to_owned(),
            Self::Disconnect(_) => "Disconnect".to_owned(),
            Self::FindRoom { id, peer_addr: _ } => format!("FindRoom {{ id: {} }}", id,),
            Self::SendMessage() => "SendMessage".to_owned(),
        }
    }
}

impl Handler<ServerRequest> for GameServer {
    type Result = PlayerId;

    fn handle(&mut self, event: ServerRequest, _: &mut Self::Context) -> Self::Result {
        debug!("Handling server request <> {}", event.to_string());

        match event {
            ServerRequest::FindRoom { id, peer_addr } => {
                // handle join room request, called when user wants random room
                debug!("({}) requested to join a game room", peer_addr);

                if self.users.contains_key(&id) {
                    // send user to game queue
                    self.queue.push_back(id);
                }

                id
            }

            ServerRequest::Connect(info) => {
                let id = rand::thread_rng().gen::<usize>();
                self.users.insert(id, info);

                id
            }

            ServerRequest::Disconnect(user_id) => {
                if self.queue.contains(&user_id) {
                    self.queue.remove(user_id);
                }

                self.users.remove(&user_id);

                user_id
            }
            _ => todo!(),
        }
    }
}

/// Help function to generate random username
fn random_username() -> Username {
    // for now make it constant
    "ruby".to_owned().into()
}

/// Store the options for the game room
#[derive(Debug, Clone)]
pub struct GameOpts {
    pub(crate) dimensions: (usize, usize),
    pub(crate) words: Either<Vec<String>, Cycle<IntoIter<String>>>,
    pub(crate) number_of_rounds: usize,
    pub(crate) round_duration: u64,
}

/// Various stages of the game. (Idle -> InLobby -> Skribbl)
pub enum GameState {
    ///An idle game state is a game waiting to start.
    Idle,

    /// Has connected users in lobby, game is waiting for to start.
    /// Here users can add custom words or use default_words.
    InLobby {
        leader: ServerUser,
        game_opts: GameOpts,
    },

    /// Main game state
    InGame(Skribbl),

    /// Do nothing, allow all users to draw. (deprecated)
    FreeDraw,
}

impl GameState {
    fn game(&mut self) -> Option<&mut Skribbl> {
        if let GameState::InGame(ref mut skribbl) = self {
            Some(skribbl)
        } else {
            None
        }
    }

    fn start_game(&self, users: Vec<(usize, Username)>, default_game_opts: GameOpts) -> Self {
        let game_opts = match self {
            GameState::InLobby {
                game_opts,
                leader: _,
            } => game_opts.clone(),
            _ => default_game_opts,
        };

        Self::InGame(Skribbl::new(users, game_opts))
    }
}

pub struct GameRoom {
    sessions: HashMap<usize, ServerUser>,
    server: Box<Addr<GameServer>>,
    default_game_opts: GameOpts,
    pub state: RefCell<GameState>,
}

/// Make actor to run game room loop, reacting to any server events
/// but does not actually start the termibbl game
impl Actor for GameRoom {
    type Context = Context<Self>;

    fn stopped(&mut self, _: &mut Self::Context) {
        // let reason = "recieved stop signal";
    }
}

impl Handler<StopSignal> for GameRoom {
    type Result = ();
    fn handle(&mut self, _: StopSignal, ctx: &mut Self::Context) -> Self::Result {
        ctx.stop();
    }
}

impl GameRoom {
    fn new(server: Addr<GameServer>, default_game_opts: GameOpts) -> Self {
        Self {
            server: Box::new(server),
            sessions: HashMap::new(),
            default_game_opts,
            state: RefCell::new(GameState::Idle),
        }
    }

    fn on_guess(&mut self, sender_id: PlayerId, msg: data::Message, ctx: &mut Context<Self>) {
        let message = msg.text();
        let username = &self.sessions.get(&sender_id).unwrap().username;
        let message_broadcast =
            ToClientMsg::NewMessage(data::Message::UserMsg(username.clone(), message.to_owned()));

        if let Some(game) = self.state.borrow_mut().game() {
            if let Some(levenshtein_distance) = game.do_guess(&sender_id, message) {
                match levenshtein_distance {
                    0 => {
                        // on correct guess
                        // self.broadcast(ToClientMsg::SkribblStateChanged(state));

                        self.broadcast_system_msg(format!("{} guessed it!", username));

                        if game.has_turn_ended() {
                            ctx.address().do_send(SkribblEvent::TurnOver);
                        }
                    }
                    1 => self.send_message(&sender_id, "You're very close!".to_string()),
                    _ => self.broadcast(message_broadcast),
                };
            } else {
                // player cannot guess, send message to all users who can't
                for id in game.get_non_guessing_players() {
                    let user = self.sessions.get_mut(&id).unwrap();
                    user.addr.do_send(message_broadcast.clone());
                }
            }
        } else {
            self.broadcast(message_broadcast);
        }
    }

    /// send a Message::SystemMsg to all active sessions
    fn broadcast_system_msg(&self, msg: String) {
        self.broadcast(ToClientMsg::NewMessage(data::Message::SystemMsg(msg)));
    }

    /// broadcast a ToClientMsg to all running sessions
    fn broadcast(&self, msg: ToClientMsg) {
        self.sessions
            .iter()
            .for_each(|(_, pl)| pl.addr.do_send(msg.clone()))
    }

    /// broadcast a ToClientMsg to all running sessions
    fn broadcast_except(&self, msg: ToClientMsg, id: &usize) {
        self.sessions
            .iter()
            .filter(|(idx, _)| **idx != *id)
            .for_each(|(_, pl)| pl.addr.do_send(msg.clone()))
    }

    fn send_message(&self, recipient_id: &usize, message: String) {
        self.send(
            recipient_id,
            ToClientMsg::NewMessage(data::Message::SystemMsg(message)),
        )
    }

    fn send(&self, recipient_id: &usize, message: ToClientMsg) {
        if let Some(user) = self.sessions.get(recipient_id) {
            user.addr.do_send(message)
        }
    }
}

#[derive(Message)]
#[rtype(result = "()")]
pub enum GameSessionEvent {
    /// message from `UserSession` to `GameRoom`
    SessionMessage(PlayerId, ToServerMsg),

    /// recieved when a user joins game room from the main server actor,
    /// that user must not already be in a game room.
    Connect(PlayerId, ServerUser),

    Disconnect(PlayerId),
}

/// Handle Game room events
impl Handler<GameSessionEvent> for GameRoom {
    type Result = ();

    fn handle(&mut self, event: GameSessionEvent, ctx: &mut Self::Context) -> Self::Result {
        match event {
            GameSessionEvent::Connect(user_id, user) => {
                let username = user.username.clone();
                let addr = user.addr.clone();

                // add user to list of users in this game.
                self.sessions.insert(user_id, user);

                {
                    // announce user join
                    let msg = format!("{} has joined game room.", username);
                    info!("{}", msg);

                    self.broadcast_system_msg(msg);
                }

                let initial_state: Option<InitialState> = match *self.state.borrow_mut() {
                    GameState::Idle => {
                        // Idle games start when enough users join.
                        if self.sessions.len() >= 2 {
                            // start game.
                            ctx.address().do_send(SkribblEvent::GameStart);
                        } else {
                            self.broadcast_system_msg(
                                "waiting for more users to join the game..".to_owned(),
                            )
                        }
                        None
                    }

                    GameState::InGame(ref mut skribbl) => {
                        skribbl.add_player(user_id, username);

                        Some(InitialState {
                            dimensions: skribbl.game_opts.dimensions,
                            number_of_rounds: skribbl.game_opts.number_of_rounds,
                            skribbl_state: Some(skribbl.state.clone()),
                            player_id: user_id,
                        })
                    }
                    _ => None,
                };

                // tell user game room has been found.
                addr.do_send(JoinRoom(ctx.address(), initial_state));
            }
            GameSessionEvent::SessionMessage(sender_id, msg) => {
                match msg {
                    ToServerMsg::CommandMsg(msg) => {}
                    ToServerMsg::NewMessage(message) => self.on_guess(sender_id, message, ctx),
                    ToServerMsg::NewLine(line) => {
                        if let GameState::InGame(ref mut skribbl) = *self.state.borrow_mut() {
                            if skribbl.state.is_drawing(&sender_id) {
                                skribbl.state.canvas.push(line);
                                self.broadcast_except(ToClientMsg::NewLine(line), &sender_id);
                            } else {
                                // TODO: send this message after several attempts?
                                self.send_message(
                                    &sender_id,
                                    "It is not your turn to draw!".to_owned(),
                                );
                            }
                        }
                    }
                    ToServerMsg::ClearCanvas => {
                        if let Some(ref mut game) = self.state.borrow_mut().game() {
                            game.clear_canvas();
                            self.broadcast(ToClientMsg::ClearCanvas);
                        }
                    }
                    _ => unimplemented!(),
                };
            }
            _ => unimplemented!(),
        }
    }
}

#[derive(Message)]
#[rtype(result = "()")]
enum SkribblEvent {
    GameStart,
    TurnStart,
    Guess(PlayerId, String),
    TurnOver,
    GameEnd,
}

impl Handler<SkribblEvent> for GameRoom {
    type Result = ();

    fn handle(&mut self, event: SkribblEvent, ctx: &mut Self::Context) -> Self::Result {
        match (event, self.state.borrow_mut().game()) {
            (SkribblEvent::GameStart, _) => {
                let sessions_by_username: Vec<(PlayerId, Username)> = self
                    .sessions
                    .iter()
                    .map(|(id, user)| (*id, user.username.clone()))
                    .collect();

                self.state.replace_with(|old| {
                    old.start_game(sessions_by_username, self.default_game_opts.clone())
                });

                ctx.address().do_send(SkribblEvent::TurnStart);
            }
            (SkribblEvent::TurnStart, Some(ref mut game)) => {
                game.next_turn();

                game.turn_timer.replace(ctx.run_later(
                    Duration::from_secs(game.game_opts.round_duration),
                    |_, ctx| ctx.address().do_send(SkribblEvent::TurnOver), // end the turn when the time runs out.
                ));

                let game_state = game.state.clone();
                let drawing_user = &game.state.drawing_user;

                self.send(
                    drawing_user,
                    ToClientMsg::SkribblRoundStart(
                        Some(game.current_word().to_owned()),
                        game_state.clone(),
                    ),
                );

                self.broadcast_except(
                    ToClientMsg::SkribblRoundStart(None, game_state),
                    drawing_user,
                );
            }
            (SkribblEvent::TurnOver, Some(ref mut game)) => {
                self.broadcast_system_msg(format!("The word was: \"{}\"", game.current_word()));

                // stop all future timers.
                game.turn_timer.map(|handle| ctx.cancel_future(handle));

                // let revealed_char_cnt = game.revealed_characters().len();
                // } else if remaining_time <= (ROUND_DURATION / 4) as u32 && revealed_char_cnt < 2
                //     || remaining_time <= (ROUND_DURATION / 2) as u32 && revealed_char_cnt < 1
                // {
                //     skribbl.reveal_random_char();
                //     self.broadcast(ToClientMsg::SkribblStateChanged(skribbl.state));
                // }

                // self.broadcast(ToClientMsg::TimeChanged(remaining_time as u32));

                self.broadcast(ToClientMsg::SkribblRoundEnd(game.state.clone()));

                ctx.address().do_send(if game.is_finished() {
                    SkribblEvent::GameEnd
                } else {
                    SkribblEvent::TurnStart
                });
            }
            _ => {}
        }
    }
}

//     pub async fn on_tick(&mut self) -> Result<()> {
