//! The closed set of things Domovoi can know, with the shape, subject kinds and freshness of
//! each.

use jiff::SignedDuration;
use serde_json::{Value, json};

use super::subject::SubjectRef;
use super::value::{ValueError, cidr_object, object, text, utc_timestamp};

/// How many live facts one subject may hold for a predicate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cardinality {
    /// One value at a time; different values from different sources are a dispute.
    One,
    /// Any number of distinct values.
    Many,
}

/// What a fact says about its subject.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Predicate {
    /// What the subject is for.
    Role,
    /// How much the subject matters (`critical` or `warning`).
    Criticality,
    /// A free-form note from the owner.
    OwnerNote,
    /// A subject this one depends on.
    DependsOn,
    /// A network range from which the host is managed.
    ManagementPath,
    /// A time window in which work is expected.
    MaintenanceWindow,
    /// Where the subject is backed up.
    BackupLocation,
    /// The subject must never be restarted automatically.
    NeverRestart,
    /// A network range the owner trusts.
    TrustedCidr,
    /// A description of the machine a host runs on.
    HostPlatform,
}

const DAY: i64 = 24 * 3600;

impl Predicate {
    /// Every predicate.
    pub const ALL: [Predicate; 10] = [
        Self::Role,
        Self::Criticality,
        Self::OwnerNote,
        Self::DependsOn,
        Self::ManagementPath,
        Self::MaintenanceWindow,
        Self::BackupLocation,
        Self::NeverRestart,
        Self::TrustedCidr,
        Self::HostPlatform,
    ];

    /// The stable name used in storage and on the wire.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Role => "role",
            Self::Criticality => "criticality",
            Self::OwnerNote => "owner_note",
            Self::DependsOn => "depends_on",
            Self::ManagementPath => "management_path",
            Self::MaintenanceWindow => "maintenance_window",
            Self::BackupLocation => "backup_location",
            Self::NeverRestart => "never_restart",
            Self::TrustedCidr => "trusted_cidr",
            Self::HostPlatform => "host_platform",
        }
    }

    /// Reads [`Predicate::as_str`] back; `None` for anything else.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.as_str() == text)
    }

    /// Whether a subject holds one value or many.
    #[must_use]
    pub const fn cardinality(self) -> Cardinality {
        match self {
            Self::Role | Self::Criticality | Self::NeverRestart | Self::HostPlatform => {
                Cardinality::One
            }
            _ => Cardinality::Many,
        }
    }

    /// Predicates that policy hard limits may read: they matter only while current and
    /// owner-sourced.
    #[must_use]
    pub const fn is_safety_critical(self) -> bool {
        matches!(self, Self::ManagementPath | Self::TrustedCidr)
    }

    /// How long a fact stays fresh without being observed or verified again.
    #[must_use]
    pub fn staleness(self) -> SignedDuration {
        let days = match self {
            Self::Role | Self::Criticality | Self::DependsOn => 180,
            Self::OwnerNote | Self::NeverRestart => 365,
            Self::ManagementPath | Self::TrustedCidr => 30,
            Self::MaintenanceWindow => 7,
            Self::BackupLocation => 90,
            Self::HostPlatform => 14,
        };
        SignedDuration::from_secs(days * DAY)
    }

    /// Whether the predicate can describe `subject`.
    #[must_use]
    pub fn allows_subject(self, subject: &SubjectRef) -> bool {
        use SubjectRef::{Container, Entity, Host, Unit};
        match self {
            Self::Role | Self::Criticality | Self::OwnerNote | Self::DependsOn => true,
            Self::MaintenanceWindow => true,
            Self::BackupLocation => matches!(
                subject,
                Host(_) | Container { .. } | Unit { .. } | Entity(_)
            ),
            Self::ManagementPath | Self::TrustedCidr | Self::HostPlatform => {
                matches!(subject, Host(_))
            }
            Self::NeverRestart => matches!(subject, Container { .. } | Unit { .. }),
        }
    }

    /// Checks `value` for this predicate and subject and returns it in canonical form.
    pub fn validate(self, subject: &SubjectRef, value: &Value) -> Result<Value, ValueError> {
        if !self.allows_subject(subject) {
            return Err(ValueError::SubjectNotAllowed);
        }
        match self {
            Self::Role => Ok(Value::String(text(value, 1, 80)?)),
            Self::Criticality => match value.as_str() {
                Some(word @ ("critical" | "warning")) => Ok(Value::String(word.to_owned())),
                Some(_) => Err(ValueError::OutOfRange),
                None => Err(ValueError::WrongType),
            },
            Self::OwnerNote => Ok(Value::String(text(value, 1, 1000)?)),
            Self::BackupLocation => Ok(Value::String(text(value, 1, 200)?)),
            Self::DependsOn => {
                let raw = value.as_str().ok_or(ValueError::WrongType)?;
                let target: SubjectRef = raw.parse().map_err(|_| ValueError::OutOfRange)?;
                if &target == subject {
                    return Err(ValueError::OutOfRange);
                }
                Ok(Value::String(target.to_string()))
            }
            Self::ManagementPath | Self::TrustedCidr => cidr_object(value),
            Self::NeverRestart => match value.as_bool() {
                Some(true) => Ok(Value::Bool(true)),
                Some(false) => Err(ValueError::OutOfRange),
                None => Err(ValueError::WrongType),
            },
            Self::MaintenanceWindow => maintenance_window(value),
            Self::HostPlatform => host_platform(value),
        }
    }
}

impl serde::Serialize for Predicate {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> serde::Deserialize<'de> for Predicate {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::parse(&text).ok_or_else(|| serde::de::Error::custom("unknown predicate"))
    }
}

/// The end of a maintenance window value, for the fact's forced `valid_until`.
#[must_use]
pub fn window_end(value: &Value) -> Option<jiff::Timestamp> {
    value.get("ends_at")?.as_str()?.parse().ok()
}

fn maintenance_window(value: &Value) -> Result<Value, ValueError> {
    let map = object(value, &["starts_at", "ends_at"], &["note"])?;
    let starts = utc_timestamp(&map["starts_at"])?;
    let ends = utc_timestamp(&map["ends_at"])?;
    if ends <= starts {
        return Err(ValueError::OutOfRange);
    }
    let mut out = json!({ "starts_at": starts.to_string(), "ends_at": ends.to_string() });
    if let Some(note) = map.get("note") {
        out["note"] = Value::String(text(note, 1, 200)?);
    }
    Ok(out)
}

fn host_platform(value: &Value) -> Result<Value, ValueError> {
    let map = object(
        value,
        &[
            "hostname",
            "os",
            "kernel",
            "arch",
            "cpu_count",
            "mem_total_bytes",
        ],
        &[],
    )?;
    let mut out = serde_json::Map::new();
    for key in ["hostname", "os", "kernel", "arch"] {
        out.insert(key.to_owned(), Value::String(text(&map[key], 1, 200)?));
    }
    let cpus = map["cpu_count"].as_u64().ok_or(ValueError::WrongType)?;
    if u32::try_from(cpus).is_err() {
        return Err(ValueError::OutOfRange);
    }
    out.insert("cpu_count".to_owned(), json!(cpus));
    let mem = map["mem_total_bytes"]
        .as_u64()
        .ok_or(ValueError::WrongType)?;
    out.insert("mem_total_bytes".to_owned(), json!(mem));
    Ok(Value::Object(out))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::id::{EntityId, HostId};

    fn host() -> SubjectRef {
        SubjectRef::Host(HostId::new())
    }

    fn container() -> SubjectRef {
        SubjectRef::Container {
            host: HostId::new(),
            name: "plex".into(),
        }
    }

    #[test]
    fn every_predicate_validates_its_documented_values_and_rejects_others() {
        use Predicate::*;
        let entity = SubjectRef::Entity(EntityId::new());
        let other = host().to_string();
        let ok: Vec<(Predicate, SubjectRef, Value, Value)> = vec![
            (
                Role,
                host(),
                json!("  media server "),
                json!("media server"),
            ),
            (
                Criticality,
                container(),
                json!("critical"),
                json!("critical"),
            ),
            (OwnerNote, entity.clone(), json!(" hi "), json!("hi")),
            (DependsOn, host(), json!(other), json!(other)),
            (
                ManagementPath,
                host(),
                json!({"cidr": "192.168.1.7/24"}),
                json!({"cidr": "192.168.1.0/24"}),
            ),
            (
                TrustedCidr,
                host(),
                json!({"cidr": "10.0.0.5"}),
                json!({"cidr": "10.0.0.5/32"}),
            ),
            (
                BackupLocation,
                container(),
                json!("nas:/b"),
                json!("nas:/b"),
            ),
            (NeverRestart, container(), json!(true), json!(true)),
            (
                MaintenanceWindow,
                host(),
                json!({"starts_at": "2026-01-01T00:00:00Z", "ends_at": "2026-01-01T02:00:00Z", "note": " x "}),
                json!({"starts_at": "2026-01-01T00:00:00Z", "ends_at": "2026-01-01T02:00:00Z", "note": "x"}),
            ),
            (
                HostPlatform,
                host(),
                json!({"hostname": "n", "os": "d", "kernel": "k", "arch": "x", "cpu_count": 4, "mem_total_bytes": 8}),
                json!({"hostname": "n", "os": "d", "kernel": "k", "arch": "x", "cpu_count": 4, "mem_total_bytes": 8}),
            ),
        ];
        for (predicate, subject, value, canonical) in ok {
            assert_eq!(
                predicate.validate(&subject, &value).unwrap(),
                canonical,
                "{predicate:?}"
            );
        }
        let me = host();
        let bad: Vec<(Predicate, SubjectRef, Value, ValueError)> = vec![
            (Role, host(), json!(3), ValueError::WrongType),
            (Role, host(), json!("  "), ValueError::OutOfRange),
            (Role, host(), json!("x".repeat(81)), ValueError::OutOfRange),
            (Criticality, host(), json!("high"), ValueError::OutOfRange),
            (
                DependsOn,
                me.clone(),
                json!(me.to_string()),
                ValueError::OutOfRange,
            ),
            (DependsOn, host(), json!("nope"), ValueError::OutOfRange),
            (
                ManagementPath,
                container(),
                json!({"cidr": "10.0.0.0/8"}),
                ValueError::SubjectNotAllowed,
            ),
            (
                ManagementPath,
                host(),
                json!({"cidr": "10.0.0.0/8", "x": 1}),
                ValueError::UnknownKey,
            ),
            (ManagementPath, host(), json!({}), ValueError::MissingKey),
            (
                ManagementPath,
                host(),
                json!({"cidr": "10.0.0.0/33"}),
                ValueError::BadCidr,
            ),
            (
                ManagementPath,
                host(),
                json!({"cidr": "0.0.0.0/0"}),
                ValueError::CoversEverything,
            ),
            (
                TrustedCidr,
                host(),
                json!({"cidr": "::/0"}),
                ValueError::CoversEverything,
            ),
            (
                NeverRestart,
                host(),
                json!(true),
                ValueError::SubjectNotAllowed,
            ),
            (
                NeverRestart,
                container(),
                json!(false),
                ValueError::OutOfRange,
            ),
            (
                MaintenanceWindow,
                host(),
                json!({"starts_at": "2026-01-01T02:00:00Z", "ends_at": "2026-01-01T02:00:00Z"}),
                ValueError::OutOfRange,
            ),
            (
                MaintenanceWindow,
                host(),
                json!({"starts_at": "soon", "ends_at": "2026-01-01T02:00:00Z"}),
                ValueError::BadTimestamp,
            ),
            (
                HostPlatform,
                host(),
                json!({"hostname": "n", "os": "d", "kernel": "k", "arch": "x", "cpu_count": 4}),
                ValueError::MissingKey,
            ),
        ];
        for (predicate, subject, value, error) in bad {
            assert_eq!(
                predicate.validate(&subject, &value),
                Err(error),
                "{predicate:?} {value}"
            );
        }
    }

    #[test]
    fn predicate_names_round_trip() {
        for p in Predicate::ALL {
            assert_eq!(Predicate::parse(p.as_str()), Some(p));
        }
        assert_eq!(Predicate::parse("bogus"), None);
    }
}
