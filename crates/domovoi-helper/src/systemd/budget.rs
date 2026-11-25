//! The helper-side restart budget, kept in the state directory so it survives the helper's
//! one-process-per-connection life.
//!
//! The `restarts` file holds a ring of accepted restarts per unit id and the helper-wide count
//! of accepted write operations. Both are read and written under the state lock. A file that
//! cannot be read or parsed refuses the restart: a budget that cannot be proven is spent.

use std::path::Path;

use domovoi_core::unit::UnitRefusal;
use jiff::{SignedDuration, Timestamp};

use crate::state::{LockGuard, WriteFaults, write_atomic_locked};

/// The file's name inside the helper's state directory.
pub const RESTARTS_FILE: &str = "restarts";

/// How long a helper-wide write is remembered for `[limits] mutations_per_hour`.
const HOUR: SignedDuration = SignedDuration::from_hours(1);

/// The parsed file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Budget {
    restarts: Vec<(String, i64)>,
    writes: Vec<i64>,
}

impl Budget {
    /// Reads the file in `dir`; a missing file is an empty budget, an unreadable or malformed
    /// one is `read_failed`.
    pub fn load(dir: &Path) -> Result<Self, UnitRefusal> {
        let text = match std::fs::read_to_string(dir.join(RESTARTS_FILE)) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(_) => return Err(UnitRefusal::ReadFailed),
        };
        let mut budget = Self::default();
        for line in text.lines().filter(|l| !l.is_empty()) {
            let parts: Vec<&str> = line.split('\t').collect();
            match parts.as_slice() {
                ["r", id, ms] => budget.restarts.push((
                    (*id).to_owned(),
                    ms.parse().map_err(|_| UnitRefusal::ReadFailed)?,
                )),
                ["w", ms] => budget
                    .writes
                    .push(ms.parse().map_err(|_| UnitRefusal::ReadFailed)?),
                _ => return Err(UnitRefusal::ReadFailed),
            }
        }
        Ok(budget)
    }

    fn encode(&self) -> String {
        let mut out = String::new();
        for (id, ms) in &self.restarts {
            out.push_str(&format!("r\t{id}\t{ms}\n"));
        }
        for ms in &self.writes {
            out.push_str(&format!("w\t{ms}\n"));
        }
        out
    }

    /// Whether another write fits: the helper-wide count inside the last hour is below
    /// `mutations_per_hour` (`0` accepts none), and the unit's own count inside its start-limit
    /// interval is below `burst - 1`. A unit with the limit disabled has no per-unit budget.
    pub fn check(
        &self,
        id: &str,
        now: Timestamp,
        mutations_per_hour: u32,
        interval_usec: u64,
        burst: u32,
    ) -> Result<(), UnitRefusal> {
        let since = |window: SignedDuration| now.as_millisecond() - window.as_millis() as i64;
        let recent_writes = self.writes.iter().filter(|ms| **ms > since(HOUR)).count();
        if recent_writes >= mutations_per_hour as usize {
            return Err(UnitRefusal::RestartBudgetExhausted);
        }
        if interval_usec > 0 && burst > 0 {
            let interval =
                SignedDuration::from_micros(i64::try_from(interval_usec).unwrap_or(i64::MAX));
            let recent = self
                .restarts
                .iter()
                .filter(|(i, ms)| i == id && *ms > since(interval))
                .count();
            if recent >= (burst as usize).saturating_sub(1) {
                return Err(UnitRefusal::RestartBudgetExhausted);
            }
        }
        Ok(())
    }

    /// Records one accepted restart of `id` and prunes what no limit can still count.
    pub fn record(&mut self, id: &str, now: Timestamp, interval_usec: u64) {
        let ms = now.as_millisecond();
        let keep = SignedDuration::from_micros(i64::try_from(interval_usec).unwrap_or(i64::MAX))
            .max(HOUR)
            .as_millis() as i64;
        self.restarts.retain(|(_, at)| *at > ms - keep);
        self.writes.retain(|at| *at > ms - HOUR.as_millis() as i64);
        self.restarts.push((id.to_owned(), ms));
        self.writes.push(ms);
    }

    /// Writes the file atomically while `guard` proves the state lock is held.
    pub fn store(&self, guard: &LockGuard<'_>, dir: &Path) -> Result<(), UnitRefusal> {
        write_atomic_locked(
            guard,
            dir,
            RESTARTS_FILE,
            self.encode().as_bytes(),
            0o600,
            WriteFaults::default(),
        )
        .map_err(|_| UnitRefusal::ReadFailed)
    }
}
