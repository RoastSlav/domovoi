//! Tests for the review of what an Operator answer states.

use domovoi_core::ReadToolCall;

use super::{CONTRADICTED_LEAD, DRAFT_HEADING, UNVERIFIED_LEAD, before_claim, review};
use crate::triage::meta::EvidenceRecord;
use crate::triage::tools::EvidenceClass;

const CLAIM: &str = "The unit failed because the `ip` command from iproute2 was not installed.";

fn binary(name: &str, output: &str) -> EvidenceRecord {
    EvidenceRecord {
        call: ReadToolCall::HostBinary {
            name: name.to_owned(),
            package: None,
        },
        class: EvidenceClass::Observed,
        summary: output.to_owned(),
        output: output.to_owned(),
    }
}

fn shown(answer: &str, reads: &[EvidenceRecord], read_host: bool) -> String {
    review(answer, reads, read_host).apply(answer.to_owned())
}

#[test]
fn an_unchecked_absence_claim_never_leads_the_answer() {
    let text = shown(&format!("{CLAIM} The journal shows exit 1."), &[], true);
    assert!(text.starts_with(UNVERIFIED_LEAD), "{text}");
    let (before, after) = text.split_once(DRAFT_HEADING).unwrap();
    assert!(before.contains("The journal shows exit 1."), "{text}");
    assert!(!before.contains("was not installed"), "{text}");
    assert!(after.contains(CLAIM), "{text}");
}

#[test]
fn a_check_of_another_program_does_not_back_the_claim() {
    let reads = [binary("nginx", "file `nginx`: not found in /usr/bin.")];
    let text = shown(CLAIM, &reads, true);
    assert!(text.starts_with(UNVERIFIED_LEAD), "{text}");
}

#[test]
fn a_claim_the_host_contradicts_is_said_to_be_wrong() {
    let reads = [binary("ip", "file `ip`: found at /usr/sbin/ip.")];
    let text = shown(CLAIM, &reads, true);
    assert!(
        text.starts_with(&format!("{CONTRADICTED_LEAD} `ip`.")),
        "{text}"
    );
}

#[test]
fn a_claim_that_host_binary_shows_stays_as_written() {
    let reads = [binary("ip", "file `ip`: not found in /usr/sbin, /usr/bin.")];
    assert_eq!(shown(CLAIM, &reads, true), CLAIM);
}

#[test]
fn a_cause_without_any_read_of_the_host_is_not_stated_first() {
    let answer = "It stopped because the disk was full.";
    assert!(shown(answer, &[], false).starts_with(UNVERIFIED_LEAD));
    assert_eq!(shown(answer, &[], true), answer);
}

#[test]
fn doubt_questions_and_quoted_output_are_not_claims() {
    for answer in [
        "I do not know whether `ip` is missing.",
        "Is iproute2 not installed?",
        "The log says:\n```\nip: command not found\n```",
        "The unit failed at 10:00.",
    ] {
        assert_eq!(shown(answer, &[], true), answer);
    }
}

#[test]
fn a_live_view_stops_before_the_claim() {
    let text = "Looking.\nIt is down. iproute2 is not installed. Install it.";
    assert_eq!(before_claim(text), "Looking.\nIt is down. ");
    assert_eq!(before_claim("All fine."), "All fine.");
}
