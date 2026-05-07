use anyhow::Result;
use crossterm::{
    event::{
        DisableMouseCapture, EnableMouseCapture, Event, EventStream, KeyEventKind,
        KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
    },
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use futures::StreamExt;
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io::stdout;
use tokio::sync::mpsc;

mod api;
mod app;
mod config;
mod ui;
mod welcome;

use app::App;
use config::{Config, LoadError};

#[tokio::main]
async fn main() -> Result<()> {
    enable_raw_mode()?;
    let mut out = stdout();
    execute!(out, EnterAlternateScreen, EnableMouseCapture)?;
    // Best-effort: ask for the Kitty keyboard protocol so Ctrl+Enter,
    // Shift+Enter, etc. arrive distinctly. Terminals that don't support it
    // silently ignore this and our F2 / Alt+Enter fallbacks still work.
    let _ = execute!(
        out,
        PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
    );
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout()))?;

    let result = run_app(&mut terminal).await;

    let _ = execute!(terminal.backend_mut(), PopKeyboardEnhancementFlags);
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen, DisableMouseCapture)?;
    terminal.show_cursor()?;
    result
}

async fn run_app<B: ratatui::backend::Backend>(terminal: &mut Terminal<B>) -> Result<()> {
    loop {
        let config = match Config::load() {
            Ok(c) => c,
            Err(LoadError::Invalid(e)) => return Err(e),
            Err(LoadError::Missing) => match welcome::run(terminal).await? {
                welcome::WelcomeOutcome::Quit => return Ok(()),
                welcome::WelcomeOutcome::Token(token) => {
                    Config::save_token(&token)?;
                    Config { api_token: token }
                }
            },
        };
        let client = api::Client::new(config.api_token);
        match run_main(terminal, client).await? {
            ExitReason::Quit => return Ok(()),
            ExitReason::Reauth => continue,
        }
    }
}

enum ExitReason {
    Quit,
    Reauth,
}

async fn run_main<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    client: api::Client,
) -> Result<ExitReason> {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let mut app = App::new(client, tx);
    app.bootstrap();

    let mut events = EventStream::new();

    loop {
        terminal.draw(|f| ui::render(f, &mut app))?;
        if app.should_quit {
            break;
        }
        tokio::select! {
            maybe_event = events.next() => {
                if let Some(Ok(Event::Key(key))) = maybe_event {
                    if key.kind == KeyEventKind::Press {
                        app.handle_key(key);
                    }
                }
            }
            Some(msg) = rx.recv() => {
                app.update(msg);
            }
        }
    }
    Ok(if app.should_reauth { ExitReason::Reauth } else { ExitReason::Quit })
}
