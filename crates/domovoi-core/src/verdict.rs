//! The triage outcome attached to an incident.

/// What triage concluded about an incident.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Verdict {
    /// A human should act.
    ActionNeeded,
    /// Keep an eye on it; no action yet.
    Monitor,
    /// Known and acceptable.
    Expected,
    /// Already fixed or fixed itself.
    Fixed,
}

impl Verdict {
    /// The uppercase label shown on cards and in digests.
    #[must_use]
    pub const fn label(&self) -> &'static str {
        match self {
            Self::ActionNeeded => "ACTION NEEDED",
            Self::Monitor => "MONITOR",
            Self::Expected => "EXPECTED",
            Self::Fixed => "FIXED",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_and_serde_names() {
        assert_eq!(Verdict::ActionNeeded.label(), "ACTION NEEDED");
        assert_eq!(
            serde_json::to_string(&Verdict::ActionNeeded).unwrap(),
            "\"ACTION_NEEDED\""
        );
    }
}
