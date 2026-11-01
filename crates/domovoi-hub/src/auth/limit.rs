//! Login attempts per client address: five failures a minute, then a lockout that doubles
//! from one minute up to fifteen.

use std::collections::{BTreeMap, VecDeque};
use std::net::IpAddr;

use jiff::{SignedDuration, Timestamp};

/// Failed logins allowed per address in a rolling minute.
pub const FAILURES_PER_MINUTE: usize = 5;

/// The first lockout.
pub const FIRST_LOCKOUT: SignedDuration = SignedDuration::from_mins(1);

/// The longest lockout.
pub const MAX_LOCKOUT: SignedDuration = SignedDuration::from_mins(15);

#[derive(Debug, Default)]
struct Entry {
    failures: VecDeque<Timestamp>,
    lockouts: u32,
    locked_until: Option<Timestamp>,
}

/// Tracks failed logins in memory.
#[derive(Debug, Default)]
pub struct LoginLimiter {
    entries: BTreeMap<IpAddr, Entry>,
}

impl LoginLimiter {
    /// How long `ip` must still wait, or `None` when it may try.
    pub fn locked(&self, ip: IpAddr, now: Timestamp) -> Option<SignedDuration> {
        let until = self.entries.get(&ip)?.locked_until?;
        (until > now).then(|| until.duration_since(now))
    }

    /// Records a failed login from `ip`.
    pub fn failed(&mut self, ip: IpAddr, now: Timestamp) {
        let entry = self.entries.entry(ip).or_default();
        let minute_ago = now.checked_sub(SignedDuration::from_mins(1)).unwrap_or(now);
        while entry.failures.front().is_some_and(|t| *t <= minute_ago) {
            entry.failures.pop_front();
        }
        entry.failures.push_back(now);
        if entry.failures.len() >= FAILURES_PER_MINUTE {
            entry.failures.clear();
            let factor = 1_i64 << entry.lockouts.min(4);
            let lockout = FIRST_LOCKOUT
                .checked_mul(i32::try_from(factor).unwrap_or(i32::MAX))
                .unwrap_or(MAX_LOCKOUT)
                .min(MAX_LOCKOUT);
            entry.lockouts += 1;
            entry.locked_until = now.checked_add(lockout).ok();
        }
    }

    /// Forgets `ip` after a successful login.
    pub fn succeeded(&mut self, ip: IpAddr) {
        self.entries.remove(&ip);
    }
}
