use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::PathBuf;
use std::sync::OnceLock;

static CONFIG_ENV: OnceLock<HashMap<String, String>> = OnceLock::new();

pub(crate) fn status_is_enabled(status: &str) -> bool {
    let configured =
        config_var("HERDR_FOCUS_NOTIFY_STATUSES").unwrap_or_else(|| "blocked,done".to_string());

    configured
        .split(',')
        .map(|value| value.trim().to_ascii_lowercase())
        .any(|value| value == status)
}

pub(crate) fn is_enabled() -> bool {
    config_var("HERDR_FOCUS_NOTIFY_ENABLED")
        .map(|value| {
            !matches!(
                value.to_ascii_lowercase().as_str(),
                "0" | "false" | "no" | "off"
            )
        })
        .unwrap_or(true)
}

pub(crate) fn is_debug_enabled() -> bool {
    config_var("HERDR_FOCUS_NOTIFY_DEBUG")
        .map(|value| {
            matches!(
                value.to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
}

pub(crate) fn alerter_timeout_secs() -> u64 {
    parse_timeout_secs(config_var("HERDR_FOCUS_NOTIFY_TIMEOUT"))
}

pub(crate) fn activate_app() -> Option<String> {
    config_var("HERDR_FOCUS_NOTIFY_ACTIVATE_APP")
}

pub(crate) fn parse_timeout_secs(raw: Option<String>) -> u64 {
    raw.and_then(|v| v.trim().parse::<u64>().ok())
        .unwrap_or(3600)
}

pub(crate) fn config_var(key: &str) -> Option<String> {
    env::var(key)
        .ok()
        .or_else(|| CONFIG_ENV.get_or_init(load_config_env).get(key).cloned())
}

fn load_config_env() -> HashMap<String, String> {
    let Some(config_dir) = env::var_os("HERDR_PLUGIN_CONFIG_DIR") else {
        return HashMap::new();
    };

    let path = PathBuf::from(config_dir).join(".env");
    let Ok(content) = fs::read_to_string(path) else {
        return HashMap::new();
    };

    parse_env_file(&content)
}

pub(crate) fn parse_env_file(content: &str) -> HashMap<String, String> {
    let mut values = HashMap::new();

    for line in content.lines() {
        let Some((key, value)) = parse_env_line(line) else {
            continue;
        };
        values.insert(key, value);
    }

    values
}

fn parse_env_line(line: &str) -> Option<(String, String)> {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return None;
    }

    let assignment = trimmed.strip_prefix("export ").unwrap_or(trimmed);
    let (key, raw_value) = assignment.split_once('=')?;
    let key = key.trim();
    if key.is_empty() {
        return None;
    }

    Some((key.to_string(), parse_env_value(raw_value.trim())))
}

fn parse_env_value(value: &str) -> String {
    let mut chars = value.chars();
    let Some(quote @ ('\'' | '"')) = chars.next() else {
        return strip_unquoted_comment(value).trim().to_string();
    };

    let mut output = String::new();
    let mut escaped = false;

    for ch in chars {
        if escaped {
            match ch {
                'n' if quote == '"' => output.push('\n'),
                'r' if quote == '"' => output.push('\r'),
                't' if quote == '"' => output.push('\t'),
                _ => output.push(ch),
            }
            escaped = false;
            continue;
        }

        if quote == '"' && ch == '\\' {
            escaped = true;
            continue;
        }

        if ch == quote {
            break;
        }

        output.push(ch);
    }

    output
}

fn strip_unquoted_comment(value: &str) -> &str {
    let mut prev_was_space = true;

    for (index, ch) in value.char_indices() {
        if ch == '#' && prev_was_space {
            return &value[..index];
        }
        prev_was_space = ch.is_whitespace();
    }

    value
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_timeout_secs_defaults_and_overrides() {
        assert_eq!(parse_timeout_secs(None), 3600);
        assert_eq!(parse_timeout_secs(Some("abc".to_string())), 3600);
        assert_eq!(parse_timeout_secs(Some("0".to_string())), 0);
        assert_eq!(parse_timeout_secs(Some("120".to_string())), 120);
    }

    #[test]
    fn parses_plugin_env_file() {
        let values = parse_env_file(
            r#"
            # comment
            export HERDR_FOCUS_NOTIFY_ENABLED=1
            HERDR_FOCUS_NOTIFY_NOTIFIER="/opt/homebrew/bin/alerter"
            HERDR_FOCUS_NOTIFY_STATUSES='blocked,done'
            HERDR_FOCUS_NOTIFY_ACTIVATE_APP=kitty # inline comment
            HERDR_FOCUS_NOTIFY_TITLE="line\nnext"
            "#,
        );

        assert_eq!(
            values.get("HERDR_FOCUS_NOTIFY_NOTIFIER").unwrap(),
            "/opt/homebrew/bin/alerter"
        );
        assert_eq!(
            values.get("HERDR_FOCUS_NOTIFY_STATUSES").unwrap(),
            "blocked,done"
        );
        assert_eq!(
            values.get("HERDR_FOCUS_NOTIFY_ACTIVATE_APP").unwrap(),
            "kitty"
        );
        assert_eq!(
            values.get("HERDR_FOCUS_NOTIFY_TITLE").unwrap(),
            "line\nnext"
        );
    }

    #[test]
    fn keeps_hashes_inside_unquoted_values_without_leading_space() {
        let values = parse_env_file("TOKEN=abc#123\n");

        assert_eq!(values.get("TOKEN").unwrap(), "abc#123");
    }
}
