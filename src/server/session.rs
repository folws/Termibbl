// use super::{GameRoom, GameServer, GameSessionEvent, ServerRequest};

use crate::{
    data, message,
    message::{ClientMsg, ServerMsg},
};
use data::Username;
use log::*;
use UserState::{InGame, InQueue};

use actix::prelude::*;
use message::GameMessage;
use std::net::SocketAddr;
use tokio::{io::WriteHalf, net::TcpStream};

use super::game_server::{GameRoom, GameServer, ServerEvent, ServerResponse};

pub type SocketMessage = std::result::Result<ClientMsg, std::io::Error>;
pub type ClientMessageWriter =
    actix::io::FramedWrite<ServerMsg, WriteHalf<TcpStream>, GameMessage<ServerMsg>>;

#[derive(Clone)]
pub struct User {
    pub username: Username,
    pub peer_addr: SocketAddr,
    pub session: Addr<ClientSession>,
}

impl User {
    pub fn new(session: Addr<ClientSession>, username: Username, peer_addr: SocketAddr) -> Self {
        Self {
            session,
            username,
            peer_addr,
        }
    }
}

const NO_USERNAME: String = "".to_owned();

#[derive(Clone)]
enum UserState {
    Idle,
    InQueue {
        username: Username,
    },
    InGame {
        username: Username,
        room: Addr<GameRoom>,
        // last_msg_instant: std::time::Instant,
    },
}

/// `UserSession` actor is responsible for TCP peer communications.
pub struct ClientSession {
    /// unique session id
    id: String,
    peer_addr: SocketAddr,
    server_ref: Addr<GameServer>,
    to_client_socket: ClientMessageWriter,
    state: UserState,
}

/// Helper functions for `UserSession`
impl ClientSession {
    pub fn new(
        server_ref: Addr<GameServer>,
        to_client_socket: ClientMessageWriter,
        peer_addr: SocketAddr,
    ) -> Self {
        Self {
            id: "".to_owned(),
            state: UserState::Idle,
            server_ref,
            peer_addr,
            to_client_socket,
        }
    }

    fn username(&self) -> Option<&Username> {
        if let InQueue { ref username }
        | InGame {
            ref username,
            room: _,
        } = &self.state
        {
            Some(username)
        } else {
            None
        }
    }

    /// Close this session's sink and stopping the actor
    fn close(&mut self, ctx: &mut Context<Self>) {
        // self.server
        //     .send(ServerRequest::Disconnect(self.id))
        //     .into_actor(self)
        //     .then(|_, this, ctx| {
        //         ctx.stop();
        //         async {}.into_actor(this)
        //     })
        //     .wait(ctx);
    }

    fn is_ingame(&self) -> bool { matches!(self.state, InGame{ username: _, room: _}) }

    fn find_room(&self, username: Username) {
        if let Idle = self.state {
            self.server_ref
                .do_send(ServerEvent::ClientQueue(username.clone()));

            // update state to show in queue
            self.state = UserState::InQueue { username };
        }
    }
}

impl Actor for ClientSession {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        let peer_addr = self.peer_addr;
        debug!("started actor for client {}", peer_addr);

        // inform the server of this client and
        // request a unique identifier from the server to make requests with
        self.server_ref
            .send(ServerEvent::ClientJoin(peer_addr, ctx.address()))
            .into_actor(self)
            .then(move |res, act: &mut Self, _| {
                if let Ok(Some(ServerResponse::AssignId(id))) = res {
                    act.id = id;

                    // TODO: let user choose to either join, search for or create a private room, or just wait if they please
                    // for now send server request to join publc game room search session to the single default game room
                    act.find_room("fl-w".to_string().into());
                }
                async {}.into_actor(act)
            })
            .wait(ctx);
    }

    fn stopped(&mut self, ctx: &mut Self::Context) {
        debug!("stopping actor for {}", self.peer_addr);

        // close write stream
        self.to_client_socket.close();
    }
}

impl actix::io::WriteHandler<bincode::Error> for ClientSession {}

/// Handle messages from the tcp stream of the client (Client -> Server)
impl StreamHandler<Result<ClientMsg, bincode::Error>> for ClientSession {
    fn handle(&mut self, msg: Result<ClientMsg, bincode::Error>, ctx: &mut Self::Context) {
        let msg = if let Ok(msg) = msg {
            msg
        } else {
            return;
        };

        debug!("({}): processing message <> {:?}", self.peer_addr, msg);

        // if let UserState::InGame { room } = &self.state {
        //     if let ToServerMsg::CommandMsg(_) = msg {
        //         // TODO: parse command messages
        //         info!("({}) recieved command msg.", self.peer_addr);
        //     }

        //     // room.do_send(GameSessionEvent::SessionMessage(self.id, msg));
        // }
    }
}

// /// TODO something
// impl Handler<ToClientMsg> for UserSession {
//     type Result = ResponseActFuture<Self, Result<(), ()>>;

//     fn handle(&mut self, msg: ToClientMsg, ctx: &mut Self::Context) -> Self::Result {
//         // handle message by forwarding it to the websocket stream.

//         if let ToClientMsg::Kick(reason) = msg {
//             // kick user from server
//             Box::new(
//                 ctx.address()
//                     .send(ToClientMsg::NewMessage(data::Message::SystemMsg(reason)))
//                     .into_actor(self)
//                     .map(|_, _, _| Ok(())),
//             )
//         } else {
//             let msg = serde_json::to_string(&msg).unwrap();
//             let msg = tungstenite::Message::Text(msg);

//             trace!("({}) sending message to client: {}", self.peer_addr, msg);

//             Box::new(
//                 async {}
//                     .into_actor(self)
//                     .then(move |_, this: &mut Self, _| {
//                         let mut sink = this.sink.take().unwrap();
//                         async move { (sink.send(msg).await.map_err(|_| ()), sink) }.into_actor(this)
//                     })
//                     .then(|res, this, _| {
//                         this.sink = Some(res.1);
//                         async { Ok(()) }.into_actor(this)
//                     }),
//             )
//         }
//     }
// }

// #[derive(Message)]
// #[rtype(result = "()")]
// pub struct JoinRoom(pub Addr<GameRoom>, pub Option<message::InitialState>);

// impl Handler<JoinRoom> for UserSession {
//     type Result = ();

//     fn handle(&mut self, msg: JoinRoom, ctx: &mut Self::Context) -> Self::Result {
//         self.state = UserState::InGame { room: msg.0 };
//         if let Some(state) = msg.1 {
//             ctx.address().do_send(ToClientMsg::InitialState(state));
//         }
//     }
// }
