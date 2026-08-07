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

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let config = config::Config::load()?;
    let clients = client::Clients::new(&config)?;
    let state = state::SharedState::new(config.ui.limits());

    workers::spawn(clients, &config, state.clone());

    app::App::new(state, config.ui).run().await
}
