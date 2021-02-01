use crate::{
    data,
    network::{ClientMsg, NetworkMessage, ServerMsg},
    StopSignal,
};
use data::Username;
use log::*;
use UserState::{InGame, InQueue};

use actix::prelude::*;
use std::net::SocketAddr;
use tokio::{io::WriteHalf, net::TcpStream};

use super::game::{GameServer, ServerEvent, ServerResponse};

pub type ClientMessageWriter =
    actix::io::FramedWrite<ServerMsg, WriteHalf<TcpStream>, NetworkMessage<ServerMsg>>;

#[derive(Clone)]
pub struct User {
    pub username: Username,
    pub peer_addr: SocketAddr,
    pub session: Addr<UserSession>,
}

impl User {
    pub fn new(session: Addr<UserSession>, username: Username, peer_addr: SocketAddr) -> Self {
        Self {
            session,
            username,
            peer_addr,
        }
    }
}

#[derive(Clone)]
enum UserState {
    Idle,
    InQueue {
        username: Username,
    },
    InGame {
        username: Username,
        // room: Addr<GameRoom>,
        // last_msg_instant: std::time::Instant,
    },
}

/// `UserSession` actor is responsible for TCP peer communications.
pub struct UserSession {
    /// unique session id
    username: Username,
    peer_addr: SocketAddr,
    server_ref: Addr<GameServer>,
    to_client_socket: ClientMessageWriter,
    state: UserState,
}

/// Helper functions for `UserSession`
impl UserSession {
    pub fn new(
        username: Username,
        server_ref: Addr<GameServer>,
        to_client_socket: ClientMessageWriter,
        peer_addr: SocketAddr,
    ) -> Self {
        Self {
            state: UserState::Idle,
            server_ref,
            username,
            peer_addr,
            to_client_socket,
        }
    }

    fn is_ingame(&self) -> bool {
        matches!(
            self.state,
            InGame {
                username: _,
                // room: _
            }
        )
    }

    /// queue this user
    fn join_game_queue(&mut self) {
        if let UserState::Idle = &self.state {
            let username = self.username.clone();
            self.server_ref
                .do_send(ServerEvent::UserQueue(username.clone()));

            // update state to show in queue
            self.state = UserState::InQueue { username };
        }
    }
}

impl Actor for UserSession {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        let peer_addr = self.peer_addr;
        let username = self.username.clone();
        debug!("started actor for client {}", peer_addr);

        // inform the server of this client and
        // request a unique identifier from the server to make requests with
        self.server_ref
            .send(ServerEvent::UserJoined(username, peer_addr, ctx.address()))
            .into_actor(self)
            .then(move |res, act: &mut Self, _| {
                if let Ok(Some(ServerResponse::AssignId(id))) = res {
                    act.username.set_identifier(id);

                    // TODO: let user choose to either join, search for or create a private room, or just wait if they please
                    // for now send server request to join publc game room search session to the single default game room
                    act.join_game_queue();
                }
                async {}.into_actor(act)
            })
            .wait(ctx);
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        debug!("stopping actor for {}", self.peer_addr);

        // close write stream
        self.to_client_socket.close();
    }
}

/// Close this session's sink and stopping the actor
impl Handler<StopSignal> for UserSession {
    type Result = ();

    fn handle(&mut self, _msg: StopSignal, ctx: &mut Self::Context) -> Self::Result {
        ctx.stop();
    }
}

impl actix::io::WriteHandler<bincode::Error> for UserSession {}

/// Handle messages from the tcp stream of the client (Client -> Server)
impl StreamHandler<Result<ClientMsg, bincode::Error>> for UserSession {
    fn handle(&mut self, msg: Result<ClientMsg, bincode::Error>, ctx: &mut Self::Context) {
        let msg = if let Ok(msg) = msg {
            msg
        } else {
            return;
        };

        debug!("({}): processing message <> {:?}", self.peer_addr, msg);
    }
}
