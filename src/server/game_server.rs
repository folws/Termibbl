use std::{collections::HashMap, net::SocketAddr};

use crate::{
    message::{ClientMsg, GameMessage, ServerMsg},
    Username,
};

use super::{
    session::{self, ClientSession},
    skribbl::SkribblState,
    GameOpts, StopSignal, TcpConnect, ROOM_KEY_LENGTH,
};

use actix::{io::FramedWrite, prelude::*};
use log::{debug, info};
use nanoid::nanoid;
use tokio_util::codec::FramedRead;
use ServerEvent::{ClientJoin, ClientLeave};

#[derive(Message)]
#[rtype(result = "Option<ServerResponse>")]
pub enum ServerEvent {
    /// Notify server of newly connected client.
    ClientJoin(SocketAddr, Addr<ClientSession>),

    /// Notify server of disconnecting client. First argument holds client id.
    ClientLeave(Username),

    /// Add client
    ClientQueue(Username),
}

pub enum ServerResponse {
    AssignId(String),
}

pub struct GameServer {
    default_game_opts: GameOpts,

    /// holds all players connected by id
    connected_players: HashMap<String, Addr<ClientSession>>,

    /// list of players searching for a game
    game_queue: Vec<Username>,

    /// hold game rooms by their generated key.
    rooms: HashMap<String, GameRoom>,
    // rooms: BinaryHeap<ServerRoom>,
}

/// Helper functions for `GameServer`
impl GameServer {
    pub fn new(default_game_opts: GameOpts) -> Self {
        Self {
            default_game_opts,
            game_queue: Vec::new(),
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

/// Handle stream of TcpStream's
impl Handler<TcpConnect> for GameServer {
    type Result = ();

    fn handle(&mut self, msg: TcpConnect, ctx: &mut Context<Self>) {
        debug!("new client connection: {}", msg.1);

        let server_ref = ctx.address();
        let peer_addr = msg.1;

        ClientSession::create(move |ctx| {
            let (r, w) = tokio::io::split(msg.0);
            ClientSession::add_stream(FramedRead::new(r, GameMessage::<ClientMsg>::new()), ctx);

            ClientSession::new(
                server_ref,
                FramedWrite::new(w, GameMessage::<ServerMsg>::new(), ctx),
                peer_addr,
            )
        });
    }
}

impl Handler<StopSignal> for GameServer {
    type Result = ();
    fn handle(&mut self, msg: StopSignal, ctx: &mut Self::Context) -> Self::Result { ctx.stop(); }
}

impl Handler<ServerEvent> for GameServer {
    type Result = Option<ServerResponse>;

    fn handle(&mut self, msg: ServerEvent, ctx: &mut Self::Context) -> Self::Result {
        match msg {
            ClientJoin(peer_addr, addr) => {
                let id = self.add_client(&peer_addr, addr);
                Some(ServerResponse::AssignId(id))
            }

            ClientLeave(username) => {
                self.remove_client(username.identifier().unwrap());
                None
            }

            ServerEvent::ClientQueue(username) => {
                self.game_queue.push(username);
                None
            }
        }
    }
}
/// Helper functions for `GameServer`
impl GameServer {
    fn generate_room_key(&self) -> String { nanoid!(ROOM_KEY_LENGTH, &nanoid::alphabet::SAFE) }

    /// create a new game room actor with default opts,
    fn create_game_room(&mut self, ctx: &mut Context<Self>) -> String {
        debug!("Spawning a new game room session from default opts.");

        let room_key = self.generate_room_key().to_owned();
        let room = GameRoom::new(self.default_game_opts.clone());

        self.rooms.insert(room_key.clone(), room);

        room_key
    }

    fn add_client(&mut self, peer_addr: &SocketAddr, session: Addr<ClientSession>) -> String {
        let id = nanoid!();
        debug!("({}): assigning id <> {}", peer_addr, id);

        self.connected_players.insert(id, session);

        id
    }

    fn remove_client(&mut self, id: String) { self.connected_players.remove(&id); }
}

pub enum GameState {
    Lobby,
    InGame(Addr<SkribblState>),
}

pub struct GameRoom {
    state: GameState,
    clients: HashMap<usize, session::User>,
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

impl Actor for GameRoom {
    type Context = Context<Self>;
}
