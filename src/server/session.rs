// use super::{GameRoom, GameServer, GameSessionEvent, ServerRequest};

use crate::{
    data, message,
    message::{ToClientMsg, ToServerMsg},
};
use data::Username;
use log::{debug, info, trace};

use actix::prelude::*;
use message::GameMessage;
use std::net::SocketAddr;
use tokio::{io::WriteHalf, net::TcpStream};

use super::{GameRoom, GameServer};

pub type SocketMessage = std::result::Result<ToServerMsg, std::io::Error>;
pub type ClientMessageWriter =
    actix::io::FramedWrite<ToClientMsg, WriteHalf<TcpStream>, GameMessage<ToClientMsg>>;

#[derive(Clone)]
pub struct ServerUser {
    pub username: Username,
    pub session: Addr<UserSession>,
    pub peer_addr: SocketAddr,
}

impl ServerUser {
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
    InQueue,
    InGame {
        room: Addr<GameRoom>,
        // last_msg_instant: std::time::Instant,
    },
}

/// `UserSession` actor is responsible for TCP peer communications.
pub struct UserSession {
    /// unique session id
    id: usize,
    peer_addr: SocketAddr,
    username: Option<Username>,
    server: Addr<GameServer>,
    writer: ClientMessageWriter,
    state: UserState,
}

impl Actor for UserSession {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        let peer_addr = self.peer_addr;
        debug!("started actor for client {}", peer_addr);

        // request a unique identifier from the server to make requests with
        // self.server
        //     .send(ServerRequest::Connect(ServerUser::new(
        //         ctx.address(),
        //         self.username.clone(),
        //         peer_addr,
        //     )))
        //     .into_actor(self)
        //     .then(move |res, act: &mut Self, _| {
        //         if let Ok(id) = res {
        //             act.id = id;

        //             // TODO: let user choose to either join, search for or create a private gameroom, or just wait if they please
        //             // for now send server request to join publc game room search session to the single default game room
        //             act.server.do_send(ServerRequest::FindRoom {
        //                 peer_addr: act.peer_addr,
        //                 id: act.id,
        //             });

        //             // update state to show in queue
        //             act.state = UserState::InQueue;
        //         }
        //         async {}.into_actor(act)
        //     })
        //     .wait(ctx);
    }

    fn stopped(&mut self, ctx: &mut Self::Context) {
        debug!("stopping actor for {}", self.peer_addr);

        // close websocket stream
        // let mut sink = self.sink.take().unwrap();

        // async move { sink.close().await }
        //     .into_actor(self)
        //     .then(move |_, _: &mut Self, _| actix::fut::ready(()))
        //     .wait(ctx);
    }
}

impl actix::io::WriteHandler<bincode::Error> for UserSession {}

/// Handle messages from the tcp stream of the client (Client -> Server)
impl StreamHandler<Result<ToServerMsg, bincode::Error>> for UserSession {
    fn handle(&mut self, msg: Result<ToServerMsg, bincode::Error>, ctx: &mut Self::Context) {
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

/// Helper functions for `UserSession`
impl UserSession {
    pub fn new(
        server: Addr<GameServer>,
        writer: ClientMessageWriter,
        peer_addr: SocketAddr,
    ) -> Self {
        Self {
            server,
            peer_addr,
            writer,
            id: 0,
            username: None,
            state: UserState::Idle,
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
}
