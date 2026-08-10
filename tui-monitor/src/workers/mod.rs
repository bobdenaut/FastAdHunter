//! Background tasks: the only code that both talks to a client and writes
//! [`AppState`].
//!
//! A worker never renders and never reads UI state; the UI never polls. That
//! contract is what keeps a new panel to one widget plus, at most, one field
//! in the state.

pub mod events;
pub mod history;
pub mod routeros;
pub mod telemetry;

use crate::client::Clients;
use crate::config::Config;
use crate::state::SharedState;

/// Starts every worker. Each owns its client and its cadence, and each writes a
/// disjoint part of the state.
pub fn spawn(clients: Clients, config: &Config, state: SharedState) {
    let Clients {
        api,
        events,
        routeros,
    } = clients;

    tokio::spawn(telemetry::run(api.clone(), config.poll, state.clone()));
    tokio::spawn(history::run_summary(
        api.clone(),
        config.poll,
        state.clone(),
    ));
    tokio::spawn(history::run_perf(api.clone(), config.poll, state.clone()));
    tokio::spawn(events::run(
        events,
        config.timeout.reconnect(),
        config.timeout.events_idle(),
        state.clone(),
    ));

    if let Some(client) = routeros {
        tokio::spawn(routeros::run(
            client,
            api,
            config.routeros.auto_name,
            config.poll,
            state,
        ));
    }
}
