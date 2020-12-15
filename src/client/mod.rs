use std::io::Stdout;
use std::time::Duration;

pub use crate::*;
pub mod app;
mod conn;
mod ui;

use actix::Actor;
use actix::AsyncContext;
use conn::ServerConnection;
use crossterm::event;
use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, MouseEvent},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};

use tokio::sync::mpsc;
use tui::{backend::CrosstermBackend, Terminal};

#[derive(actix::Message)]
#[rtype(result = "()")]
enum InputEvent {
    Mouse(MouseEvent),
    Key(KeyEvent),
}

enum AppEvent {}

#[derive(actix::Message)]
#[rtype(result = "()")]
struct DrawEvent<'a>(&'a mut Terminal<CrosstermBackend<Stdout>>);

// terminal.draw(|mut f| ui::draw(&mut app, &mut f))?;
pub async fn run(addr: &String, username: Username) -> crate::error::Result<()> {
    let mut stdout = stdout();

    enable_raw_mode()?;
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;

    let app = App::new(username);
    let (app_tx, app_rx) = mpsc::channel::<AppEvent>(1);


    tokio::spawn(async move {
        app_rt.await
    });

    app.send()
    let app = App::create(|ctx| {
        let server_connection = ServerConnection::new(ctx.address()).start();
        App::new(username, server_connection)
    });

    let mut terminal = Terminal::new(CrosstermBackend::new(stdout))?;

    // terminal.clear()?;
    loop {
        app.do_send(DrawEvent(&mut terminal)); // tell app to draw to terminal.
        if let Some(event) = event::poll(Duration::from_millis(0)) {
            match event {
                Event::Key(key) => {
                    match key {
                        KeyEvent {
                            code: KeyCode::Esc,
                            modifiers: _,
                        } => break,
                        _ => app.do_send(InputEvent::Key(key)),
                    };
                }
                Event::Mouse(evt) => {
                    app.do_send(InputEvent::Mouse(evt));
                }
                _ => {}
            }
        }
    }

    execute!(
        terminal.backend_mut(),
        DisableMouseCapture,
        LeaveAlternateScreen
    )?;

    disable_raw_mode()?;
    terminal.show_cursor()?;

    Ok(())
}
