//! The plan hash: what exactly an approval binds (contract, implementation, target, arguments
//! and execution).
//!
//! The encoding is length-prefixed so a value may contain `\n`, `:` or `@` without making the
//! byte string ambiguous. The domain line (`plan:v2`) keeps this hash apart from the agent
//! check hash (`plan:v1`), so one can never stand in for the other.

use crate::ScriptDigest;
use crate::host::{KeyFingerprint, host_target_text};
use crate::id::{ExecutionId, HostId};
use crate::incident::Incident;
use crate::sha256::sha256_newtype;
use crate::signal::{CheckKey, TargetKey};
use crate::skill::{ManifestHash, SkillName, ValidatedArgs};

use super::approval::PlanHash;

/// The capability contract a plan names: an id and a version.
///
/// The skill adapter supplies the skill name as the id and version `1`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanContract {
    id: String,
    version: u32,
}

impl PlanContract {
    /// Validates `id` against `^[a-z][a-z0-9_.-]{0,63}$` and records `version`.
    ///
    /// An id outside that grammar is refused here, so canonical input has one home.
    pub fn new(id: &str, version: u32) -> Result<Self, PlanError> {
        let bytes = id.as_bytes();
        let first_ok = bytes.first().is_some_and(u8::is_ascii_lowercase);
        let rest_ok = bytes.get(1..).unwrap_or_default().iter().all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'_' | b'.' | b'-')
        });
        if bytes.is_empty() || bytes.len() > 64 || !first_ok || !rest_ok {
            return Err(PlanError::ContractId { id: id.to_owned() });
        }
        Ok(Self {
            id: id.to_owned(),
            version,
        })
    }

    /// The skill adapter: the skill name is the contract id and the version is supplied.
    pub fn skill(skill: &SkillName, version: u32) -> Result<Self, PlanError> {
        Self::new(skill.as_str(), version)
    }

    /// The contract id.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// The contract version.
    #[must_use]
    pub fn version(&self) -> u32 {
        self.version
    }

    fn canonical(&self) -> String {
        format!("{}@{}", self.id, self.version)
    }
}

/// The canonical target a plan names: the host, its enrolled agent key and the incident
/// subject.
///
/// Every field is already a validated type, so [`PlanTarget::new`] is infallible and the
/// canonical text is produced by construction rather than parsed back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanTarget {
    host: HostId,
    host_key: KeyFingerprint,
    check: CheckKey,
    target: Option<TargetKey>,
}

impl PlanTarget {
    /// Builds a target from validated values. There is deliberately no `&str` form.
    #[must_use]
    pub fn new(
        host: HostId,
        host_key: KeyFingerprint,
        check: CheckKey,
        target: Option<TargetKey>,
    ) -> Self {
        Self {
            host,
            host_key,
            check,
            target,
        }
    }

    /// Takes the host and subject from `incident` and the key from `host_key`.
    ///
    /// There is no separate host argument, so a plan cannot be built for a host other than
    /// the incident's own.
    #[must_use]
    pub fn of_incident(incident: &Incident, host_key: KeyFingerprint) -> Self {
        Self::new(
            incident.host(),
            host_key,
            incident.check().clone(),
            incident.target().cloned(),
        )
    }

    /// The host this target names.
    #[must_use]
    pub fn host(&self) -> HostId {
        self.host
    }

    /// The canonical target text.
    ///
    /// `target:absent` keeps "no target" distinct from an empty target; the fingerprint is
    /// always present because a plan cannot name a host without an enrolled key.
    #[must_use]
    pub fn canonical(&self) -> String {
        let subject = match &self.target {
            Some(target) => format!("target:{}:{}", target.as_str().len(), target),
            None => "target:absent".to_owned(),
        };
        format!(
            "{}\ncheck:{}:{}\n{subject}",
            host_target_text(self.host, &self.host_key),
            self.check.as_str().len(),
            self.check,
        )
    }
}

sha256_newtype!(
    /// What the implementation of a capability is: the manifest hash and the entry script's
    /// digest, both always present.
    ///
    /// It does not bind transitive dependencies, interpreters, sourced scripts, other files
    /// in the skill folder or external programs.
    ImplHash
);

impl ImplHash {
    /// Hashes the manifest and the required entry-script digest.
    ///
    /// There is no manifest-only constructor and no absent-entry form.
    #[must_use]
    pub fn from_parts(manifest: ManifestHash, entry: ScriptDigest) -> Self {
        let text = format!("impl:v1\nmanifest:{manifest}\nentry:{entry}\n");
        Self::digest(text.as_bytes())
    }
}

impl PlanHash {
    /// The plan identity over exactly five inputs, in this order: contract (id and
    /// version), implementation, target, arguments and execution id.
    ///
    /// Not included: the incident id, the proposer, risk, expiry, approval time, the short
    /// form, the entry path, `run_as` or any UI text.
    #[must_use]
    pub fn compute(
        contract: &PlanContract,
        implementation: ImplHash,
        target: &PlanTarget,
        args: &ValidatedArgs,
        execution: ExecutionId,
    ) -> Self {
        let contract = contract.canonical();
        let target = target.canonical();
        let args = args.canonical();
        let text = format!(
            "plan:v2\ncontract:{}:{contract}\nimpl:{implementation}\ntarget:{}:{target}\n\
             args:{}:{args}\nexecution:{execution}\n",
            contract.len(),
            target.len(),
            args.len(),
        );
        Self::digest(text.as_bytes())
    }
}

/// Why a plan cannot be built.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PlanError {
    /// A capability contract id is outside its canonical grammar.
    #[error("`{id}` is not a canonical capability contract id")]
    ContractId {
        /// The rejected id.
        id: String,
    },
}
