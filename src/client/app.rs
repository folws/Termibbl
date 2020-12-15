use crate::{
    client::error::Result,
    client::ui,
    data::{self, CanvasColor, Coord, Line, Message},
    message::{InitialState, ToClientMsg, ToServerMsg},
    server::skribbl::SkribblState,
    ClientEvent,
};
use actix::Actor;
use actix::Addr;
use actix::Context;
use actix::Handler;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent};
use futures_util::sink::SinkExt;
use futures_util::stream::StreamExt;

use data::{CommandMsg, Username};
use tokio::task::JoinHandle;
use tokio_tungstenite::WebSocketStream;
use tui::{backend::Backend, Terminal};

use super::InputEvent;
use super::{conn::ServerConnection, DrawEvent};

const PALETTE: [CanvasColor; 16] = [
    CanvasColor::White,
    CanvasColor::Gray,
    CanvasColor::DarkGray,
    CanvasColor::Black,
    CanvasColor::Red,
    CanvasColor::LightRed,
    CanvasColor::Green,
    CanvasColor::LightGreen,
    CanvasColor::Blue,
    CanvasColor::LightBlue,
    CanvasColor::Yellow,
    CanvasColor::LightYellow,
    CanvasColor::Cyan,
    CanvasColor::LightCyan,
    CanvasColor::Magenta,
    CanvasColor::LightMagenta,
];

#[derive(Debug, Clone)]
pub struct AppCanvas {
    pub palette: Vec<CanvasColor>,
    pub lines: Vec<data::Line>,
    pub dimensions: (usize, usize),
}

impl AppCanvas {
    fn new(dimensions: (usize, usize), lines: Vec<data::Line>) -> Self {
        AppCanvas {
            lines,
            dimensions,
            palette: PALETTE.to_vec(),
        }
    }
}

impl AppCanvas {
    pub fn draw_line(&mut self, line: Line) {
        self.lines.push(line);
    }
}

#[derive(Debug, Clone, Default)]
pub struct Chat {
    pub input: String,
    pub messages: Vec<Message>,
}

#[derive(Debug)]
pub enum AppState {
    /// disconnected from server,
    Disconnected {
        last_connected_server: Option<String>,
        last_username: Username,
        number_of_retries: usize,
    },

    /// connected to server, idle
    Idle {
        session: ServerSession,
    },

    Playing(AppGameState),
}

struct AppGameState {
    canvas: AppCanvas,
    chat: Chat,
    session: ServerSession,
    last_mouse_pos: Option<Coord>,
    current_color: CanvasColor,
    game_state: Option<SkribblState>,
    remaining_time: Option<u32>,
}

#[derive(Debug)]
pub struct App {
    pub state: AppState,
    pub connection: Addr<ServerConnection>,
}

impl Actor for App {
    type Context = Context<App>;
}

impl Handler<ToClientMsg> for App {
    type Result = ();

    fn handle(&mut self, msg: ToClientMsg, ctx: &mut Self::Context) -> Self::Result {
        match msg {
            ToClientMsg::TimeChanged(new_time) => {
                self.remaining_time = Some(new_time);
            }
            ToClientMsg::NewMessage(message) => self.chat.messages.push(message),
            ToClientMsg::NewLine(line) => {
                self.canvas.draw_line(line);
            }
            ToClientMsg::SkribblRoundStart(_word_to_draw, skribbl_state) => {
                self.game_state = Some(skribbl_state);
            }
            // ToClientMsg::SkribblStateChanged(new_state) => {
            //     self.game_state = Some(new_state);
            // }
            ToClientMsg::ClearCanvas => {
                self.canvas.lines.clear();
            }
            ToClientMsg::GameOver(state) => {
                dbg!(state);
                panic!("Game over, I couldn't yet be bothered to implement this in a better way yet,...");
            }
            ToClientMsg::InitialState(_) => {}
            _ => unimplemented!(),
        };
    }
}

// impl Handler<ConnectionChange> for App {}

impl Handler<InputEvent> for App {
    type Result = ();

    fn handle(&mut self, event: InputEvent, ctx: &mut Self::Context) -> Self::Result {
        let game_state = if let AppState::Playing(ref mut game_state) = self.state {
            game_state
        } else {
            return;
        };

        match event {
            InputEvent::Mouse(event) => {
                if !self.is_drawing() {
                    return Ok(());
                }

                let dimensions = game_state.canvas.dimensions;
                match event {
                    MouseEvent::Down(_, x, y, _) => {
                        if y == 0 {
                            let swatch_size = dimensions.0 / self.canvas.palette.len() as usize;
                            let selected_color = self.canvas.palette.get(x as usize / swatch_size);
                            match selected_color {
                                Some(color) => self.current_color = color.clone(),
                                _ => {}
                            }
                        } else {
                            self.last_mouse_pos = Some(Coord(x, y));
                        }
                    }
                    MouseEvent::Up(_, _, _, _) => {
                        self.last_mouse_pos = None;
                    }
                    MouseEvent::Drag(_, x, y, _) => {
                        let mouse_pos = Coord(x, y);
                        let line = Line::new(
                            self.last_mouse_pos.unwrap_or(mouse_pos),
                            mouse_pos,
                            self.current_color,
                        );
                        self.canvas.draw_line(line);
                        self.session.send(ToServerMsg::NewLine(line)).await?;
                        self.last_mouse_pos = Some(mouse_pos);
                    }
                    _ => {}
                }
            }
            InputEvent::Key(event) => {
                let KeyEvent { modifiers, code } = event;
                match code {
                    KeyCode::Enter => {
                        if self.chat.input.trim().is_empty() {
                            self.chat.input = String::new();
                            return Ok(());
                        }

                        let msg_content = self.chat.input.clone();
                        if msg_content.starts_with("!") {
                            if msg_content.starts_with("!kick ") {
                                let msg_without_cmd =
                                    msg_content.trim_start_matches("!kick ").trim().to_string();
                                let command =
                                    CommandMsg::KickPlayer(Username::from(msg_without_cmd));
                                self.session.send(ToServerMsg::CommandMsg(command)).await?;
                            };
                        } else {
                            let message = Message::UserMsg(
                                self.session.username.clone(),
                                self.chat.input.clone(),
                            );
                            self.session.send(ToServerMsg::NewMessage(message)).await?;
                        }
                        self.chat.input = String::new();
                    }
                    KeyCode::Backspace => {
                        self.chat.input.pop();
                    }
                    KeyCode::Char('h') if modifiers.contains(KeyModifiers::CONTROL) => {
                        self.chat.input.pop();
                    }
                    KeyCode::Delete => {
                        if self.is_drawing() {
                            self.session.send(ToServerMsg::ClearCanvas).await?;
                            self.canvas.lines.clear();
                        }
                    }
                    KeyCode::Char(c) => {
                        self.chat.input.push(*c);
                    }
                    _ => {}
                }
            }
        }
    }
}

impl Handler<DrawEvent<'_>> for App {
    type Result = ();

    fn handle(&mut self, msg: DrawEvent<'_>, ctx: &mut Self::Context) -> Self::Result {
        msg.0.draw(|&mut f| ui::draw(self, f));
    }
}

impl Handler<ConnectionChange> for App {}

impl App {
    pub fn new(username: Username, connection: Addr<ServerConnection>) -> App {
        App {
            state: AppState::Disconnected {
                last_username: username,
                number_of_retries: 0,
                last_connected_server: None,
            },
            connection,
        }
        // App {
        //     canvas: AppCanvas::new(
        //         initial_state.dimensions,
        //         if let Some(skribbl_state) = &initial_state.skribbl_state {
        //             skribbl_state.canvas.clone()
        //         } else {
        //             Vec::new()
        //         },
        //     ),
        //     chat: Chat::default(),
        //     last_mouse_pos: None,
        //     current_color: CanvasColor::White,
        //     game_state: initial_state.skribbl_state,
        //     session,
        //     remaining_time: None,
        // }
    }

    pub fn is_drawing(&self) -> bool {
        self.game_state
            .as_ref()
            .map(|x| x.is_drawing(&self.session.id))
            .unwrap_or(true)
    }
}
