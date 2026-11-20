//! Typed identifiers: UUIDv7 newtypes so ids of different entities cannot be mixed up.

use std::fmt;
use std::str::FromStr;

use uuid::Uuid;

macro_rules! define_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(
            Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash,
            serde::Serialize, serde::Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name(Uuid);

        impl $name {
            /// Generates a fresh, time-ordered (UUIDv7) identifier.
            #[must_use]
            pub fn new() -> Self {
                Self(Uuid::now_v7())
            }

            /// Wraps an existing UUID, for example one read from storage.
            #[must_use]
            pub const fn from_uuid(uuid: Uuid) -> Self {
                Self(uuid)
            }

            /// Returns the underlying UUID.
            #[must_use]
            pub const fn as_uuid(&self) -> Uuid {
                self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(f)
            }
        }

        impl FromStr for $name {
            type Err = uuid::Error;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                Uuid::from_str(s).map(Self)
            }
        }
    };
}

define_id!(
    /// Identifies a host.
    HostId
);
define_id!(
    /// Identifies an enrolled agent.
    AgentId
);
define_id!(
    /// Identifies an incident.
    IncidentId
);
define_id!(
    /// Identifies a single signal observation.
    SignalId
);
define_id!(
    /// Identifies a proposed or executed action.
    ActionId
);
define_id!(
    /// Identifies one authenticated hub-agent connection.
    ///
    /// It is unique per connection and is not the agent's journal epoch: a reconnection may
    /// keep the journal while replacing the session.
    AgentSessionId
);

define_id!(
    /// The opaque idempotency key of one execution of an action.
    ///
    /// A legacy action's execution is keyed by its [`ActionId`]; an operation's execution
    /// is keyed by the id of its Execution [`StepId`]. An id never names both.
    ExecutionId
);

impl ExecutionId {
    /// The execution identity of `action`. At M1 the idempotency key is the action id.
    #[must_use]
    pub fn from_action(action: ActionId) -> Self {
        Self(action.as_uuid())
    }

    /// The action this execution belongs to, for the M1 dispatch path.
    #[must_use]
    pub fn as_action(self) -> ActionId {
        ActionId::from_uuid(self.as_uuid())
    }

    /// The execution identity of an operation's Execution `step`: the step id's UUID.
    #[must_use]
    pub fn from_step(step: StepId) -> Self {
        Self(step.as_uuid())
    }

    /// The Execution step this identity belongs to, for operation executions.
    #[must_use]
    pub fn as_step(self) -> StepId {
        StepId::from_uuid(self.as_uuid())
    }
}

define_id!(
    /// Identifies a human user.
    UserId
);
define_id!(
    /// Identifies a note attached to an incident.
    NoteId
);
define_id!(
    /// Identifies a hub-to-agent request.
    RequestId
);
define_id!(
    /// Identifies an ack rule.
    AckRuleId
);
define_id!(
    /// Identifies an advisory.
    AdvisoryId
);
define_id!(
    /// Identifies an operation: one durable piece of work on hosts.
    OperationId
);
define_id!(
    /// Identifies one immutable step of an operation.
    StepId
);
define_id!(
    /// Identifies a pending effect in the work outbox.
    EffectId
);
define_id!(
    /// Identifies one policy version, draft or published.
    PolicyVersionId
);

define_id!(
    /// Identifies one stored knowledge fact row. It is not `capability::FactId`, which names a
    /// safety-critical fact the policy evaluator reads.
    FactRecordId
);
define_id!(
    /// Identifies a stored relation between two subjects.
    RelationId
);
define_id!(
    /// Identifies a subject the owner invented, such as a VPN or a NAS pool.
    EntityId
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_round_trip_through_display_and_from_str() {
        let id = HostId::new();
        let parsed: HostId = id.to_string().parse().unwrap();
        assert_eq!(id, parsed);
    }

    #[test]
    fn execution_id_from_step_keeps_the_uuid() {
        let step = StepId::new();
        let execution = ExecutionId::from_step(step);
        assert_eq!(execution.as_uuid(), step.as_uuid());
        assert_eq!(execution.as_step(), step);
    }

    #[test]
    fn new_ids_are_time_ordered_v7() {
        let id = IncidentId::new();
        assert_eq!(id.as_uuid().get_version_num(), 7);
    }
}
