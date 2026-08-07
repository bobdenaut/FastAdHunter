//! FastAdHunter TUI monitor — startup and wiring only.
//!
//! Everything below this file obeys one direction of flow, and no layer skips
//! the next:
//!
//! ```text
//! HTTP/WS  →  client::*  →  models::*  →  workers::*  →  state::AppState  →  ui::*
//! ```
//!
//! A drawing function receives `&AppState` and nothing else — it cannot reach a
//! socket, and a worker cannot reach a widget. `serde_json::Value` exists only
//! inside `client`/`models`, where bytes become types; past that boundary every
//! figure is typed.

mod app;
mod client;
mod config;
mod models;
mod state;
mod ui;
mod util;
mod workers;

use std::error::Error;

/// `Send + Sync` throughout: startup failures cross a `spawn_blocking`
/// boundary, and a plain `Box<dyn Error>` cannot.
pub type BoxError = Box<dyn Error + Send + Sync>;

#[tokio::main]
async fn main() -> Result<(), BoxError> {
    let path = match config::Startup::from_args(std::env::args_os().skip(1))? {
        config::Startup::Help => {
            println!("{}", config::USAGE);
            return Ok(());
        }
        config::Startup::Run(path) => path,
    };

    let config = config::Config::load(&path)?;
    let clients = client::Clients::new(&config)?;
    let state = state::SharedState::new(config.ui.limits());

    workers::spawn(clients, &config, state.clone());

    // The UI loop is synchronous — `event::poll` blocks — so it runs on the
    // blocking pool rather than parking one of the runtime's worker threads for
    // the life of the process.
    tokio::task::spawn_blocking(move || app::App::new(state, config.ui).run()).await??;
    Ok(())
}
