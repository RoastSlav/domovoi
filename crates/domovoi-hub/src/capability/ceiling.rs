//! The agent's ceiling words, read as the hub's own capability types: `hello.ceiling_summary`
//! is a JSON string on the wire (wire 1.2), and the registry needs a `CeilingSummary` it can
//! compare.
//!
//! The read fails closed. A summary that does not deserialize, that names a contract the
//! wire cannot carry, that has a target set outside the `TargetName` grammar, or that
//! carries a negative rate is [`CeilingParse::Malformed`]: the registry then refuses every
//! non-`Read` contract on that host with `NotAllowedOnHost(CeilingAbsent)`, never `Available`
//! and never the permissive default. The hub adds no second ceiling language here.

use domovoi_core::capability::{CeilingSummary, ContractId};
use serde_json::from_str;

/// Maps the agent's `hello.ceiling_summary` (a JSON string, wire 1.2) into the hub's own
/// capability types.
#[must_use]
pub fn parse_ceiling(summary: Option<&str>) -> CeilingParse {
    match summary {
        None => CeilingParse::Absent,
        Some(text) => match text {
            "" => CeilingParse::Absent,
            "null" => CeilingParse::Absent,
            _ => match from_str(text) {
                Err(error) => CeilingParse::Malformed {
                    reason: error.to_string(),
                },
                Ok(ceiling) => CeilingParse::Parsed(ceiling),
            },
        },
    }
}

/// Whether the ceiling covers this contract id (exact ids only, D-18).
#[must_use]
pub fn covers(ceiling: &CeilingSummary, contract: &ContractId) -> bool {
    ceiling
        .entries
        .iter()
        .any(|entry| entry.contract == *contract)
}

/// The result of reading the agent's ceiling words.
#[derive(Debug, Clone, PartialEq)]
pub enum CeilingParse {
    /// No summary was reported: the agent has no ceiling, or predates it.
    Absent,
    /// A summary arrived and is valid.
    Parsed(CeilingSummary),
    /// A summary arrived and is not usable. Never treated as permissive.
    Malformed {
        /// What made it unusable.
        reason: String,
    },
}

#[cfg(test)]
mod tests {
    use domovoi_core::capability::{CeilingSummary, ContractId};

    use super::{CeilingParse, covers, parse_ceiling};

    const TEMPLATE: &str = "{\"enabled\":true,\"entries\":[{\"contract\":\"@C@\",\"targets\":{\"kind\":\"any\"},\"max_per_hour\":@R@,\"args\":{}}]}";
    const TEMPLATE_HASH: &str = "{\"enabled\":true,\"entries\":[{\"contract\":\"@C@\",\"targets\":{\"kind\":\"any\"},\"max_per_hour\":@R@,\"args\":{}}],\"hash\":\"@H@\"}";
    const DISABLED: &str = "{\"enabled\":false,\"entries\":[]}";

    fn id(text: &str) -> ContractId {
        ContractId::parse(text).unwrap()
    }

    #[must_use]
    fn covered_text(contract: &str, rate: &str) -> String {
        TEMPLATE.replace("@C@", contract).replace("@R@", rate)
    }

    #[must_use]
    fn text_of(contract: &str, rate: &str, hash: &str) -> String {
        TEMPLATE_HASH
            .replace("@C@", contract)
            .replace("@R@", rate)
            .replace("@H@", hash)
    }

    #[must_use]
    fn parse_text(text: &str) -> CeilingParse {
        parse_ceiling(Some(text))
    }

    fn summary_of(parsed: CeilingParse) -> Option<CeilingSummary> {
        match parsed {
            CeilingParse::Parsed(summary) => Some(summary),
            CeilingParse::Absent => None,
            CeilingParse::Malformed { reason: _ } => None,
        }
    }

    #[must_use]
    fn malformed(text: &str) -> bool {
        matches!(parse_text(text), CeilingParse::Malformed { reason: _ })
    }

    #[test]
    fn missing_ceiling_words_parse_as_absent() {
        assert_eq!(
            parse_ceiling(None),
            CeilingParse::Absent,
            "no words read as something else"
        );
        assert_eq!(
            parse_text(""),
            CeilingParse::Absent,
            "empty words read as something else"
        );
        assert_eq!(
            parse_text("null"),
            CeilingParse::Absent,
            "the null literal read as something else"
        );
    }

    #[test]
    fn valid_ceiling_summary_covers_its_exact_contract() {
        let Some(ceiling) = summary_of(parse_text(&covered_text("container.restart", "4"))) else {
            panic!("a well-formed ceiling did not parse");
        };
        assert!(
            covers(&ceiling, &id("container.restart")),
            "the exact contract was not covered"
        );
    }

    #[test]
    fn covers_never_matches_by_prefix_or_category() {
        let Some(ceiling) = summary_of(parse_text(&covered_text("container.restart", "4"))) else {
            panic!("a well-formed ceiling did not parse");
        };
        assert!(
            !covers(&ceiling, &id("container.restartx")),
            "a longer id was covered"
        );
        assert!(
            !covers(&ceiling, &id("container.resta")),
            "a prefix was covered"
        );
        assert!(
            !covers(&ceiling, &id("container.observe")),
            "the category was covered"
        );
    }

    #[test]
    fn disabled_ceiling_words_still_parse() {
        let Some(ceiling) = summary_of(parse_text(DISABLED)) else {
            panic!("a valid disabled summary did not parse");
        };
        assert!(!ceiling.enabled, "a disabled summary read as enabled");
    }

    #[test]
    fn unparseable_ceiling_words_are_malformed() {
        assert!(malformed("{ not json"), "unparseable words read as absent");
        assert!(
            malformed("{\"enabled\":true,\"entries\":["),
            "an unfinished summary read as absent"
        );
    }

    #[test]
    fn unknown_contract_in_a_ceiling_entry_is_malformed() {
        assert!(
            malformed(&covered_text("Bad ID!", "4")),
            "an unknown contract id read as parsed"
        );
        assert!(
            malformed(&covered_text("single", "4")),
            "a one-segment id read as parsed"
        );
    }

    #[test]
    fn a_target_name_outside_the_grammar_is_malformed() {
        let text = "{\"enabled\":true,\"entries\":[{\"contract\":\"container.restart\",\"targets\":{\"kind\":\"names\",\"names\":[\" leading\"]},\"max_per_hour\":4,\"args\":{}}]}";
        assert!(
            malformed(text),
            "a name that fails TargetName read as parsed"
        );
    }

    #[test]
    fn a_negative_ceiling_rate_is_malformed() {
        assert!(
            malformed(&covered_text("container.restart", "-3")),
            "a negative rate read as parsed"
        );
    }

    #[test]
    fn entries_and_args_are_required() {
        assert!(
            malformed("{\"enabled\":true,\"hash\":\"ab\"}"),
            "a summary without entries read as parsed"
        );
        let no_args = covered_text("container.restart", "4").replace(",\"args\":{}", "");
        assert!(malformed(&no_args), "a summary without args read as parsed");
    }

    #[test]
    fn ceiling_hash_is_opaque() {
        let first = text_of("container.restart", "4", "ab12");
        let second = text_of("container.restart", "4", "cd34");
        let Some(one) = summary_of(parse_text(&first)) else {
            panic!("the first ceiling did not parse");
        };
        let Some(two) = summary_of(parse_text(&second)) else {
            panic!("the second ceiling did not parse");
        };
        assert_eq!(
            covers(&one, &id("container.restart")),
            covers(&two, &id("container.restart")),
            "the hash changed what the coverage answer says"
        );
    }
}
