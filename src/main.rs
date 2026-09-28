mod api;
mod app;
mod config;
mod media;
mod player;
mod ui;

use ratatui::crossterm::event::{self, Event};
use std::time::Duration;

fn main() -> std::io::Result<()> {
    let cfg = config::Config::load();
    let client = api::Client::new(cfg);
    let mut app = app::App::new(client);

    let mut terminal = ratatui::init();
    let result = run(&mut terminal, &mut app);
    ratatui::restore();
    app.shutdown();
    drop(app); // stops mpv
    result
}

fn run(terminal: &mut ratatui::DefaultTerminal, app: &mut app::App) -> std::io::Result<()> {
    while !app.quit {
        app.drain();
        app.tick();
        terminal.draw(|f| ui::draw(f, app))?;
        if event::poll(Duration::from_millis(200))? {
            if let Event::Key(k) = event::read()? {
                app.on_key(k);
            }
        }
    }
    Ok(())
}
