use anyhow::Result;
use crossterm::event::{Event, EventStream, KeyCode, KeyEventKind, KeyModifiers};
use futures::StreamExt;
use ratatui::{
    backend::Backend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
    Frame, Terminal,
};
use tui_textarea::TextArea;

const LOGO: &[&str] = &[
    "  ██████╗██╗     ██╗ ██████╗██╗  ██╗██╗   ██╗██████╗ ",
    " ██╔════╝██║     ██║██╔════╝██║ ██╔╝██║   ██║██╔══██╗",
    " ██║     ██║     ██║██║     █████╔╝ ██║   ██║██████╔╝",
    " ██║     ██║     ██║██║     ██╔═██╗ ██║   ██║██╔═══╝ ",
    " ╚██████╗███████╗██║╚██████╗██║  ██╗╚██████╔╝██║     ",
    "  ╚═════╝╚══════╝╚═╝ ╚═════╝╚═╝  ╚═╝ ╚═════╝ ╚═╝     ",
];

pub enum WelcomeOutcome {
    Token(String),
    Quit,
}

struct WelcomeState {
    input: TextArea<'static>,
    show_settings: bool,
    error: Option<String>,
}

pub async fn run<B: Backend>(terminal: &mut Terminal<B>) -> Result<WelcomeOutcome> {
    let mut state = WelcomeState {
        input: {
            let mut ta = TextArea::default();
            ta.set_placeholder_text("paste your ClickUp Personal API token");
            ta.set_mask_char('•');
            ta.set_cursor_line_style(Style::default());
            ta
        },
        show_settings: false,
        error: None,
    };

    let mut events = EventStream::new();
    loop {
        terminal.draw(|f| render(f, &state))?;
        let Some(Ok(Event::Key(key))) = events.next().await else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        if state.show_settings {
            match key.code {
                KeyCode::Esc => state.show_settings = false,
                KeyCode::Enter => return Ok(WelcomeOutcome::Quit),
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    return Ok(WelcomeOutcome::Quit);
                }
                _ => {}
            }
            continue;
        }
        match key.code {
            KeyCode::Esc => state.show_settings = true,
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                return Ok(WelcomeOutcome::Quit);
            }
            KeyCode::Enter => {
                let token = state.input.lines().join("").trim().to_string();
                if token.is_empty() {
                    state.error = Some("token can't be empty".into());
                } else {
                    return Ok(WelcomeOutcome::Token(token));
                }
            }
            _ => {
                state.input.input(key);
                state.error = None;
            }
        }
    }
}

fn render(f: &mut Frame, state: &WelcomeState) {
    let area = f.area();

    // Vertically center the welcome content. Logo is 6 lines + paddings.
    let block_h: u16 = 22;
    let top_spacer = area.height.saturating_sub(block_h) / 2;

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(top_spacer),
            Constraint::Length(LOGO.len() as u16),
            Constraint::Length(1), // tui label
            Constraint::Length(2), // welcome line
            Constraint::Length(3), // input box
            Constraint::Length(1), // hint
            Constraint::Length(1), // error (always reserved)
            Constraint::Min(0),
            Constraint::Length(1), // footer
        ])
        .split(area);

    // Logo
    let logo_lines: Vec<Line> = LOGO
        .iter()
        .map(|l| {
            Line::from(Span::styled(
                *l,
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            ))
        })
        .collect();
    f.render_widget(
        Paragraph::new(logo_lines).alignment(Alignment::Center),
        chunks[1],
    );

    // tui subtitle
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "─ tui ─",
            Style::default().fg(Color::DarkGray),
        )))
        .alignment(Alignment::Center),
        chunks[2],
    );

    // welcome line
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "Welcome — let's get you signed in.",
            Style::default().add_modifier(Modifier::BOLD),
        )))
        .alignment(Alignment::Center),
        chunks[3],
    );

    // Input box (centered, fixed width)
    let input_area = centered_x(60, chunks[4]);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" ClickUp Personal API token ")
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(input_area);
    f.render_widget(block, input_area);
    f.render_widget(&state.input, inner);

    // hint
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "Get one at app.clickup.com → Settings → Apps",
            Style::default().fg(Color::DarkGray),
        )))
        .alignment(Alignment::Center),
        chunks[5],
    );

    // error
    if let Some(err) = &state.error {
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                err.clone(),
                Style::default().fg(Color::Red),
            )))
            .alignment(Alignment::Center),
            chunks[6],
        );
    }

    // footer
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "⏎ sign in    esc settings    ⌃c quit",
            Style::default().fg(Color::DarkGray),
        )))
        .alignment(Alignment::Center),
        chunks[8],
    );

    if state.show_settings {
        render_settings(f, area);
    }
}

fn render_settings(f: &mut Frame, area: Rect) {
    let modal = centered(60, 70, area);
    f.render_widget(Clear, modal);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Settings — esc back   ⏎ exit app ")
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(modal);
    f.render_widget(block, modal);

    let bold = Style::default().add_modifier(Modifier::BOLD);
    let dim = Style::default().fg(Color::DarkGray);

    let lines = vec![
        Line::from(Span::styled("Keys", bold)),
        Line::from("  ⏎       sign in with the token you typed"),
        Line::from("  esc     close this menu / cancel input"),
        Line::from("  ⌃c      quit"),
        Line::raw(""),
        Line::from(Span::styled("Where is my token?", bold)),
        Line::from("  app.clickup.com → Settings → Apps"),
        Line::from("  Click ‘Generate’ under Personal API Token."),
        Line::raw(""),
        Line::from(Span::styled("Why a token, not a password?", bold)),
        Line::from("  Tokens have the same access as your account but"),
        Line::from("  can be revoked any time without changing your"),
        Line::from("  password. Stored locally at:"),
        Line::from(Span::styled(
            "  ~/.config/clickup-tui/config.toml",
            dim,
        )),
        Line::raw(""),
        Line::from(Span::styled("Once signed in", bold)),
        Line::from("  Press  /  to search the workspace"),
        Line::from("  Press  f  to filter by assignee/status/etc."),
        Line::from("  Press  ↑  from a top-row pane to open the dashboard"),
        Line::raw(""),
        Line::from(Span::styled(
            "⏎  exit app",
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        )),
    ];
    f.render_widget(Paragraph::new(lines), inner);
}

fn centered(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let popup_h = area.height * percent_y / 100;
    let popup_w = area.width * percent_x / 100;
    let y = area.height.saturating_sub(popup_h) / 2;
    let x = area.width.saturating_sub(popup_w) / 2;
    Rect::new(area.x + x, area.y + y, popup_w, popup_h)
}

fn centered_x(width: u16, area: Rect) -> Rect {
    let w = width.min(area.width);
    let x = area.x + area.width.saturating_sub(w) / 2;
    Rect::new(x, area.y, w, area.height)
}
