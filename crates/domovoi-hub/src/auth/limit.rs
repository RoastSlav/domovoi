//! Login attempts per client address and per username: five failures a minute, then a lockout that doubles
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

/// Tracks failed logins in memory, by client address (the default key) or by username.
#[derive(Debug)]
pub struct LoginLimiter<K: Ord = IpAddr> {
    entries: BTreeMap<K, Entry>,
}

impl<K: Ord> Default for LoginLimiter<K> {
    fn default() -> Self {
        Self {
            entries: BTreeMap::new(),
        }
    }
}

impl<K: Ord> LoginLimiter<K> {
    /// How long `ip` must still wait, or `None` when it may try.
    pub fn locked(&self, ip: K, now: Timestamp) -> Option<SignedDuration> {
        let until = self.entries.get(&ip)?.locked_until?;
        (until > now).then(|| until.duration_since(now))
    }

    /// Records a failed login from `ip`. Returns the length of the lockout when this failure
    /// started one, so the caller can log it once.
    pub fn failed(&mut self, ip: K, now: Timestamp) -> Option<SignedDuration> {
        let entry = self.entries.entry(ip).or_default();
        let minute_ago = now.checked_sub(SignedDuration::from_mins(1)).unwrap_or(now);
        while entry.failures.front().is_some_and(|t| *t <= minute_ago) {
            entry.failures.pop_front();
        }
        entry.failures.push_back(now);
        let mut started = None;
        if entry.failures.len() >= FAILURES_PER_MINUTE {
            entry.failures.clear();
            let factor = 1_i64 << entry.lockouts.min(4);
            let lockout = FIRST_LOCKOUT
                .checked_mul(i32::try_from(factor).unwrap_or(i32::MAX))
                .unwrap_or(MAX_LOCKOUT)
                .min(MAX_LOCKOUT);
            entry.lockouts += 1;
            entry.locked_until = now.checked_add(lockout).ok();
            started = Some(lockout);
        }
        started
    }

    /// Forgets `ip` after a successful login.
    pub fn succeeded(&mut self, ip: K) {
        self.entries.remove(&ip);
    }
}

#[cfg(test)]
mod tests {
    use jiff::Timestamp;

    use super::{FAILURES_PER_MINUTE, LoginLimiter};

    #[test]
    fn a_username_locks_after_five_failures_from_any_address() {
        let mut by_name: LoginLimiter<String> = LoginLimiter::default();
        let now = Timestamp::UNIX_EPOCH;
        for _ in 0..FAILURES_PER_MINUTE {
            by_name.failed("admin".to_owned(), now);
        }
        assert!(by_name.locked("admin".to_owned(), now).is_some());
        assert!(by_name.locked("other".to_owned(), now).is_none());
        by_name.succeeded("admin".to_owned());
        assert!(by_name.locked("admin".to_owned(), now).is_none());
    }
}
