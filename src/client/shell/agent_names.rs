//! Names for agent rows, taken from what the agent says about its session.

use crate::protocol::{ClientShellAgent, ClientShellPane};

/// Separators agents put between a session name and their own default title,
/// as in `<name> | <project>` or `<name> - <agent>`.
const DEFAULT_TITLE_SEPARATORS: [&str; 3] = [" | ", " - ", " — "];

/// The agent session's name, or `None` when the agent reports none.
///
/// The first source with a value wins:
/// 1. the Herdr agent name from `herdr agent start --name` or `herdr agent rename`;
/// 2. a title reported with `herdr pane report-metadata --title`;
/// 3. the terminal title (OSC 0/2) without its leading activity glyph. A title
///    that only repeats a default (the agent's own name, the pane directory's
///    name, or the space name) or that is a path, such as the shell executable a
///    console shows before the agent sets a title, does not count. A trailing
///    ` | <default>` or ` - <default>` part is dropped.
pub(super) fn session_name(
    agent: &ClientShellAgent,
    pane: Option<&ClientShellPane>,
    workspace_label: &str,
) -> Option<String> {
    let reported = |value: Option<&str>| {
        value
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
    };
    reported(agent.name.as_deref())
        .or_else(|| reported(agent.title.as_deref()))
        .or_else(|| {
            let defaults = DefaultTitles {
                agent,
                pane,
                workspace_label,
            };
            title_session_name(agent.terminal_title_stripped.as_deref()?, &defaults)
                .map(str::to_owned)
        })
}

fn title_session_name<'a>(title: &'a str, defaults: &DefaultTitles<'_>) -> Option<&'a str> {
    let title = title.trim();
    if title.is_empty() || looks_like_path(title) || defaults.matches(title) {
        return None;
    }
    for separator in DEFAULT_TITLE_SEPARATORS {
        if let Some((name, suffix)) = title.rsplit_once(separator) {
            if defaults.matches(suffix) {
                return title_session_name(name, defaults);
            }
        }
    }
    Some(title)
}

/// Titles an agent shows before it has a session name.
struct DefaultTitles<'a> {
    agent: &'a ClientShellAgent,
    pane: Option<&'a ClientShellPane>,
    workspace_label: &'a str,
}

impl DefaultTitles<'_> {
    fn matches(&self, text: &str) -> bool {
        let text = text.trim();
        let same = |candidate: Option<&str>| {
            candidate.is_some_and(|candidate| {
                let candidate = candidate.trim();
                !candidate.is_empty() && candidate.eq_ignore_ascii_case(text)
            })
        };
        let pane_directories = self.pane.into_iter().flat_map(|pane| {
            [pane.cwd.as_deref(), pane.foreground_cwd.as_deref()]
                .into_iter()
                .map(|cwd| cwd.and_then(directory_name))
        });
        same(self.agent.agent.as_deref())
            || same(self.agent.display_agent.as_deref())
            || same(Some(self.workspace_label))
            || pane_directories.into_iter().any(same)
            || names_the_agent(text, self.agent.agent.as_deref())
    }
}

/// Whether `text` is a product name for the agent, such as `Claude Code` for `claude`.
fn names_the_agent(text: &str, agent: Option<&str>) -> bool {
    let Some(agent) = agent.and_then(crate::detect::parse_agent_label) else {
        return false;
    };
    !text.contains(['/', '\\'])
        && crate::detect::parse_agent_label(&text.split_whitespace().collect::<Vec<_>>().join("-"))
            == Some(agent)
}

/// The last component of a directory path, ignoring trailing separators.
fn directory_name(path: &str) -> Option<&str> {
    path.rsplit(['/', '\\'])
        .find(|component| !component.is_empty())
}

/// Absolute or home-relative paths, on Unix or Windows.
fn looks_like_path(text: &str) -> bool {
    let bytes = text.as_bytes();
    text.starts_with(['/', '\\', '~'])
        || (bytes.len() > 2
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && matches!(bytes[2], b'/' | b'\\'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn agent(kind: &str, stripped_title: Option<&str>) -> ClientShellAgent {
        ClientShellAgent {
            pane_id: "w1:p1".into(),
            workspace_id: "w1".into(),
            tab_id: "w1:t1".into(),
            name: None,
            display_agent: None,
            agent: Some(kind.into()),
            title: None,
            terminal_title: stripped_title.map(|title| format!("* {title}")),
            terminal_title_stripped: stripped_title.map(str::to_owned),
            agent_status: crate::api::schema::AgentStatus::Idle,
            state_change_seq: 0,
            state_labels: Vec::new(),
            tokens: Vec::new(),
            focused: false,
        }
    }

    fn pane(cwd: &str) -> ClientShellPane {
        ClientShellPane {
            pane_id: "w1:p1".into(),
            workspace_id: "w1".into(),
            tab_id: "w1:t1".into(),
            label: None,
            cwd: Some(cwd.into()),
            foreground_cwd: None,
            focused: false,
            right_click_passthrough: false,
        }
    }

    fn name(agent: &ClientShellAgent, cwd: &str) -> Option<String> {
        session_name(agent, Some(&pane(cwd)), "space")
    }

    #[test]
    fn reported_names_win_over_the_terminal_title() {
        let mut named = agent("claude", Some("topic title"));
        named.title = Some("reported title".into());
        assert_eq!(name(&named, "/src/app").as_deref(), Some("reported title"));
        named.name = Some("reviewer".into());
        assert_eq!(name(&named, "/src/app").as_deref(), Some("reviewer"));
        named.name = Some("  ".into());
        assert_eq!(name(&named, "/src/app").as_deref(), Some("reported title"));
    }

    #[test]
    fn terminal_titles_name_the_session() {
        for title in ["Fix the login form", "renamed-session", "a - b"] {
            let titled = agent("claude", Some(title));
            assert_eq!(name(&titled, "/src/app").as_deref(), Some(title));
        }
    }

    #[test]
    fn default_titles_leave_the_session_unnamed() {
        for (kind, title) in [
            ("claude", "claude"),
            ("claude", "Claude Code"),
            ("claude", "CLAUDE"),
            ("codex", "codex"),
            ("codex", "app"),
            ("codex", "space"),
            (
                "claude",
                r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe",
            ),
            ("claude", "/usr/bin/zsh"),
            ("claude", "~/src/app"),
            ("claude", "   "),
        ] {
            let titled = agent(kind, Some(title));
            assert_eq!(name(&titled, r"C:\src\app\"), None, "{kind}: {title:?}");
        }
        assert_eq!(name(&agent("claude", None), "/src/app"), None);
        assert_eq!(session_name(&agent("claude", None), None, "space"), None);

        let mut displayed = agent("claude", Some("My Agent"));
        displayed.display_agent = Some("my agent".into());
        assert_eq!(name(&displayed, "/src/app"), None);
    }

    #[test]
    fn trailing_default_parts_are_dropped() {
        for (kind, title, expected) in [
            ("codex", "Reply with ok | app", Some("Reply with ok")),
            ("codex", "renamed | app", Some("renamed")),
            ("codex", "renamed | other", Some("renamed | other")),
            ("grok", "review - grok", Some("review")),
            ("claude", "notes — Claude Code", Some("notes")),
            ("codex", "app | app", None),
        ] {
            let titled = agent(kind, Some(title));
            assert_eq!(name(&titled, "/src/app").as_deref(), expected, "{title}");
        }
    }

    #[test]
    fn product_names_only_match_the_running_agent() {
        let titled = agent("codex", Some("Claude Code"));
        assert_eq!(name(&titled, "/src/app").as_deref(), Some("Claude Code"));
        let unknown = agent("my-agent", Some("Claude Code"));
        assert_eq!(name(&unknown, "/src/app").as_deref(), Some("Claude Code"));
        let unknown = agent("my-agent", Some("my-agent"));
        assert_eq!(name(&unknown, "/src/app"), None);
    }
}
