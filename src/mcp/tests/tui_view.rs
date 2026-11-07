use std::sync::Arc;

use mcp::tui::{
    model::{App, Inventory, Key},
    view::{draw, key_of},
};
use rafa_config::Layer;
use ratatui::{
    backend::TestBackend,
    crossterm::event::{KeyCode, KeyEvent, KeyModifiers},
    Terminal,
};
use rmcp::model::{Tool, ToolAnnotations};

fn tool(name: &str, ro: bool) -> Tool {
    let mut t = Tool::new(name.to_string(), format!("{name}: a tool that does a thing with words"), Arc::new(serde_json::Map::new()));
    if ro {
        let mut a = ToolAnnotations::new();
        a.read_only_hint = Some(true);
        t.annotations = Some(a);
    }
    t
}

fn app() -> App {
    let inventory = Inventory::from([
        ("mine".to_string(), Ok(vec![tool("alpha", true), tool("beta", false)])),
        ("down".to_string(), Err("connect: refused".to_string())),
    ]);
    let nix = Layer::parse("10-nix.yaml", "profiles:\n  ops: { servers: { mine: {} } }\n").unwrap();
    App::new(inventory, vec![nix], Layer::parse("mcp.yaml", "servers:\n  mine: { exclude: [beta] }\n").unwrap())
}

fn screen(app: &App) -> String {
    let mut terminal = Terminal::new(TestBackend::new(110, 16)).unwrap();
    terminal.draw(|f| draw(f, app)).unwrap();
    let buffer = terminal.backend().buffer().clone();
    (0..buffer.area.height)
        .map(|y| (0..buffer.area.width).map(|x| buffer[(x, y)].symbol().to_string()).collect::<String>().trim_end().to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn servers_screen_lists_origin_counts_tokens_and_profiles() {
    let text = screen(&app());
    assert!(text.contains("Servers"), "{text}");
    let mine = text.lines().find(|l| l.contains("mine")).expect(&text);
    assert!(mine.contains("user") && mine.contains("1/2") && mine.contains("% ") && mine.contains("ops"), "{mine}");
    let down = text.lines().find(|l| l.contains("down")).expect(&text);
    assert!(down.contains("connect: refused"), "{down}");
    assert!(text.contains("q quit"), "{text}");
}

#[test]
fn tools_screen_shows_toggles_names_tokens_and_flags() {
    let mut a = app();
    a.cursor = 1;
    a.update(Key::Enter);
    let text = screen(&a);
    let alpha = text.lines().find(|l| l.contains("alpha")).expect(&text);
    assert!(alpha.contains("[x]") && alpha.contains("mine-alpha") && alpha.contains("ro"), "{alpha}");
    let beta = text.lines().find(|l| l.contains("beta")).expect(&text);
    assert!(beta.contains("[ ]") && beta.contains("hidden"), "{beta}");
    assert!(text.contains("shown 1 of 2"), "{text}");
    a.update(Key::Down);
    a.update(Key::Toggle);
    assert!(screen(&a).contains("modified"), "{}", screen(&a));
}

#[test]
fn profiles_screen_and_messages() {
    let mut a = app();
    a.update(Key::Tab);
    let text = screen(&a);
    let ops = text.lines().find(|l| l.contains("ops")).expect(&text);
    assert!(ops.contains("mine") && ops.contains("nix"), "{ops}");
    a.update(Key::Tab);
    a.cursor = 0;
    a.update(Key::Enter); // down: cannot be listed
    assert!(screen(&a).contains("down: connect: refused"));
}

#[test]
fn keys_map_to_actions() {
    let k = |code| key_of(KeyEvent::new(code, KeyModifiers::NONE));
    assert_eq!(k(KeyCode::Up), Some(Key::Up));
    assert_eq!(k(KeyCode::Char('k')), Some(Key::Up));
    assert_eq!(k(KeyCode::Char('j')), Some(Key::Down));
    assert_eq!(k(KeyCode::Down), Some(Key::Down));
    assert_eq!(k(KeyCode::Enter), Some(Key::Enter));
    assert_eq!(k(KeyCode::Esc), Some(Key::Back));
    assert_eq!(k(KeyCode::Char(' ')), Some(Key::Toggle));
    assert_eq!(k(KeyCode::Tab), Some(Key::Tab));
    assert_eq!(k(KeyCode::Char('s')), Some(Key::Save));
    assert_eq!(k(KeyCode::Char('q')), Some(Key::Quit));
    assert_eq!(key_of(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)), Some(Key::Quit));
    assert_eq!(k(KeyCode::Char('z')), None);
}
