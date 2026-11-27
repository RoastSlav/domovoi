//! A cap on concurrent connections, in total and per source address, for the two network
//! ports. Idle or half-open sockets each hold a file descriptor; without a cap one host with a
//! script can use every descriptor the hub has and take the UI and the agents down together.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, Mutex};

/// How many connections one port allows at once.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConnLimits {
    /// Connections from all addresses together.
    pub total: usize,
    /// Connections from any one address.
    pub per_source: usize,
}

/// Why a connection was refused, with the counts that made it so.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Refused {
    /// True when the address was at its own limit, false when the port was full.
    pub source_full: bool,
    /// Connections open from the address when it was refused.
    pub from_source: usize,
    /// Connections open from all addresses when it was refused.
    pub in_total: usize,
}

#[derive(Debug, Default)]
struct Counts {
    total: usize,
    by_source: HashMap<IpAddr, usize>,
}

/// Counts open connections and refuses new ones past the limits. Cheap to clone; clones
/// share the counts.
#[derive(Debug, Clone)]
pub struct ConnGate {
    limits: ConnLimits,
    counts: Arc<Mutex<Counts>>,
}

impl ConnGate {
    /// A gate that allows `limits`.
    #[must_use]
    pub fn new(limits: ConnLimits) -> Self {
        Self {
            limits,
            counts: Arc::new(Mutex::new(Counts::default())),
        }
    }

    /// The limits this gate enforces.
    #[must_use]
    pub fn limits(&self) -> ConnLimits {
        self.limits
    }

    /// Takes a place for a connection from `source`. The place is given back when the
    /// returned permit drops.
    pub fn try_acquire(&self, source: IpAddr) -> Result<ConnPermit, Refused> {
        // A poisoned lock only means another thread panicked while counting; the counts are
        // plain integers, so keep going rather than refuse every connection.
        let mut counts = self
            .counts
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let from_source = counts.by_source.get(&source).copied().unwrap_or(0);
        if from_source >= self.limits.per_source || counts.total >= self.limits.total {
            return Err(Refused {
                source_full: from_source >= self.limits.per_source,
                from_source,
                in_total: counts.total,
            });
        }
        counts.total += 1;
        *counts.by_source.entry(source).or_insert(0) += 1;
        Ok(ConnPermit {
            gate: self.clone(),
            source,
        })
    }

    /// Connections open now, from all addresses.
    #[must_use]
    pub fn open(&self) -> usize {
        self.counts
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .total
    }

    fn release(&self, source: IpAddr) {
        let mut counts = self
            .counts
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        counts.total = counts.total.saturating_sub(1);
        if let Some(open) = counts.by_source.get_mut(&source) {
            *open = open.saturating_sub(1);
            if *open == 0 {
                counts.by_source.remove(&source);
            }
        }
    }
}

/// A connection's place under the cap; dropping it frees the place.
#[derive(Debug)]
pub struct ConnPermit {
    gate: ConnGate,
    source: IpAddr,
}

impl Drop for ConnPermit {
    fn drop(&mut self) {
        self.gate.release(self.source);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(last: u8) -> IpAddr {
        IpAddr::from([10, 0, 0, last])
    }

    #[test]
    fn one_address_is_limited_before_the_port_is_full() {
        let gate = ConnGate::new(ConnLimits {
            total: 10,
            per_source: 2,
        });
        let first = gate.try_acquire(ip(1)).unwrap();
        let _second = gate.try_acquire(ip(1)).unwrap();
        let refused = gate.try_acquire(ip(1)).unwrap_err();
        assert!(refused.source_full);
        assert_eq!(refused.from_source, 2);
        assert!(gate.try_acquire(ip(2)).is_ok(), "others are not affected");
        drop(first);
        assert!(gate.try_acquire(ip(1)).is_ok(), "a freed place is reusable");
    }

    #[test]
    fn the_total_caps_many_addresses_and_frees_on_drop() {
        let gate = ConnGate::new(ConnLimits {
            total: 3,
            per_source: 3,
        });
        let held: Vec<_> = (1..=3).map(|n| gate.try_acquire(ip(n)).unwrap()).collect();
        let refused = gate.try_acquire(ip(9)).unwrap_err();
        assert!(!refused.source_full);
        assert_eq!(refused.in_total, 3);
        drop(held);
        assert_eq!(gate.open(), 0);
        assert!(gate.try_acquire(ip(9)).is_ok());
    }
}
