mod session;
mod skribbl;

use std::{
    collections::{BinaryHeap, HashMap},
    io::Read,
};

use actix::{io::FramedWrite, prelude::*};
use argh::FromArgs;
use log::*;
use tokio::{
    net::{TcpListener, TcpStream},
    stream::StreamExt,
};
use tokio_util::codec::FramedRead;

use crate::message::{GameMessage, ToClientMsg, ToServerMsg};

use self::session::UserSession;

pub type ServerResult<T> = std::result::Result<T, ServerError>;

struct ServerError {}

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
    let game_server = GameServer::new(default_game_opts).start();

    // listen and handle incoming connections in async thread.
    let tcp_srv = TcpServer::create(move |ctx| {
        ctx.add_message_stream(Box::leak(server_listener).incoming().map(|stream| {
            let st = stream.unwrap();
            let addr = st.peer_addr().unwrap();

            TcpConnect(st, addr)
        }));

        TcpServer { game_server }
    });

    tokio::signal::ctrl_c().await.unwrap();
    println!("Ctrl-C received. Stopping..");

    // gracefully exit
    tcp_srv.do_send(StopSignal);
    Ok(())
}

struct TcpServer {
    game_server: Addr<GameServer>,
}

impl Actor for TcpServer {
    type Context = Context<Self>;
}

#[derive(Message)]
#[rtype(result = "()")]
struct TcpConnect(pub TcpStream, pub std::net::SocketAddr);

/// Handle stream of TcpStream's
impl Handler<TcpConnect> for TcpServer {
    type Result = ();

    fn handle(&mut self, msg: TcpConnect, _: &mut Context<Self>) {
        info!("new client connection: {}", msg.1);

        let server = self.game_server.clone();
        UserSession::create(move |ctx| {
            let (r, w) = tokio::io::split(msg.0);
            UserSession::add_stream(FramedRead::new(r, GameMessage::<ToServerMsg>::new()), ctx);

            UserSession::new(
                server,
                FramedWrite::new(w, GameMessage::<ToClientMsg>::new(), ctx),
                msg.1,
            )
        });
    }
}

#[derive(Message)]
#[rtype(result = "()")]
struct StopSignal;

impl Handler<StopSignal> for TcpServer {
    type Result = ();

    fn handle(&mut self, msg: StopSignal, ctx: &mut Self::Context) -> Self::Result {
        // pass stop signal to the game server
        self.game_server.do_send(msg)
    }
}

pub struct GameOpts {
    pub(crate) dimensions: (usize, usize),
    pub(crate) words: Box<(dyn Iterator<Item = String>)>,
    pub(crate) number_of_rounds: usize,
    pub(crate) round_duration: usize,
}

pub struct GameServer {
    default_game_opts: GameOpts,
    connected_players: HashMap<usize, Addr<UserSession>>,
    queue: Vec<usize>,
    // rooms: BinaryHeap<ServerRoom>,
    // private_rooms: HashMap<String, GameRoom>,
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

impl Handler<StopSignal> for GameServer {
    type Result = ();

    fn handle(&mut self, msg: StopSignal, ctx: &mut Self::Context) -> Self::Result { ctx.stop(); }
}

/// Helper functions for `GameServer`
impl GameServer {
    pub fn new(default_game_opts: GameOpts) -> Self {
        Self {
            default_game_opts,
            queue: Vec::new(),
            connected_players: HashMap::new(),
            // private_rooms: HashMap::new(),
            // rooms: BinaryHeap::new(),
        }
    }

    fn spawn_room(&mut self, ctx: &mut Context<Self>) {
        // create a new game room actor with default opts,
        debug!("Spawning a new game room session from default opts.");

        // let addr = GameRoom::new(ctx.address(), self.default_game_opts.clone()).start();
        // self.rooms.push(ServerRoom {
        //     addr,
        //     user_count: 0,
        // });
    }
}

pub struct GameRoom;

impl Actor for GameRoom {
    type Context = Context<Self>;
}
