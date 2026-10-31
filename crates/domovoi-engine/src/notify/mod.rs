//! The notification policy: what gets said about incidents, and when. Pure.
//!
//! - A new incident posts one card with a thread; repeats become one thread line at most
//!   every 30 minutes, with a count of those skipped.
//! - Open incidents are mentioned again every 24 h (warning) or 4 h (critical).
//! - During quiet hours only critical alerts go out. The rest are held and released as
//!   one message when quiet hours end; ones that resolved meanwhile are marked so.
//! - Suppressed events (maintenance, ack rules) never notify.

mod plan;
mod quiet;
mod settings;
mod text;
mod tick;

pub use plan::{NotifyAction, Plan, card_changed, held_line, plan};
pub use quiet::{in_quiet_hours, quiet_ends_at};
pub use settings::{NotifySettings, QuietHours};
pub use text::{human, ordinal};
pub use tick::{TickPlan, tick};
