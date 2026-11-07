//! Drawing the TUI state. Pure apart from writing into the frame.
use ratatui::{
    crossterm::event::{KeyCode, KeyEvent, KeyModifiers},
    layout::{Constraint, Layout},
    style::{Modifier, Style},
    text::Line,
    widgets::{Block, Borders, Paragraph, Row, Table},
    Frame,
};

use super::model::{App, Key, Origin, Scope, Screen};

const HELP: &str = "up/down move  Enter open  Space toggle  Tab servers/profiles  Esc back  s save  q quit";

fn origin(o: Origin) -> &'static str {
    match o {
        Origin::Nix => "nix",
        Origin::User => "user",
        Origin::Pool => "pool",
    }
}

fn highlight(i: usize, cursor: usize, row: Row<'_>) -> Row<'_> {
    if i == cursor { row.style(Style::default().add_modifier(Modifier::REVERSED)) } else { row }
}

pub fn draw(frame: &mut Frame, app: &App) {
    let [body, footer, status] =
        Layout::vertical([Constraint::Min(3), Constraint::Length(1), Constraint::Length(1)]).areas(frame.area());
    let (title, table, summary) = match &app.screen {
        Screen::Servers => {
            let rows = app.servers().into_iter().enumerate().map(|(i, s)| {
                let (tools, tokens, saved) = match &s.error {
                    Some(e) => (e.clone(), String::new(), String::new()),
                    None => (
                        format!("{}/{}", s.shown, s.total),
                        format!("{} -> {}", s.savings.full, s.savings.filtered),
                        format!("{:.1}% ", s.savings.percent_saved),
                    ),
                };
                highlight(i, app.cursor, Row::new(vec![s.name, origin(s.origin).into(), tools, tokens, saved, s.profiles.join(", ")]))
            });
            let table = Table::new(
                rows,
                [Constraint::Length(16), Constraint::Length(6), Constraint::Length(24), Constraint::Length(16), Constraint::Length(8), Constraint::Min(10)],
            )
            .header(Row::new(["Server", "Origin", "Tools", "Tokens", "Saved", "Profiles"]).style(Style::default().add_modifier(Modifier::BOLD)));
            ("Servers".to_string(), table, "token counts are estimates (o200k)".to_string())
        }
        Screen::Profiles => {
            let rows = app
                .profile_rows()
                .into_iter()
                .enumerate()
                .map(|(i, (p, s, o))| highlight(i, app.cursor, Row::new(vec![p, s, origin(o).to_string()])));
            let table = Table::new(rows, [Constraint::Length(20), Constraint::Length(20), Constraint::Min(6)])
                .header(Row::new(["Profile", "Server", "Origin"]).style(Style::default().add_modifier(Modifier::BOLD)));
            ("Profiles".to_string(), table, String::new())
        }
        Screen::Tools(scope) => {
            let tools = app.tools(scope);
            let (full, kept): (usize, usize) =
                (tools.iter().map(|t| t.tokens).sum(), tools.iter().filter(|t| t.exposed.is_some()).map(|t| t.tokens).sum());
            let shown = tools.iter().filter(|t| t.exposed.is_some()).count();
            let saved = if full == 0 { 0.0 } else { (full - kept) as f64 * 100.0 / full as f64 };
            let summary = format!("shown {shown} of {}, tokens {full} -> {kept} ({saved:.1}% saved, estimate)", tools.len());
            let rows = tools.into_iter().enumerate().map(|(i, t)| {
                let flags = [t.read_only.then_some("ro"), t.destructive.then_some("destr")].into_iter().flatten().collect::<Vec<_>>().join(" ");
                highlight(
                    i,
                    app.cursor,
                    Row::new(vec![
                        if t.exposed.is_some() { "[x]" } else { "[ ]" }.to_string(),
                        t.original,
                        t.exposed.unwrap_or_else(|| "(hidden)".into()),
                        t.tokens.to_string(),
                        flags,
                    ]),
                )
            });
            let table = Table::new(rows, [Constraint::Length(4), Constraint::Length(28), Constraint::Length(32), Constraint::Length(8), Constraint::Min(8)])
                .header(Row::new(["", "Tool", "Exposed as", "Tokens", "Flags"]).style(Style::default().add_modifier(Modifier::BOLD)));
            let title = match scope {
                Scope::Server(s) => format!("Tools of server {s} (proxy rules)"),
                Scope::Profile(p, s) => format!("Tools of {s} in profile {p}"),
            };
            (title, table, summary)
        }
    };
    let modified = if app.dirty { "  [modified]" } else { "" };
    frame.render_widget(table.block(Block::default().borders(Borders::ALL).title(format!(" mcp config-tui: {title}{modified} "))), body);
    frame.render_widget(Paragraph::new(Line::from(summary)), footer);
    frame.render_widget(Paragraph::new(Line::from(app.message.clone().unwrap_or_else(|| HELP.to_string()))), status);
}

/// Terminal key -> TUI key.
pub fn key_of(event: KeyEvent) -> Option<Key> {
    if event.modifiers.contains(KeyModifiers::CONTROL) && event.code == KeyCode::Char('c') {
        return Some(Key::Quit);
    }
    Some(match event.code {
        KeyCode::Up | KeyCode::Char('k') => Key::Up,
        KeyCode::Down | KeyCode::Char('j') => Key::Down,
        KeyCode::Enter | KeyCode::Right | KeyCode::Char('l') => Key::Enter,
        KeyCode::Esc | KeyCode::Backspace | KeyCode::Left | KeyCode::Char('h') => Key::Back,
        KeyCode::Char(' ') => Key::Toggle,
        KeyCode::Tab => Key::Tab,
        KeyCode::Char('s') => Key::Save,
        KeyCode::Char('q') => Key::Quit,
        _ => return None,
    })
}
