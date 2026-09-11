use std::ops::Deref;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

#[derive(Debug, Default)]
pub struct ConnectionGauge {
    open: AtomicU32,
    peak: AtomicU32,
}

impl ConnectionGauge {
    pub fn open(&self) -> u32 {
        self.open.load(Ordering::Relaxed)
    }

    pub fn peak(&self) -> u32 {
        self.peak.load(Ordering::Relaxed)
    }

    pub fn take_peak(&self) -> u32 {
        let peak = self.peak.swap(0, Ordering::Relaxed);
        self.peak.fetch_max(self.open(), Ordering::Relaxed);
        peak
    }
}

impl AsRef<ConnectionGauge> for ConnectionGauge {
    fn as_ref(&self) -> &ConnectionGauge {
        self
    }
}

#[derive(Debug)]
pub struct OpenConnection<T: AsRef<ConnectionGauge>>(Arc<T>);

impl<T: AsRef<ConnectionGauge>> OpenConnection<T> {
    pub fn enter(owner: &Arc<T>) -> Self {
        let gauge = T::as_ref(owner);
        let open = gauge.open.fetch_add(1, Ordering::Relaxed) + 1;
        gauge.peak.fetch_max(open, Ordering::Relaxed);
        Self(Arc::clone(owner))
    }
}

impl<T: AsRef<ConnectionGauge>> Deref for OpenConnection<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.0
    }
}

impl<T: AsRef<ConnectionGauge>> Drop for OpenConnection<T> {
    fn drop(&mut self) {
        T::as_ref(&self.0).open.fetch_sub(1, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn peak_is_the_most_open_at_once_and_resets_to_what_is_still_open() {
        let gauge = Arc::new(ConnectionGauge::default());
        let a = OpenConnection::enter(&gauge);
        let b = OpenConnection::enter(&gauge);
        drop(a);
        let c = OpenConnection::enter(&gauge);
        assert_eq!(gauge.open(), 2);
        assert_eq!(gauge.peak(), 2);
        assert_eq!(gauge.take_peak(), 2);
        drop(b);
        assert_eq!(gauge.take_peak(), 2);
        assert_eq!(gauge.take_peak(), 1);
        drop(c);
        assert_eq!(gauge.take_peak(), 1);
        assert_eq!(gauge.take_peak(), 0);
        assert_eq!(gauge.open(), 0);
    }

    struct Wrapped {
        connections: ConnectionGauge,
    }

    impl AsRef<ConnectionGauge> for Wrapped {
        fn as_ref(&self) -> &ConnectionGauge {
            &self.connections
        }
    }

    #[test]
    fn a_wrapping_owner_is_reachable_through_the_guard_and_counted_on_drop() {
        let owner = Arc::new(Wrapped {
            connections: ConnectionGauge::default(),
        });
        let open = OpenConnection::enter(&owner);
        assert_eq!(open.connections.open(), 1);
        assert_eq!(Arc::strong_count(&owner), 2);
        drop(open);
        assert_eq!(owner.connections.open(), 0);
        assert_eq!(Arc::strong_count(&owner), 1);
    }
}
