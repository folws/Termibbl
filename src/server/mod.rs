mod game_server;
mod session;
mod skribbl;

use std::{
    collections::{BinaryHeap, HashMap},
    io::Read,
    net::SocketAddr,
};

use actix::{io::FramedWrite, prelude::*};
use argh::FromArgs;
use log::*;
use tokio::{
    net::{TcpListener, TcpStream},
    stream::StreamExt,
};
use tokio_util::codec::FramedRead;

use crate::{
    client::Username,
    message::{GameMessage, ToClientMsg, ToServerMsg},
};

use self::{
    session::{ClientRef, ClientSession},
    skribbl::SkribblState,
};

pub type ServerResult<T> = std::result::Result<T, ServerError>;

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
#[derive(Clone)]
pub struct GameOpts {
    pub(crate) dimensions: (usize, usize),
    pub(crate) words: Box<(dyn Iterator<Item = String>)>,
    pub(crate) number_of_rounds: usize,
    pub(crate) round_duration: usize,
}

const DIMEN: (usize, usize) = (900, 60);
const ROUND_DURATION: usize = 120;
const ROUNDS: usize = 3;

#[derive(FromArgs)]
/// host a Termibbl session
#[argh(subcommand, name = "server")]
pub struct CliOpts {
    /// port for server to run on
    #[argh(option, short = 'p')]
    port: u32,

    /// whether to show public ip when server starts
    #[argh(switch, short = 'y')]
    display_public_ip: bool,

    #[argh(option, default = "ROUND_DURATION")]
    /// default round duration in seconds
    round_duration: usize,

    #[argh(option, default = "ROUNDS")]
    /// default number of rounds per game
    rounds: usize,

    /// default canvas dimensions <width>x<height>
    #[argh(option, from_str_fn(parse_dimension), default = "DIMEN")]
    dimensions: (usize, usize),

    /// optional path to custom word list
    #[argh(option, from_str_fn(read_words_file))]
    words: Option<Vec<String>>,
}

fn parse_dimension(s: &str) -> std::result::Result<(usize, usize), String> {
    let mut split = s
        .split('x')
        .map(str::parse)
        .filter_map(std::result::Result::ok);

    split
        .next()
        .and_then(|width| split.next().map(|height| (width, height)))
        .ok_or("could not parse dimensions".to_owned())
}

fn read_words_file(path: &str) -> std::result::Result<Vec<String>, String> {
    info!("reading words from file {}", path);

    let mut words = String::new();
    let mut file = std::fs::File::open(path).map_err(|e| e.to_string())?;

    file.read_to_string(&mut words).map_err(|e| e.to_string())?;

    debug!("read {} words from file", words.len());

    Ok(words
        .lines()
        .map(|x| x.trim().to_string())
        .filter(|x| !x.is_empty())
        .collect::<Vec<String>>())
}

/// Main entry point for the server
/// Define tcp server that will accept incoming tcp connection and create
/// game room and user actors.
pub async fn run_with_opts(opt: CliOpts) -> Result<(), ()> {
    let port = opt.port;
    let default_words = opt.words.unwrap_or_else(Vec::new);
    let default_dimensions = opt.dimensions;
    let default_round_duration = opt.round_duration;
    let default_number_of_rounds = opt.rounds;

    let default_game_opts = GameOpts {
        dimensions: default_dimensions,
        words: Box::new(default_words.into_iter().cycle()),
        number_of_rounds: default_number_of_rounds,
        round_duration: default_round_duration,
    };

    // display public ip
    if opt.display_public_ip {
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

    // start tcp listener on given port
    let addr = format!("127.0.0.1:{}", port);
    let server_listener = Box::new(
        TcpListener::bind(&addr)
            .await
            .expect("Could not start webserver (could not bind)"),
    );

    info!("🚀 Running Termibbl server on {}...", addr);

    // start termibbl server actor
    let game_server = GameServer::create(move |ctx| {
        // listen and handle incoming connections in async thread.
        ctx.add_message_stream(Box::leak(server_listener).incoming().map(|stream| {
            let st = stream.unwrap();
            let addr = st.peer_addr().unwrap();

            TcpConnect(st, addr)
        }));

        GameServer::new(default_game_opts)
    });

    tokio::signal::ctrl_c().await.unwrap();
    println!("Ctrl-C received. Stopping..");

    // gracefully exit
    game_server.do_send(StopSignal);
    Ok(())
}

pub struct GameServer {
    default_game_opts: GameOpts,
    connected_players: HashMap<usize, Addr<ClientSession>>,
    player_queue: Vec<usize>,
    /// hold game rooms by their generated key.
    rooms: HashMap<String, GameRoom>,
    // rooms: BinaryHeap<ServerRoom>,
    // private_rooms: HashMap<String, GameRoom>,
}

/// Helper functions for `GameServer`
impl GameServer {
    pub fn new(default_game_opts: GameOpts) -> Self {
        Self {
            default_game_opts,
            player_queue: Vec::new(),
            connected_players: HashMap::new(),
            rooms: HashMap::new(),
            // rooms: BinaryHeap::new(),
        }
    }
}

impl Actor for GameServer {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        // ctx.run_interval(Duration::from_secs(2), |this, ctx| {
        //     if this.queue.is_empty() {
        //         return;
        //     }

        //     if this.rooms.is_empty() || this.queue.len() > 3 {
        //         // no game room or if enough users in the queue, start a new game
        //         this.spawn_room(ctx);
        //     }

        //     // for each user in the in queue, and put them in game
        //     while let Some(user_id) = this.queue.pop_front() {
        //         let mut room = this.rooms.pop().unwrap();
        //         let user = this.connected_players.get_mut(&user_id).unwrap();

        //         // for now send user to first game room found
        //         room.addr
        //             .do_send(GameSessionEvent::Connect(user_id, user.clone()));
        //         room.user_count += 1;

        //         // put room back into heap
        //         this.rooms.push(room)
        //     }
        // });
    }

    fn stopped(&mut self, _: &mut Self::Context) {
        info!("Stopping termibbl server");

        // stop all connected user sessions
        // self.rooms
        //     .iter()
        //     .for_each(|room| room.addr.do_send(StopSignal))
    }
}

#[derive(Message)]
#[rtype(result = "()")]
struct TcpConnect(pub TcpStream, pub std::net::SocketAddr);

/// Handle stream of TcpStream's
impl Handler<TcpConnect> for GameServer {
    type Result = ();

    fn handle(&mut self, msg: TcpConnect, ctx: &mut Context<Self>) {
        info!("new client connection: {}", msg.1);

        let server_ref = ctx.address();
        let peer_addr = msg.1;

        ClientSession::create(move |ctx| {
            let (r, w) = tokio::io::split(msg.0);
            ClientSession::add_stream(FramedRead::new(r, GameMessage::<ToServerMsg>::new()), ctx);

            ClientSession::new(
                server_ref,
                FramedWrite::new(w, GameMessage::<ToClientMsg>::new(), ctx),
                peer_addr,
            )
        });
    }
}

#[derive(Message)]
#[rtype(result = "()")]
struct StopSignal;

impl Handler<StopSignal> for GameServer {
    type Result = ();

    fn handle(&mut self, msg: StopSignal, ctx: &mut Self::Context) -> Self::Result {
        ctx.stop();
    }
}

/// Helper functions for `GameServer`
impl GameServer {
    pub fn new(default_game_opts: GameOpts) -> Self {
        Self {
            default_game_opts,
            player_queue: Vec::new(),
            connected_players: HashMap::new(),
            rooms: HashMap::new(),
            // rooms: BinaryHeap::new(),
        }
    }

    fn generate_room_key(&self) -> String {}

    /// create a new game room actor with default opts,
    fn create_game_room(&mut self, ctx: &mut Context<Self>) -> String {
        debug!("Spawning a new game room session from default opts.");

        let room_key = self.generate_room_key();
        let room = GameRoom::new(self.default_game_opts.clone());

        self.rooms.insert(room_key.clone(), room);

        room_key
    }

    fn add_client_connection(
        &mut self,
        peer_addr: &SocketAddr,
        session: Addr<ClientSession>,
    ) -> usize {
        // let id = rand::thread_rng().gen();
        let id = 0;
        debug!("({}): assigning idmessage <> {}", peer_addr, id);

        self.connected_players.insert(id, session);

        id
    }
}

#[derive(Message)]
#[rtype(result = "usize")]
pub struct NewClientSession(Addr<ClientSession>);

impl Handler<NewClientSession> for GameServer {
    type Result = usize;

    fn handle(&mut self, msg: NewClientSession, ctx: &mut Self::Context) -> Self::Result {
        self.add_client_connection(msg.0, msg.1)
    }
}

pub enum GameState {
    Lobby,
    InGame(Addr<SkribblState>),
}

pub struct GameRoom {
    state: GameState,
    clients: HashMap<usize, ClientRef>,
    game_opts: GameOpts,
}

impl GameRoom {
    fn new(game_opts: GameOpts) -> Self {
        Self {
            state: GameState::Lobby,
            clients: HashMap::new(),
            game_opts,
        }
    }
}
