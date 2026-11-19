//! The effect handlers of the operation runner.
//!
//! This is the one place handlers are registered, kept apart from the service so the
//! handlers can be added without touching how the writer or the runner start.

use std::sync::Arc;

use crate::effects::EffectRunner;
use crate::operations::writer::OpShared;

/// Registers a handler on `runner` for each effect kind the operation service claims.
pub(super) fn register(_runner: &mut EffectRunner, _shared: &Arc<OpShared>) {}
