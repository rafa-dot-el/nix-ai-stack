//! `mcp config-tui`: per-server and per-profile tool toggles with token estimates.
//! [`model`] is the pure state and reducer; [`view`] draws it; [`run`] is the terminal loop.
pub mod model;
pub mod view;

use ratatui::crossterm::event::{self, Event, KeyEventKind};

use model::{App, Effect};

/// The terminal loop: draw, read a key, update; `save` writes the user layer.
pub fn run(mut app: App, mut save: impl FnMut(&rafa_config::Value) -> Result<(), String>) -> Result<(), String> {
    let mut terminal = ratatui::try_init().map_err(|e| format!("terminal: {e}"))?;
    let result = (|| loop {
        terminal.draw(|f| view::draw(f, &app)).map_err(|e| e.to_string())?;
        let Event::Key(press) = event::read().map_err(|e| e.to_string())? else { continue };
        if press.kind != KeyEventKind::Press {
            continue;
        }
        let Some(key) = view::key_of(press) else { continue };
        match app.update(key) {
            Effect::Quit => return Ok(()),
            Effect::Save(value) => {
                if let Err(e) = save(&value) {
                    app.dirty = true;
                    app.message = Some(format!("not saved: {e}"));
                }
            }
            Effect::None => {}
        }
    })();
    ratatui::restore();
    result
}
