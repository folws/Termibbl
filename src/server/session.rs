use super::{GameRoom, GameServer, GameSessionEvent, ServerRequest};

use crate::{
    data, message,
    message::{ToClientMsg, ToServerMsg},
};
use data::Username;
use log::{debug, info, trace};

use actix::prelude::*;
use futures_util::SinkExt;
use std::net::SocketAddr;
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::Message;

type WebSocketStream =
    futures_util::stream::SplitSink<tokio_tungstenite::WebSocketStream<TcpStream>, Message>;

#[derive(Clone)]
pub struct ServerUser {
    pub username: Username,
    pub addr: Addr<UserSession>,
    pub peer_addr: SocketAddr,
}

impl ServerUser {
    pub fn new(addr: Addr<UserSession>, username: Username, peer_addr: SocketAddr) -> Self {
        Self {
            addr,
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

pub struct UserSession {
    id: usize,
    peer_addr: SocketAddr,
    server: Addr<GameServer>,
    username: Username,
    sink: Option<WebSocketStream>,
    state: UserState,
}

impl Actor for UserSession {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        let peer_addr = self.peer_addr;
        debug!("started actor for client {}", peer_addr);

        // request a unique identifier from the server to make requests with
        self.server
            .send(ServerRequest::Connect(ServerUser::new(
                ctx.address(),
                self.username.clone(),
                peer_addr,
            )))
            .into_actor(self)
            .then(move |res, act: &mut Self, _| {
                if let Ok(id) = res {
                    act.id = id;

                    // TODO: let user choose to either join, search for or create a private gameroom, or just wait if they please
                    // for now send server request to join publc game room search session to the single default game room
                    act.server.do_send(ServerRequest::FindRoom {
                        peer_addr: act.peer_addr,
                        id: act.id,
                    });

                    // update state to show in queue
                    act.state = UserState::InQueue;
                }
                async {}.into_actor(act)
            })
            .wait(ctx);
    }

    fn stopped(&mut self, ctx: &mut Self::Context) {
        debug!("stopping actor for {}", self.peer_addr);

        // close websocket stream
        let mut sink = self.sink.take().unwrap();

        async move { sink.close().await }
            .into_actor(self)
            .then(move |_, _: &mut Self, _| actix::fut::ready(()))
            .wait(ctx);
    }
}

#[derive(actix::Message)]
#[rtype(result = "()")]
pub struct WebSocketMessage(pub Result<Message, tungstenite::Error>);

/// Handle messages from the websocket stream of the client (Client -> Server)
impl StreamHandler<WebSocketMessage> for UserSession {
    fn handle(&mut self, ws_msg: WebSocketMessage, ctx: &mut Self::Context) {
        let msg = ws_msg.0;

        let msg = msg.map(|msg| {
            debug!(
                "({}): processing message <> {}",
                self.peer_addr,
                msg.to_text().unwrap().trim()
            );
            msg
        });

        match msg {
            Ok(Message::Close(_)) | Err(_) => self.close(ctx),
            Ok(Message::Text(msg)) => {
                // deserialize message
                match serde_json::from_str(&msg) {
                    Ok(Some(msg)) => {
                        if let UserState::InGame { room } = &self.state {
                            if let ToServerMsg::CommandMsg(_) = msg {
                                // TODO: parse command messages
                                info!("({}) recieved command msg.", self.peer_addr);
                            }
                            room.do_send(GameSessionEvent::SessionMessage(self.id, msg));
                        }
                    }
                    Ok(None) => {
                        // could not deserialize message,
                    }
                    Err(err) => eprintln!("{} (msg was: {})", err, msg),
                }
            }

            _ => {}
        }
    }
}

/// TODO something
impl Handler<ToClientMsg> for UserSession {
    type Result = ResponseActFuture<Self, Result<(), ()>>;

    fn handle(&mut self, msg: ToClientMsg, ctx: &mut Self::Context) -> Self::Result {
        // handle message by forwarding it to the websocket stream.

        if let ToClientMsg::Kick(reason) = msg {
            // kick user from server
            Box::new(
                ctx.address()
                    .send(ToClientMsg::NewMessage(data::Message::SystemMsg(reason)))
                    .into_actor(self)
                    .map(|_, _, _| Ok(())),
            )
        } else {
            let msg = serde_json::to_string(&msg).unwrap();
            let msg = tungstenite::Message::Text(msg);

            trace!("({}) sending message to client: {}", self.peer_addr, msg);

            Box::new(
                async {}
                    .into_actor(self)
                    .then(move |_, this: &mut Self, _| {
                        let mut sink = this.sink.take().unwrap();
                        async move { (sink.send(msg).await.map_err(|_| ()), sink) }.into_actor(this)
                    })
                    .then(|res, this, _| {
                        this.sink = Some(res.1);
                        async { Ok(()) }.into_actor(this)
                    }),
            )
        }
    }
}

#[derive(Message)]
#[rtype(result = "()")]
pub struct JoinRoom(pub Addr<GameRoom>, pub Option<message::InitialState>);

impl Handler<JoinRoom> for UserSession {
    type Result = ();

    fn handle(&mut self, msg: JoinRoom, ctx: &mut Self::Context) -> Self::Result {
        self.state = UserState::InGame { room: msg.0 };
        if let Some(state) = msg.1 {
            ctx.address().do_send(ToClientMsg::InitialState(state));
        }
    }
}

/// Helper functions for `UserSession`
impl UserSession {
    pub fn new(
        server: Addr<GameServer>,
        username: Username,
        sink: WebSocketStream,
        peer_addr: SocketAddr,
    ) -> Self {
        Self {
            id: 0,
            server,
            username,
            peer_addr,
            sink: Some(sink),
            state: UserState::Idle,
        }
    }

    /// Close this session's sink and stopping the actor
    fn close(&mut self, ctx: &mut Context<Self>) {
        self.server
            .send(ServerRequest::Disconnect(self.id))
            .into_actor(self)
            .then(|_, this, ctx| {
                ctx.stop();
                async {}.into_actor(this)
            })
            .wait(ctx);
    }
}
