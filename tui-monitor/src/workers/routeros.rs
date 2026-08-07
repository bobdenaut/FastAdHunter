//! The router's own figures for the footer. Two read-only GETs per tick.

use crate::client::RouterOsClient;
use crate::config::PollConfig;
use crate::state::SharedState;

pub async fn run(client: RouterOsClient, poll: PollConfig, state: SharedState) {
    let mut ticker = tokio::time::interval(poll.routeros());
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    loop {
        ticker.tick().await;

        // Concurrent: two independent reads of the same device, and the tick
        // budget is for both together.
        let (resource, container) = tokio::join!(client.system_resource(), client.container());

        state.update(|app| {
            let router = &mut app.router;
            // Each half is applied only when it succeeded, so one failing read
            // does not blank the figures the other one just refreshed.
            if let Ok(resource) = resource {
                router.free_memory = resource.free_memory;
                router.total_memory = resource.total_memory;
                router.cpu_load = resource.cpu_load;
                router.cpu_frequency = resource.cpu_frequency;
                router.uptime = resource.uptime;
            }
            if let Ok(container) = container {
                router.container_memory = container.as_ref().and_then(|c| c.memory_current);
                router.container_status = container.and_then(|c| c.status);
            }
        });
    }
}
