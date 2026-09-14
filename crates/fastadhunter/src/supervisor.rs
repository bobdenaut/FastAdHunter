use std::fmt;

use tokio::task::{JoinError, JoinHandle};

pub struct Supervised {
    pub name: &'static str,
    pub handle: JoinHandle<()>,
}

impl Supervised {
    pub fn new(name: &'static str, handle: JoinHandle<()>) -> Self {
        Self { name, handle }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Cause {
    Panicked(String),
    Returned,
    Cancelled,
}

impl fmt::Display for Cause {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Cause::Panicked(message) => write!(f, "panicked: {message}"),
            Cause::Returned => f.write_str("returned"),
            Cause::Cancelled => f.write_str("cancelled"),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct Death {
    pub name: &'static str,
    pub cause: Cause,
}

pub async fn reap(tasks: &mut Vec<Supervised>) -> Vec<Death> {
    let mut deaths = Vec::new();
    let mut index = 0;
    while index < tasks.len() {
        if !tasks[index].handle.is_finished() {
            index += 1;
            continue;
        }
        let task = tasks.swap_remove(index);
        deaths.push(death_of(task.name, task.handle).await);
    }
    deaths
}

pub async fn death_of(name: &'static str, handle: JoinHandle<()>) -> Death {
    let cause = match handle.await {
        Ok(()) => Cause::Returned,
        Err(err) => classify(err),
    };
    Death { name, cause }
}

fn classify(err: JoinError) -> Cause {
    match err.try_into_panic() {
        Ok(payload) => {
            let message = payload
                .downcast_ref::<&str>()
                .map(|message| (*message).to_owned())
                .or_else(|| payload.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "non-string panic payload".to_owned());
            Cause::Panicked(message)
        }
        Err(_) => Cause::Cancelled,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn settled(tasks: &[Supervised]) {
        while tasks.iter().any(|task| !task.handle.is_finished()) {
            tokio::task::yield_now().await;
        }
    }

    #[tokio::test]
    async fn a_panicking_task_is_reported_with_its_message() {
        let mut tasks = vec![
            Supervised::new("literal", tokio::spawn(async { panic!("boom") })),
            Supervised::new("formatted", tokio::spawn(async { panic!("code {}", 7) })),
        ];
        settled(&tasks).await;

        let mut deaths = reap(&mut tasks).await;
        deaths.sort_by_key(|death| death.name);

        assert_eq!(
            deaths,
            vec![
                Death {
                    name: "formatted",
                    cause: Cause::Panicked("code 7".to_owned()),
                },
                Death {
                    name: "literal",
                    cause: Cause::Panicked("boom".to_owned()),
                },
            ]
        );
        assert!(tasks.is_empty());
    }

    #[tokio::test]
    async fn a_task_that_returns_is_reported_as_returned() {
        let mut tasks = vec![Supervised::new("loop", tokio::spawn(async {}))];
        settled(&tasks).await;

        let deaths = reap(&mut tasks).await;

        assert_eq!(
            deaths,
            vec![Death {
                name: "loop",
                cause: Cause::Returned,
            }]
        );
    }

    #[tokio::test]
    async fn an_aborted_task_is_reported_as_cancelled() {
        let mut tasks = vec![Supervised::new(
            "pending",
            tokio::spawn(std::future::pending()),
        )];
        tasks[0].handle.abort();
        settled(&tasks).await;

        let deaths = reap(&mut tasks).await;

        assert_eq!(deaths[0].cause, Cause::Cancelled);
    }

    #[tokio::test]
    async fn a_running_task_is_neither_reported_nor_removed() {
        let mut tasks = vec![Supervised::new(
            "pending",
            tokio::spawn(std::future::pending()),
        )];

        let deaths = reap(&mut tasks).await;

        assert!(deaths.is_empty());
        assert_eq!(tasks.len(), 1);
        tasks[0].handle.abort();
    }

    #[tokio::test]
    async fn a_death_is_reported_once_and_the_survivors_stay() {
        let mut tasks = vec![
            Supervised::new("dead", tokio::spawn(async { panic!("once") })),
            Supervised::new("alive", tokio::spawn(std::future::pending())),
        ];
        while !tasks[0].handle.is_finished() {
            tokio::task::yield_now().await;
        }

        let first = reap(&mut tasks).await;
        let second = reap(&mut tasks).await;

        assert_eq!(first.len(), 1);
        assert_eq!(first[0].name, "dead");
        assert!(second.is_empty());
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].name, "alive");
        tasks[0].handle.abort();
    }

    #[tokio::test]
    async fn a_handle_handed_over_on_its_own_is_classified_the_same_way() {
        let panicked = tokio::spawn(async { panic!("handed over") });
        assert_eq!(
            death_of("HTTP acceptor", panicked).await,
            Death {
                name: "HTTP acceptor",
                cause: Cause::Panicked("handed over".to_owned()),
            }
        );

        let returned = tokio::spawn(async {});
        assert_eq!(
            death_of("API acceptor", returned).await,
            Death {
                name: "API acceptor",
                cause: Cause::Returned,
            }
        );
    }

    #[tokio::test]
    async fn an_empty_set_reaps_nothing() {
        let mut tasks: Vec<Supervised> = Vec::new();
        assert!(reap(&mut tasks).await.is_empty());
    }

    #[test]
    fn causes_read_as_one_line() {
        assert_eq!(
            Cause::Panicked("boom".to_owned()).to_string(),
            "panicked: boom"
        );
        assert_eq!(Cause::Returned.to_string(), "returned");
        assert_eq!(Cause::Cancelled.to_string(), "cancelled");
    }
}
