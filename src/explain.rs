use serde::Deserialize;
use std::process::Command;

const MAX_SUMMARY_CHARS: usize = 220;

#[derive(Debug, Deserialize)]
struct ExplainResponse {
    state: Option<String>,
    matched_rule: Option<RuleInfo>,
    #[serde(default)]
    evaluated_rules: Vec<RuleInfo>,
}

#[derive(Debug, Deserialize)]
struct RuleInfo {
    id: Option<String>,
    matched: Option<bool>,
    evidence: Option<Evidence>,
}

#[derive(Debug, Deserialize)]
struct Evidence {
    region_preview: Option<String>,
    #[serde(default)]
    contains: Vec<String>,
}

pub(crate) fn notification_summary(
    pane_id: &str,
    herdr_bin: &str,
    expected_status: &str,
) -> Option<String> {
    let output = Command::new(herdr_bin)
        .args(["agent", "explain", pane_id, "--json"])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let response: ExplainResponse = serde_json::from_slice(&output.stdout).ok()?;
    if response.state.as_deref()? != expected_status {
        return None;
    }

    let rule = response.matched_rule.or_else(|| {
        response
            .evaluated_rules
            .into_iter()
            .find(|rule| rule.matched == Some(true))
    });
    let rule = rule?;

    let rule_label = rule.id.as_deref().map(humanize_rule_id);
    let evidence = rule.evidence.as_ref().and_then(evidence_summary);

    match (rule_label, evidence) {
        (Some(rule), Some(evidence)) => Some(truncate(&format!("{rule}: {evidence}"))),
        (Some(rule), None) => Some(truncate(&format!("Herdr matched {rule}"))),
        (None, Some(evidence)) => Some(truncate(&evidence)),
        (None, None) => None,
    }
}

fn evidence_summary(evidence: &Evidence) -> Option<String> {
    let preview = strip_ansi(evidence.region_preview.as_deref()?).replace('\r', "");
    let lines = preview
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty());

    for pattern in &evidence.contains {
        if let Some(line) = lines.clone().find(|line| line.contains(pattern)) {
            return Some(compact(line));
        }
    }

    preview
        .lines()
        .rev()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(compact)
}

fn humanize_rule_id(id: &str) -> String {
    let mut words = id
        .split(['_', '-'])
        .filter(|word| !word.is_empty())
        .map(|word| word.to_ascii_lowercase())
        .collect::<Vec<_>>();

    if let Some(first) = words.first_mut() {
        if let Some(character) = first.get_mut(0..1) {
            character.make_ascii_uppercase();
        }
    }

    words.join(" ")
}

fn compact(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn truncate(value: &str) -> String {
    let value = compact(value);
    if value.chars().count() <= MAX_SUMMARY_CHARS {
        return value;
    }

    let mut output: String = value.chars().take(MAX_SUMMARY_CHARS - 3).collect();
    output.push_str("...");
    output
}

fn strip_ansi(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut in_escape = false;
    let mut in_csi = false;
    for character in value.chars() {
        if in_escape {
            if !in_csi && character == '[' {
                in_csi = true;
            } else if in_csi && ('@'..='~').contains(&character) {
                in_escape = false;
                in_csi = false;
            } else if !in_csi {
                in_escape = false;
            }
            continue;
        }

        if character == '\u{1b}' {
            in_escape = true;
            in_csi = false;
        } else {
            output.push(character);
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_summary_from_matched_rule_and_evidence() {
        let json = br#"{
            "state": "blocked",
            "matched_rule": {
                "id": "needs_permission",
                "evidence": {
                    "contains": ["Do you want to allow"],
                    "region_preview": "\u001b[31mDo you want to allow this command?\u001b[0m"
                }
            },
            "evaluated_rules": []
        }"#;

        let response: ExplainResponse = serde_json::from_slice(json).unwrap();
        let rule = response.matched_rule.unwrap();

        assert_eq!(
            humanize_rule_id(rule.id.as_deref().unwrap()),
            "Needs permission"
        );
        assert_eq!(
            evidence_summary(rule.evidence.as_ref().unwrap()).unwrap(),
            "Do you want to allow this command?"
        );
    }

    #[test]
    fn falls_back_to_a_matched_evaluated_rule() {
        let json = br#"{
            "state": "done",
            "evaluated_rules": [{
                "id": "completed",
                "matched": true,
                "evidence": {"region_preview": "All tests passed", "contains": []}
            }]
        }"#;

        let response: ExplainResponse = serde_json::from_slice(json).unwrap();
        let rule = response
            .evaluated_rules
            .into_iter()
            .find(|rule| rule.matched == Some(true))
            .unwrap();

        assert_eq!(rule.id.as_deref(), Some("completed"));
        assert_eq!(
            evidence_summary(rule.evidence.as_ref().unwrap()).unwrap(),
            "All tests passed"
        );
    }

    #[test]
    fn strips_ansi_and_truncates_summaries() {
        let value = "\u{1b}[32mA   short   message\u{1b}[0m";
        assert_eq!(compact(&strip_ansi(value)), "A short message");
        assert_eq!(
            truncate(&"x".repeat(MAX_SUMMARY_CHARS + 10))
                .chars()
                .count(),
            MAX_SUMMARY_CHARS
        );
    }
}
