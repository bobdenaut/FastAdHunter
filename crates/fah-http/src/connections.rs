use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

#[derive(Debug, Default)]
pub struct ConnectionGauge {
    open: AtomicU32,
    peak: AtomicU32,
}

impl ConnectionGauge {
    pub(crate) fn enter(self: &Arc<Self>) -> OpenConnection {
        let open = self.open.fetch_add(1, Ordering::Relaxed) + 1;
        self.peak.fetch_max(open, Ordering::Relaxed);
        OpenConnection(Arc::clone(self))
    }

    pub fn open(&self) -> u32 {
        self.open.load(Ordering::Relaxed)
    }

    pub fn take_peak(&self) -> u32 {
        let peak = self.peak.swap(0, Ordering::Relaxed);
        self.peak.fetch_max(self.open(), Ordering::Relaxed);
        peak
    }
}

#[derive(Debug)]
pub(crate) struct OpenConnection(Arc<ConnectionGauge>);

impl Drop for OpenConnection {
    fn drop(&mut self) {
        self.0.open.fetch_sub(1, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn peak_is_the_most_open_at_once_and_resets_to_what_is_still_open() {
        let gauge = Arc::new(ConnectionGauge::default());
        let a = gauge.enter();
        let b = gauge.enter();
        drop(a);
        let c = gauge.enter();
        assert_eq!(gauge.open(), 2);
        assert_eq!(gauge.take_peak(), 2);
        drop(b);
        assert_eq!(gauge.take_peak(), 2);
        assert_eq!(gauge.take_peak(), 1);
        drop(c);
        assert_eq!(gauge.take_peak(), 1);
        assert_eq!(gauge.take_peak(), 0);
        assert_eq!(gauge.open(), 0);
    }
}
