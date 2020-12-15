use actix::prelude::*;
use tokio_tungstenite::WebSocketStream;

use super::{App, Username};

#[derive(Debug)]
pub struct ServerConnection {
    app: Addr<App>,
    socket: SplitSink<WebSocketStream<Message, tungstenite::error::Error>,
}

impl Actor for ServerConnection {
    type Context = Context<Self>;
}

#[derive(Message)]
#[rtype(result = "()")]
pub struct ConnectionRequest<'a> {
    server_address: &'a str,
    username: Username,
}

impl Handler<ConnectionRequest<'_>> for ServerConnection {
    type Result = ();

    fn handle(&mut self, msg: ConnectionRequest<'_>, ctx: &mut Self::Context) -> Self::Result {
        let ws: WebSocketStream<_> = tokio_tungstenite::connect_async(msg.server_address)
            .expect("Could not connect to server")
            .0;
        let (mut ws_send, mut ws_recv) = ws.split();
        let socket = ws_send.clone();

        // first send the username to the server
        async move {
            socket.send(tungstenite::Message::Text(msg.username.into()))
                .await
        }
        .into_actor(self)
        .then(move |_, _: &mut Self, _| actix::fut::ready(()))
        .wait(ctx);

    }
}

impl Handler<ToServerMsg> for U

#[derive(Debug, Clone)]
pub struct ServerSession {
    pub username: Username,
    pub id: usize,
}

impl ServerConnection {
    pub fn new(app: Addr<App>) -> ServerConnection {
        ServerConnection { app }
    }

    async fn start_session(
        &mut self,
        server_adress: &str,
        username: Username,
    ) -> Result<ServerSession> {
        // tokio::spawn(async move {
        //     // and wait for the initial state
        //     let initial_state: InitialState = loop {
        //         let msg = ws_recv.next().await;
        //         if let Some(Ok(tungstenite::Message::Text(msg))) = msg {
        //             if let Ok(ToClientMsg::InitialState(state)) = serde_json::from_str(&msg) {
        //                 break state;
        //             }
        //         }
        //     };
        // });

        // forward events to the server
        self.send_thread = Some(tokio::spawn(async move {
            loop {
                let msg = to_server_recv.recv().await;
                let msg = serde_json::to_string(&msg).unwrap();
                if let Err(_) = ws_send.send(tungstenite::Message::Text(msg)).await {
                    break;
                }
            }
        }));

        // and receive messages from the server
        tokio::spawn(async move {
            loop {
                match ws_recv.next().await {
                    Some(Ok(tungstenite::Message::Text(msg))) => {
                        let msg = serde_json::from_str(&msg).unwrap();
                        let _ = tx.send(ClientEvent::ServerMessage(msg)).await;
                    }
                    Some(Ok(tungstenite::Message::Close(_))) => {
                        break;
                    }
                    _ => {}
                }
            }
            std::mem::drop(send_handle);
        });

        Ok(App::new(
            ServerSession {
                to_server_send,
                username,
                id: initial_state.player_id,
            },
            initial_state,
        ))
    }

    pub async fn send(&mut self, message: ToServerMsg) -> Result<()> {
        self.to_server_send.send(message).await?;
        Ok(())
    }
}
