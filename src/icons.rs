use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::state::plugin_state_dir;

static ICON_WRITE_SEQUENCE: AtomicUsize = AtomicUsize::new(0);

pub(crate) fn agent_icon_path(names: &[Option<&str>]) -> Option<String> {
    names
        .iter()
        .copied()
        .flatten()
        .filter_map(icon_for_agent)
        .find_map(write_embedded_icon)
}

struct AgentIcon {
    exact: &'static [&'static str],
    substring: Option<&'static str>,
    file: &'static str,
    bytes: &'static [u8],
}

const AGENT_ICONS: &[AgentIcon] = &[
    AgentIcon {
        exact: &["claudecode"],
        substring: Some("claudecode"),
        file: "claudecode-color.png",
        bytes: include_bytes!("../assets/icons/agents/claudecode-color.png"),
    },
    AgentIcon {
        exact: &["codex", "openaicodex"],
        substring: Some("codex"),
        file: "codex-color.png",
        bytes: include_bytes!("../assets/icons/agents/codex-color.png"),
    },
    AgentIcon {
        exact: &["claude", "anthropicclaude"],
        substring: Some("claude"),
        file: "claude-color.png",
        bytes: include_bytes!("../assets/icons/agents/claude-color.png"),
    },
    AgentIcon {
        exact: &["cursor"],
        substring: Some("cursor"),
        file: "cursor.png",
        bytes: include_bytes!("../assets/icons/agents/cursor.png"),
    },
    AgentIcon {
        exact: &["geminicli", "googlegeminicli"],
        substring: Some("geminicli"),
        file: "geminicli-color.png",
        bytes: include_bytes!("../assets/icons/agents/geminicli-color.png"),
    },
    AgentIcon {
        exact: &["gemini", "googlegemini"],
        substring: Some("gemini"),
        file: "gemini-color.png",
        bytes: include_bytes!("../assets/icons/agents/gemini-color.png"),
    },
    AgentIcon {
        exact: &["githubcopilot"],
        substring: Some("githubcopilot"),
        file: "githubcopilot.png",
        bytes: include_bytes!("../assets/icons/agents/githubcopilot.png"),
    },
    AgentIcon {
        exact: &["copilot", "microsoftcopilot"],
        substring: Some("copilot"),
        file: "copilot-color.png",
        bytes: include_bytes!("../assets/icons/agents/copilot-color.png"),
    },
    AgentIcon {
        exact: &["deepseek"],
        substring: Some("deepseek"),
        file: "deepseek-color.png",
        bytes: include_bytes!("../assets/icons/agents/deepseek-color.png"),
    },
    AgentIcon {
        exact: &["grok", "xai", "xaigrok"],
        substring: Some("grok"),
        file: "grok.png",
        bytes: include_bytes!("../assets/icons/agents/grok.png"),
    },
    AgentIcon {
        exact: &["qwen", "alibabaqwen"],
        substring: Some("qwen"),
        file: "qwen-color.png",
        bytes: include_bytes!("../assets/icons/agents/qwen-color.png"),
    },
    AgentIcon {
        exact: &["openai", "chatgpt"],
        substring: Some("openai"),
        file: "openai.png",
        bytes: include_bytes!("../assets/icons/agents/openai.png"),
    },
    AgentIcon {
        exact: &["opencode"],
        substring: Some("opencode"),
        file: "opencode.png",
        bytes: include_bytes!("../assets/icons/agents/opencode.png"),
    },
    AgentIcon {
        exact: &["openhands"],
        substring: Some("openhands"),
        file: "openhands-color.png",
        bytes: include_bytes!("../assets/icons/agents/openhands-color.png"),
    },
    AgentIcon {
        exact: &["roocode", "roo"],
        substring: Some("roocode"),
        file: "roocode.png",
        bytes: include_bytes!("../assets/icons/agents/roocode.png"),
    },
    AgentIcon {
        exact: &["cline"],
        substring: None,
        file: "cline.png",
        bytes: include_bytes!("../assets/icons/agents/cline.png"),
    },
    AgentIcon {
        exact: &["windsurf"],
        substring: Some("windsurf"),
        file: "windsurf.png",
        bytes: include_bytes!("../assets/icons/agents/windsurf.png"),
    },
    AgentIcon {
        exact: &["devin"],
        substring: None,
        file: "devin-color.png",
        bytes: include_bytes!("../assets/icons/agents/devin-color.png"),
    },
    AgentIcon {
        exact: &["manus"],
        substring: None,
        file: "manus.png",
        bytes: include_bytes!("../assets/icons/agents/manus.png"),
    },
    AgentIcon {
        exact: &["kiro"],
        substring: None,
        file: "kiro-color.png",
        bytes: include_bytes!("../assets/icons/agents/kiro-color.png"),
    },
    AgentIcon {
        exact: &["trae"],
        substring: None,
        file: "trae-color.png",
        bytes: include_bytes!("../assets/icons/agents/trae-color.png"),
    },
    AgentIcon {
        exact: &["zencoder"],
        substring: None,
        file: "zencoder-color.png",
        bytes: include_bytes!("../assets/icons/agents/zencoder-color.png"),
    },
    AgentIcon {
        exact: &["lovable"],
        substring: None,
        file: "lovable-color.png",
        bytes: include_bytes!("../assets/icons/agents/lovable-color.png"),
    },
    AgentIcon {
        exact: &["v0", "vercelv0"],
        substring: None,
        file: "v0.png",
        bytes: include_bytes!("../assets/icons/agents/v0.png"),
    },
];

fn icon_for_agent(name: &str) -> Option<&'static AgentIcon> {
    let key = normalize_agent_name(name);

    for agent in AGENT_ICONS {
        if agent.exact.iter().any(|&n| n == key) {
            return Some(agent);
        }
    }
    for agent in AGENT_ICONS {
        if let Some(sub) = agent.substring {
            if key.contains(sub) {
                return Some(agent);
            }
        }
    }
    None
}

fn write_embedded_icon(icon: &AgentIcon) -> Option<String> {
    let dir = icon_state_dir();
    fs::create_dir_all(&dir).ok()?;

    let path = dir.join(icon.file);
    let temp_path = dir.join(format!(
        ".{}-{}-{}",
        icon.file,
        std::process::id(),
        ICON_WRITE_SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ));
    fs::write(&temp_path, icon.bytes).ok()?;
    fs::rename(temp_path, &path).ok()?;

    Some(path.to_string_lossy().into_owned())
}

fn icon_state_dir() -> PathBuf {
    plugin_state_dir().join("icons")
}

fn normalize_agent_name(name: &str) -> String {
    name.chars()
        .flat_map(char::to_lowercase)
        .filter(|ch| ch.is_ascii_alphanumeric())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_primary_agent_names_to_icons() {
        for agent in AGENT_ICONS {
            assert_eq!(
                icon_for_agent(agent.exact[0]).map(|icon| icon.file),
                Some(agent.file)
            );
        }
    }

    #[test]
    fn matches_common_aliases_and_substrings_to_icons() {
        let cases = [
            ("Codex", "codex-color.png"),
            ("OpenAI Codex", "codex-color.png"),
            ("Claude Code", "claudecode-color.png"),
            ("GitHub Copilot", "githubcopilot.png"),
            ("Gemini CLI", "geminicli-color.png"),
            ("DeepSeek", "deepseek-color.png"),
            ("Qwen", "qwen-color.png"),
            ("v0", "v0.png"),
        ];

        for (name, file) in cases {
            assert_eq!(icon_for_agent(name).map(|icon| icon.file), Some(file));
        }
    }

    #[test]
    fn writes_embedded_icon_for_known_agent() {
        let path = agent_icon_path(&[Some("Codex")]).unwrap();

        assert!(path.ends_with("/icons/codex-color.png"));
        assert_eq!(
            fs::read(path).unwrap(),
            include_bytes!("../assets/icons/agents/codex-color.png")
        );
    }

    #[test]
    fn falls_back_to_next_name_that_matches() {
        let path = agent_icon_path(&[Some("Unknown Agent"), Some("Codex")]).unwrap();

        assert!(path.ends_with("/icons/codex-color.png"));
    }

    #[test]
    fn prefers_first_matching_name() {
        let path = agent_icon_path(&[Some("Claude"), Some("Codex")]).unwrap();

        assert!(path.ends_with("/icons/claude-color.png"));
    }

    #[test]
    fn ignores_unknown_agents() {
        assert_eq!(agent_icon_path(&[Some("unknown")]), None);
    }
}
