//! Tracing initialization, log formats, and level configuration (ARCHITECTURE.md L1).

use tracing::Dispatch;
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::reload;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::Registry;

/// Log line format (CONFIGURATION.md `[log].format`, boot-only).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogFormat {
    Text,
    Json,
}

/// Handle returned by [`init`], allowing the log level to change at runtime
/// (CONFIGURATION.md `[log].level` is runtime-mutable) without rebuilding the
/// whole subscriber.
pub struct LoggingHandle {
    level: reload::Handle<LevelFilter, Registry>,
}

impl LoggingHandle {
    /// Changes the active level filter. Fails only if the subscriber the
    /// handle was created for has since been dropped.
    pub fn set_level(&self, level: LevelFilter) -> Result<(), reload::Error> {
        self.level.reload(level)
    }
}

fn build_dispatch<W>(
    level: LevelFilter,
    format: LogFormat,
    writer: W,
) -> (Dispatch, reload::Handle<LevelFilter, Registry>)
where
    W: for<'a> MakeWriter<'a> + Send + Sync + 'static,
{
    let (filter, reload_handle) = reload::Layer::new(level);
    let registry = Registry::default().with(filter);
    let dispatch = match format {
        LogFormat::Text => Dispatch::new(
            registry.with(
                // No ANSI: the deploy target is a container whose stderr is captured
                // by the log system (RouterOS), not a TTY. Colour escapes there are
                // printed literally ("1B[2m…"), making the log unreadable.
                tracing_subscriber::fmt::layer()
                    .with_ansi(false)
                    .with_writer(writer),
            ),
        ),
        LogFormat::Json => Dispatch::new(
            registry.with(tracing_subscriber::fmt::layer().json().with_writer(writer)),
        ),
    };
    (dispatch, reload_handle)
}

/// Builds the tracing subscriber and installs it as the process-wide default.
///
/// Safe to call more than once: a later call that finds a subscriber already
/// installed is a no-op rather than a panic, so tests can call `init` freely.
pub fn init(level: LevelFilter, format: LogFormat) -> LoggingHandle {
    let (dispatch, reload_handle) = build_dispatch(level, format, std::io::stderr);
    let _ = dispatch.try_init();
    LoggingHandle {
        level: reload_handle,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;

    #[derive(Clone, Default)]
    struct VecWriter(Arc<Mutex<Vec<u8>>>);

    impl VecWriter {
        fn contents(&self) -> String {
            String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
        }
    }

    impl std::io::Write for VecWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl<'a> MakeWriter<'a> for VecWriter {
        type Writer = VecWriter;

        fn make_writer(&'a self) -> Self::Writer {
            self.clone()
        }
    }

    #[test]
    fn init_is_idempotent_safe() {
        let _ = init(LevelFilter::INFO, LogFormat::Text);
        let _ = init(LevelFilter::INFO, LogFormat::Text);
    }

    #[test]
    fn level_filter_honored() {
        let writer = VecWriter::default();
        let (dispatch, _reload) =
            build_dispatch(LevelFilter::WARN, LogFormat::Text, writer.clone());
        tracing::dispatcher::with_default(&dispatch, || {
            tracing::info!("should be filtered");
            tracing::warn!("should appear");
        });
        let text = writer.contents();
        assert!(!text.contains("should be filtered"));
        assert!(text.contains("should appear"));
    }

    #[test]
    fn json_format_produces_parseable_json() {
        let writer = VecWriter::default();
        let (dispatch, _reload) =
            build_dispatch(LevelFilter::INFO, LogFormat::Json, writer.clone());
        tracing::dispatcher::with_default(&dispatch, || {
            tracing::info!("hello");
        });
        let text = writer.contents();
        let line = text.lines().next().unwrap();
        assert!(line.starts_with('{') && line.ends_with('}'));
        assert!(line.contains("\"message\":\"hello\""));
        let open = line.matches('{').count();
        let close = line.matches('}').count();
        assert_eq!(open, close);
    }

    #[test]
    fn reload_changes_level_at_runtime() {
        let writer = VecWriter::default();
        let (dispatch, reload_handle) =
            build_dispatch(LevelFilter::ERROR, LogFormat::Text, writer.clone());
        tracing::dispatcher::with_default(&dispatch, || {
            tracing::info!("filtered before reload");
        });
        reload_handle.reload(LevelFilter::INFO).unwrap();
        tracing::dispatcher::with_default(&dispatch, || {
            tracing::info!("visible after reload");
        });
        let text = writer.contents();
        assert!(!text.contains("filtered before reload"));
        assert!(text.contains("visible after reload"));
    }
}
