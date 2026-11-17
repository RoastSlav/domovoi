//! The closed plan-binding discriminator exposed by the action API.

use domovoi_core::PlanFamily;
use serde::Serialize;

/// Which plan-hash family an action's displayed hash belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum PlanBinding {
    /// The legacy binding.
    V1,
    /// The current binding.
    V2,
}

impl From<PlanFamily> for PlanBinding {
    fn from(family: PlanFamily) -> Self {
        match family {
            PlanFamily::V1 => Self::V1,
            PlanFamily::V2 => Self::V2,
        }
    }
}
