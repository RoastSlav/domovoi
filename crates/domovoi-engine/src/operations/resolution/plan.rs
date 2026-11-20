//! Pure planning of summary revisions: what a draft writes and what an edit writes.

use domovoi_core::experience::{
    MAX_NARRATIVE_CHARS, SummaryField, SummaryRevision, SummarySource, code_fields, outcome_tags,
};
use domovoi_core::ports::Redactor;
use domovoi_core::{Actor, OperationSnapshot};
use serde_json::Value;

use super::narrative::Narrative;
use super::{EditSummaryField, SummaryError};

/// The actor of revisions the service writes on its own.
const SYSTEM_ACTOR: &str = "system:operations";

/// One revision the writer should append.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PlannedRevision {
    /// The field.
    pub(crate) field: SummaryField,
    /// The newest revision the plan saw, 0 when the field had none.
    pub(crate) expected: u32,
    /// The canonical, redacted value.
    pub(crate) value: Value,
    /// Who writes it.
    pub(crate) source: SummarySource,
    /// The actor in string form.
    pub(crate) actor: String,
    /// The outcome tags, on a code write.
    pub(crate) tags: Option<Vec<String>>,
}

fn newest(existing: &[SummaryRevision], field: SummaryField) -> Option<&SummaryRevision> {
    existing
        .iter()
        .filter(|rev| rev.field == field)
        .max_by_key(|rev| rev.revision)
}

/// Cuts to the narrative limit on a character boundary and drops control characters other
/// than a newline.
fn tidy(text: &str) -> String {
    let clean: String = text
        .trim()
        .chars()
        .filter(|c| !c.is_control() || *c == '\n')
        .collect();
    clean.trim().chars().take(MAX_NARRATIVE_CHARS).collect()
}

fn redact_value(value: Value, redactor: &dyn Redactor) -> Value {
    match value {
        Value::String(text) => Value::String(redactor.redact(&text)),
        Value::Array(items) => Value::Array(
            items
                .into_iter()
                .map(|v| redact_value(v, redactor))
                .collect(),
        ),
        Value::Object(map) => Value::Object(
            map.into_iter()
                .map(|(key, v)| (key, redact_value(v, redactor)))
                .collect(),
        ),
        other => other,
    }
}

/// What a draft writes for a finished operation: each code field whose value is new or
/// changed, and each narrative field that has no revision yet and has a draft text. Empty
/// when the operation is not finished or nothing changed.
pub(crate) fn plan_draft(
    snapshot: &OperationSnapshot,
    existing: &[SummaryRevision],
    narrative: Option<&Narrative>,
    redactor: &dyn Redactor,
) -> Vec<PlannedRevision> {
    let Some(code) = code_fields(snapshot) else {
        return Vec::new();
    };
    let tags: Vec<String> = outcome_tags(snapshot)
        .into_iter()
        .map(str::to_owned)
        .collect();
    let mut planned = Vec::new();
    for (field, value) in code.values() {
        let value = redact_value(value, redactor);
        let current = newest(existing, field);
        if current.is_some_and(|rev| rev.value == value) {
            continue;
        }
        planned.push(PlannedRevision {
            field,
            expected: current.map_or(0, |rev| rev.revision),
            value,
            source: SummarySource::Code,
            actor: SYSTEM_ACTOR.to_owned(),
            tags: Some(tags.clone()),
        });
    }
    let drafts = narrative.map(|n| [&n.problem, &n.cause, &n.fix]);
    let fields = [
        SummaryField::Problem,
        SummaryField::Cause,
        SummaryField::Fix,
    ];
    for (field, text) in fields.into_iter().zip(drafts.into_iter().flatten()) {
        let Some(text) = text else { continue };
        if newest(existing, field).is_some() {
            continue;
        }
        let text = tidy(&redactor.redact(&tidy(text)));
        if text.is_empty() {
            continue;
        }
        planned.push(PlannedRevision {
            field,
            expected: 0,
            value: Value::String(text),
            source: SummarySource::Model,
            actor: SYSTEM_ACTOR.to_owned(),
            tags: None,
        });
    }
    planned
}

/// What an edit becomes.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum EditPlan {
    /// The text is already held by a revision the owner wrote.
    Unchanged(SummaryRevision),
    /// Append this revision.
    Write(PlannedRevision),
}

/// Applies the edit rules in order; the first failure is returned and nothing is written.
/// `terminal` says whether the operation has finished.
pub(crate) fn plan_edit(
    terminal: bool,
    existing: &[SummaryRevision],
    edit: &EditSummaryField,
    actor: &Actor,
    redactor: &dyn Redactor,
) -> Result<EditPlan, SummaryError> {
    if edit.field.is_code_owned() {
        return Err(SummaryError::CodeOwned(edit.field));
    }
    if !terminal {
        return Err(SummaryError::NotTerminal);
    }
    let text = edit.value.trim();
    if text.is_empty() {
        return Err(SummaryError::Empty);
    }
    if text.chars().count() > MAX_NARRATIVE_CHARS {
        return Err(SummaryError::TooLong {
            max: MAX_NARRATIVE_CHARS,
        });
    }
    if text.chars().any(|c| c.is_control() && c != '\n') {
        return Err(SummaryError::BadText);
    }
    let text = redactor.redact(text);
    let current = newest(existing, edit.field);
    let actual = current.map_or(0, |rev| rev.revision);
    if edit.expected_revision != actual {
        return Err(SummaryError::Stale {
            expected: edit.expected_revision,
            actual,
        });
    }
    let value = Value::String(text);
    if let Some(rev) = current
        && rev.value == value
        && rev.source == SummarySource::User
    {
        return Ok(EditPlan::Unchanged(rev.clone()));
    }
    Ok(EditPlan::Write(PlannedRevision {
        field: edit.field,
        expected: actual,
        value,
        source: SummarySource::User,
        actor: actor.to_string(),
        tags: None,
    }))
}
