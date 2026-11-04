mod rules;

pub use rules::SidebarTokenRule;

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::detect::Agent;

const MAX_SIDEBAR_ROWS: usize = 16;
const MAX_SIDEBAR_TOKENS_PER_ROW: usize = 16;
const DEFAULT_SIDEBAR_ROW_GAP: u16 = 0;

fn deserialize_sidebar_rows<'de, D, T>(deserializer: D) -> Result<Vec<Vec<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    let rows = Vec::<Vec<T>>::deserialize(deserializer)?;
    validate_sidebar_rows(&rows).map_err(serde::de::Error::custom)?;
    Ok(rows)
}

fn validate_sidebar_rows<T>(rows: &[Vec<T>]) -> Result<(), String> {
    if rows.len() > MAX_SIDEBAR_ROWS {
        return Err(format!(
            "sidebar layouts may contain at most {MAX_SIDEBAR_ROWS} rows"
        ));
    }
    if rows
        .iter()
        .any(|row| row.len() > MAX_SIDEBAR_TOKENS_PER_ROW)
    {
        return Err(format!(
            "sidebar rows may contain at most {MAX_SIDEBAR_TOKENS_PER_ROW} tokens"
        ));
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SidebarTokenColor {
    r: u8,
    g: u8,
    b: u8,
}

impl SidebarTokenColor {
    pub(crate) fn ratatui(self) -> ratatui::style::Color {
        ratatui::style::Color::Rgb(self.r, self.g, self.b)
    }
}

impl Serialize for SidebarTokenColor {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b))
    }
}

impl<'de> Deserialize<'de> for SidebarTokenColor {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        let hex = value.strip_prefix('#').filter(|hex| {
            hex.is_ascii()
                && matches!(hex.len(), 3 | 6)
                && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
        });
        let Some(hex) = hex else {
            return Err(serde::de::Error::custom(
                "sidebar token fg must be #RGB or #RRGGBB",
            ));
        };
        let (r, g, b) = if hex.len() == 3 {
            let mut digits = hex
                .bytes()
                .map(|byte| char::from(byte).to_digit(16).expect("validated hex digit") as u8 * 17);
            (
                digits.next().expect("three hex digits"),
                digits.next().expect("three hex digits"),
                digits.next().expect("three hex digits"),
            )
        } else {
            (
                u8::from_str_radix(&hex[0..2], 16).expect("validated hex digits"),
                u8::from_str_radix(&hex[2..4], 16).expect("validated hex digits"),
                u8::from_str_radix(&hex[4..6], 16).expect("validated hex digits"),
            )
        };
        Ok(Self { r, g, b })
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SidebarTokenStyle {
    pub fg: Option<SidebarTokenColor>,
    pub bold: Option<bool>,
    pub dim: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentSidebarToken {
    StateIcon,
    StateText,
    Machine,
    Workspace,
    Tab,
    Pane,
    Agent,
    SessionName,
    TerminalTitle,
    TerminalTitleStripped,
    Custom(String),
    Styled {
        token: Box<AgentSidebarToken>,
        style: SidebarTokenStyle,
        rules: Vec<SidebarTokenRule>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpaceSidebarToken {
    StateIcon,
    StateText,
    Workspace,
    Branch,
    GitStatus,
    Custom(String),
    Styled {
        token: Box<SpaceSidebarToken>,
        style: SidebarTokenStyle,
        rules: Vec<SidebarTokenRule>,
    },
}

impl AgentSidebarToken {
    pub(crate) fn style_for_value(&self, value: &str) -> Option<SidebarTokenStyle> {
        match self {
            Self::Styled { style, rules, .. } => rules::matching_style(rules, *style, value),
            _ => Some(SidebarTokenStyle::default()),
        }
    }

    pub(crate) fn parts(&self) -> (&Self, SidebarTokenStyle) {
        match self {
            Self::Styled { token, style, .. } => (token, *style),
            token => (token, SidebarTokenStyle::default()),
        }
    }
}

impl SpaceSidebarToken {
    pub(crate) fn style_for_value(&self, value: &str) -> Option<SidebarTokenStyle> {
        match self {
            Self::Styled { style, rules, .. } => rules::matching_style(rules, *style, value),
            _ => Some(SidebarTokenStyle::default()),
        }
    }

    pub(crate) fn parts(&self) -> (&Self, SidebarTokenStyle) {
        match self {
            Self::Styled { token, style, .. } => (token, *style),
            token => (token, SidebarTokenStyle::default()),
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawStyledSidebarToken {
    token: String,
    #[serde(default)]
    fg: Option<SidebarTokenColor>,
    #[serde(default)]
    bold: Option<bool>,
    #[serde(default)]
    dim: Option<bool>,
    #[serde(default)]
    rules: Vec<SidebarTokenRule>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum RawSidebarToken {
    Plain(String),
    Styled(RawStyledSidebarToken),
}

impl RawSidebarToken {
    fn parts(self) -> Result<(String, Option<SidebarTokenStyle>, Vec<SidebarTokenRule>), String> {
        match self {
            Self::Plain(token) => Ok((token, None, Vec::new())),
            Self::Styled(token) => {
                if token.rules.len() > 16 {
                    return Err("sidebar tokens may contain at most 16 rules".into());
                }
                if !token.rules.is_empty()
                    && matches!(token.token.as_str(), "state_icon" | "git_status")
                {
                    return Err("sidebar rules require a text-valued token".into());
                }
                Ok((
                    token.token,
                    Some(SidebarTokenStyle {
                        fg: token.fg,
                        bold: token.bold,
                        dim: token.dim,
                    }),
                    token.rules,
                ))
            }
        }
    }
}

fn parse_sidebar_token<T>(value: String, builtins: &[(&str, T)]) -> Result<T, String>
where
    T: Clone + From<String>,
{
    if let Some((_, token)) = builtins.iter().find(|(name, _)| *name == value) {
        return Ok(token.clone());
    }
    let Some(name) = value.strip_prefix('$') else {
        return Err(format!(
            "unknown sidebar token `{value}`; custom tokens must start with `$`"
        ));
    };
    if name.is_empty()
        || name.len() > 32
        || !name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-'))
    {
        return Err(format!("invalid custom sidebar token `{value}`"));
    }
    Ok(T::from(name.to_string()))
}

fn serialize_styled_token<S>(
    name: String,
    style: SidebarTokenStyle,
    rules: &[SidebarTokenRule],
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    use serde::ser::SerializeMap;
    let mut map = serializer.serialize_map(None)?;
    map.serialize_entry("token", &name)?;
    if let Some(fg) = style.fg {
        map.serialize_entry("fg", &fg)?;
    }
    if let Some(bold) = style.bold {
        map.serialize_entry("bold", &bold)?;
    }
    if let Some(dim) = style.dim {
        map.serialize_entry("dim", &dim)?;
    }
    if !rules.is_empty() {
        map.serialize_entry("rules", rules)?;
    }
    map.end()
}

fn agent_token_name(token: &AgentSidebarToken) -> String {
    match token {
        AgentSidebarToken::StateIcon => "state_icon".into(),
        AgentSidebarToken::StateText => "state_text".into(),
        AgentSidebarToken::Machine => "machine".into(),
        AgentSidebarToken::Workspace => "workspace".into(),
        AgentSidebarToken::Tab => "tab".into(),
        AgentSidebarToken::Pane => "pane".into(),
        AgentSidebarToken::Agent => "agent".into(),
        AgentSidebarToken::SessionName => "session_name".into(),
        AgentSidebarToken::TerminalTitle => "terminal_title".into(),
        AgentSidebarToken::TerminalTitleStripped => "terminal_title_stripped".into(),
        AgentSidebarToken::Custom(name) => format!("${name}"),
        AgentSidebarToken::Styled { token, .. } => agent_token_name(token),
    }
}

fn space_token_name(token: &SpaceSidebarToken) -> String {
    match token {
        SpaceSidebarToken::StateIcon => "state_icon".into(),
        SpaceSidebarToken::StateText => "state_text".into(),
        SpaceSidebarToken::Workspace => "workspace".into(),
        SpaceSidebarToken::Branch => "branch".into(),
        SpaceSidebarToken::GitStatus => "git_status".into(),
        SpaceSidebarToken::Custom(name) => format!("${name}"),
        SpaceSidebarToken::Styled { token, .. } => space_token_name(token),
    }
}

impl Serialize for AgentSidebarToken {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            Self::Styled {
                token,
                style,
                rules,
            } => serialize_styled_token(agent_token_name(token), *style, rules, serializer),
            token => serializer.serialize_str(&agent_token_name(token)),
        }
    }
}

impl From<String> for AgentSidebarToken {
    fn from(value: String) -> Self {
        Self::Custom(value)
    }
}

impl<'de> Deserialize<'de> for AgentSidebarToken {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let (value, style, rules) = RawSidebarToken::deserialize(deserializer)?
            .parts()
            .map_err(serde::de::Error::custom)?;
        let token = parse_sidebar_token(
            value,
            &[
                ("state_icon", Self::StateIcon),
                ("state_text", Self::StateText),
                ("machine", Self::Machine),
                ("workspace", Self::Workspace),
                ("tab", Self::Tab),
                ("pane", Self::Pane),
                ("agent", Self::Agent),
                ("session_name", Self::SessionName),
                ("terminal_title", Self::TerminalTitle),
                ("terminal_title_stripped", Self::TerminalTitleStripped),
            ],
        )
        .map_err(serde::de::Error::custom)?;
        Ok(style.map_or(token.clone(), |style| Self::Styled {
            token: Box::new(token),
            style,
            rules,
        }))
    }
}

impl Serialize for SpaceSidebarToken {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            Self::Styled {
                token,
                style,
                rules,
            } => serialize_styled_token(space_token_name(token), *style, rules, serializer),
            token => serializer.serialize_str(&space_token_name(token)),
        }
    }
}

impl From<String> for SpaceSidebarToken {
    fn from(value: String) -> Self {
        Self::Custom(value)
    }
}

impl<'de> Deserialize<'de> for SpaceSidebarToken {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let (value, style, rules) = RawSidebarToken::deserialize(deserializer)?
            .parts()
            .map_err(serde::de::Error::custom)?;
        let token = parse_sidebar_token(
            value,
            &[
                ("state_icon", Self::StateIcon),
                ("state_text", Self::StateText),
                ("workspace", Self::Workspace),
                ("branch", Self::Branch),
                ("git_status", Self::GitStatus),
            ],
        )
        .map_err(serde::de::Error::custom)?;
        Ok(style.map_or(token.clone(), |style| Self::Styled {
            token: Box::new(token),
            style,
            rules,
        }))
    }
}

type AgentSidebarRows = Vec<Vec<AgentSidebarToken>>;
type SpaceSidebarRows = Vec<Vec<SpaceSidebarToken>>;

fn deserialize_rows_by_agent<'de, D>(
    deserializer: D,
) -> Result<BTreeMap<String, AgentSidebarRows>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let rows_by_agent = BTreeMap::<String, AgentSidebarRows>::deserialize(deserializer)?;
    for (id, rows) in &rows_by_agent {
        if crate::detect::parse_canonical_agent_label(id).is_none() {
            return Err(serde::de::Error::custom(format!(
                "unknown canonical agent id `{id}` in sidebar rows_by_agent"
            )));
        }
        validate_sidebar_rows(rows).map_err(serde::de::Error::custom)?;
    }
    Ok(rows_by_agent)
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default)]
pub struct AgentsSidebarConfig {
    #[serde(deserialize_with = "deserialize_sidebar_rows")]
    pub rows: AgentSidebarRows,
    #[serde(default, deserialize_with = "deserialize_rows_by_agent")]
    pub rows_by_agent: BTreeMap<String, AgentSidebarRows>,
    pub row_gap: u16,
    /// Retired. Agents always appear under a heading for their space. Parsed so
    /// older configs still load, then ignored.
    #[serde(rename = "group_by_space", skip_serializing)]
    pub _retired_group_by_space: bool,
}

const DEFAULT_AGENT_ROWS: [&[AgentSidebarToken]; 2] = [
    &[
        AgentSidebarToken::StateIcon,
        AgentSidebarToken::Machine,
        AgentSidebarToken::Workspace,
        AgentSidebarToken::Tab,
    ],
    &[AgentSidebarToken::Agent],
];

impl AgentsSidebarConfig {
    /// Under a space heading the default layout shrinks to one line that
    /// names the agent session, because the heading already names the space.
    /// Custom layouts stay as written.
    pub(crate) fn rows_for_agent(
        &self,
        agent: Option<Agent>,
        under_heading: bool,
    ) -> std::borrow::Cow<'_, AgentSidebarRows> {
        if let Some(rows) =
            agent.and_then(|agent| self.rows_by_agent.get(crate::detect::agent_label(agent)))
        {
            return std::borrow::Cow::Borrowed(rows);
        }
        if under_heading && self.uses_default_rows() {
            return std::borrow::Cow::Owned(vec![vec![
                AgentSidebarToken::StateIcon,
                AgentSidebarToken::SessionName,
            ]]);
        }
        std::borrow::Cow::Borrowed(&self.rows)
    }

    /// Whether `rows` is the built-in layout, which shows `session_name` under a heading.
    pub(crate) fn uses_default_rows(&self) -> bool {
        self.rows.len() == DEFAULT_AGENT_ROWS.len()
            && self
                .rows
                .iter()
                .zip(DEFAULT_AGENT_ROWS)
                .all(|(row, default)| row.as_slice() == default)
    }
}

impl Default for AgentsSidebarConfig {
    fn default() -> Self {
        Self {
            rows: DEFAULT_AGENT_ROWS.iter().map(|row| row.to_vec()).collect(),
            rows_by_agent: BTreeMap::new(),
            row_gap: DEFAULT_SIDEBAR_ROW_GAP,
            _retired_group_by_space: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default)]
pub struct SpacesSidebarConfig {
    #[serde(deserialize_with = "deserialize_sidebar_rows")]
    pub rows: SpaceSidebarRows,
    pub row_gap: u16,
}

impl Default for SpacesSidebarConfig {
    fn default() -> Self {
        Self {
            rows: vec![
                vec![SpaceSidebarToken::StateIcon, SpaceSidebarToken::Workspace],
                vec![SpaceSidebarToken::Branch, SpaceSidebarToken::GitStatus],
            ],
            row_gap: DEFAULT_SIDEBAR_ROW_GAP,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct NotificationsSidebarConfig {
    /// Show the notification log below the agents, so the sidebar splits into thirds.
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct SidebarConfig {
    pub agents: AgentsSidebarConfig,
    pub spaces: SpacesSidebarConfig,
    pub notifications: NotificationsSidebarConfig,
    /// Color for space names that match no `space_colors` rule. Unset keeps the theme colors.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub space_color_default: Option<String>,
    /// Ordered rules that color space names by the space's directory. The first match wins.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub space_colors: Vec<SpaceColorRuleConfig>,
}

/// One `[[ui.sidebar.space_colors]]` entry.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct SpaceColorRuleConfig {
    /// Regex matched against the space's directory.
    pub cwd: String,
    /// Color for matching space names, in the theme color syntax.
    pub color: String,
}

/// Space name colors compiled from `[ui.sidebar]`. Invalid rules are left out.
#[derive(Debug, Clone, Default)]
pub(crate) struct SpaceColors {
    default: Option<ratatui::style::Color>,
    rules: Vec<(regex::Regex, ratatui::style::Color)>,
}

impl SpaceColors {
    /// Compiles the rules and returns one diagnostic for each value it skips.
    pub(crate) fn compile(config: &SidebarConfig) -> (Self, Vec<String>) {
        let mut diagnostics = Vec::new();
        let default = config.space_color_default.as_deref().and_then(|value| {
            let color = crate::config::try_parse_color(value);
            if color.is_none() {
                diagnostics.push(format!(
                    "ui.sidebar.space_color_default = {value:?} is not a color; using theme colors"
                ));
            }
            color
        });
        let rules = config
            .space_colors
            .iter()
            .enumerate()
            .filter_map(|(index, rule)| {
                let key = format!("ui.sidebar.space_colors[{index}]");
                let color = crate::config::try_parse_color(&rule.color);
                if color.is_none() {
                    diagnostics.push(format!(
                        "{key}.color = {:?} is not a color; skipping the rule",
                        rule.color
                    ));
                }
                let pattern = match regex::Regex::new(&rule.cwd) {
                    Ok(_) if rule.cwd.is_empty() => {
                        diagnostics.push(format!("{key}.cwd is empty; skipping the rule"));
                        None
                    }
                    Ok(pattern) => Some(pattern),
                    Err(error) => {
                        diagnostics.push(format!(
                            "{key}.cwd = {:?} is not a valid regex ({}); skipping the rule",
                            rule.cwd,
                            regex_error_summary(&error)
                        ));
                        None
                    }
                };
                Some((pattern?, color?))
            })
            .collect();
        (Self { default, rules }, diagnostics)
    }

    /// Color for a space whose directory is `cwd`: the first matching rule, else the default.
    pub(crate) fn color_for(&self, cwd: Option<&str>) -> Option<ratatui::style::Color> {
        cwd.and_then(|cwd| {
            self.rules
                .iter()
                .find(|(pattern, _)| pattern.is_match(cwd))
                .map(|(_, color)| *color)
        })
        .or(self.default)
    }
}

/// The last line of a regex error, which states the problem without the caret diagram.
fn regex_error_summary(error: &regex::Error) -> String {
    let text = error.to_string();
    text.lines()
        .rev()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(|line| line.strip_prefix("error: ").unwrap_or(line).to_owned())
        .unwrap_or(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_the_compact_agent_and_existing_space_layouts() {
        let config = SidebarConfig::default();
        assert_eq!(
            config.agents.rows,
            vec![
                vec![
                    AgentSidebarToken::StateIcon,
                    AgentSidebarToken::Machine,
                    AgentSidebarToken::Workspace,
                    AgentSidebarToken::Tab,
                ],
                vec![AgentSidebarToken::Agent],
            ]
        );
        assert!(config.agents.rows_by_agent.is_empty());
        assert_eq!(config.agents.row_gap, 0);
        // The retired grouping switch still parses, without a diagnostic, and is not written back.
        let grouped: crate::config::Config =
            toml::from_str("[ui.sidebar.agents]\ngroup_by_space = true\n").expect("grouped");
        assert!(grouped.collect_diagnostics().is_empty());
        assert!(!toml::to_string(&grouped.ui.sidebar)
            .unwrap()
            .contains("group_by_space"));
        assert_eq!(config.space_color_default, None);
        assert!(config.space_colors.is_empty());
        assert_eq!(
            config.spaces.rows,
            vec![
                vec![SpaceSidebarToken::StateIcon, SpaceSidebarToken::Workspace],
                vec![SpaceSidebarToken::Branch, SpaceSidebarToken::GitStatus],
            ]
        );
        assert_eq!(config.spaces.row_gap, 0);
        assert!(!config.notifications.enabled);
        let log: crate::config::Config =
            toml::from_str("[ui.sidebar.notifications]\nenabled = true\n").expect("log");
        assert!(log.ui.sidebar.notifications.enabled);
        assert!(log.collect_diagnostics().is_empty());
    }

    #[test]
    fn default_layout_names_the_session_under_a_heading() {
        let config = AgentsSidebarConfig::default();
        assert!(config.uses_default_rows());
        assert_eq!(
            config.rows_for_agent(None, true).into_owned(),
            vec![vec![
                AgentSidebarToken::StateIcon,
                AgentSidebarToken::SessionName,
            ]]
        );
        assert_eq!(config.rows_for_agent(None, false).into_owned(), config.rows);

        let custom: AgentsSidebarConfig =
            toml::from_str(r#"rows = [["state_icon", "session_name", "tab"]]"#).unwrap();
        assert!(!custom.uses_default_rows());
        assert_eq!(
            custom.rows_for_agent(None, true).into_owned(),
            vec![vec![
                AgentSidebarToken::StateIcon,
                AgentSidebarToken::SessionName,
                AgentSidebarToken::Tab,
            ]]
        );
    }

    fn compiled_space_colors(input: &str) -> (SpaceColors, Vec<String>) {
        let config: crate::config::Config = toml::from_str(input).expect("space colors");
        SpaceColors::compile(&config.ui.sidebar)
    }

    #[test]
    fn space_colors_use_the_first_matching_rule_then_the_default() {
        use ratatui::style::Color;
        let (colors, diagnostics) = compiled_space_colors(
            r##"
[ui.sidebar]
space_color_default = "#f0883e"

[[ui.sidebar.space_colors]]
cwd = '(?i)[\\/]work[\\/]prod-'
color = "#f85149"

[[ui.sidebar.space_colors]]
cwd = '[\\/]work[\\/]'
color = "rgb(1, 2, 3)"

[[ui.sidebar.space_colors]]
cwd = 'side'
color = "magenta"
"##,
        );
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        for (cwd, expected) in [
            (Some("C:\\Work\\PROD-api"), Color::Rgb(0xf8, 0x51, 0x49)),
            (Some("/home/me/work/prod-api"), Color::Rgb(0xf8, 0x51, 0x49)),
            (Some("/home/me/work/api"), Color::Rgb(1, 2, 3)),
            (Some("/home/me/side-project"), Color::Magenta),
            (Some("/tmp"), Color::Rgb(0xf0, 0x88, 0x3e)),
            (None, Color::Rgb(0xf0, 0x88, 0x3e)),
        ] {
            assert_eq!(colors.color_for(cwd), Some(expected), "{cwd:?}");
        }

        let (unset, diagnostics) = compiled_space_colors("[ui.sidebar]\n");
        assert!(diagnostics.is_empty());
        assert_eq!(unset.color_for(Some("/tmp")), None);
    }

    #[test]
    fn invalid_space_colors_are_diagnosed_and_skipped() {
        use ratatui::style::Color;
        let input = r##"
[ui.sidebar]
space_color_default = "not-a-color"

[[ui.sidebar.space_colors]]
cwd = '(unclosed'
color = "#f85149"

[[ui.sidebar.space_colors]]
cwd = 'prod'
color = "#12"

[[ui.sidebar.space_colors]]
color = "red"

[[ui.sidebar.space_colors]]
cwd = 'prod'
color = "green"
"##;
        let (colors, diagnostics) = compiled_space_colors(input);
        assert_eq!(diagnostics.len(), 4, "{diagnostics:?}");
        assert!(diagnostics[0].starts_with("ui.sidebar.space_color_default"));
        assert!(diagnostics[1].starts_with("ui.sidebar.space_colors[0].cwd"));
        assert!(diagnostics[1].contains("not a valid regex"));
        assert!(!diagnostics[1].contains('\n'));
        assert!(diagnostics[2].starts_with("ui.sidebar.space_colors[1].color"));
        assert!(diagnostics[3].starts_with("ui.sidebar.space_colors[2].cwd is empty"));
        assert_eq!(colors.color_for(Some("/work/prod")), Some(Color::Green));
        assert_eq!(colors.color_for(Some("/work/other")), None);

        // `herdr config check` reports the same problems.
        let config: crate::config::Config = toml::from_str(input).unwrap();
        let reported = config.collect_diagnostics();
        for diagnostic in &diagnostics {
            assert!(reported.contains(diagnostic), "{diagnostic}");
        }
    }

    #[test]
    fn parses_builtin_and_arbitrary_custom_tokens() {
        let config: crate::config::Config = toml::from_str(
            r#"
[ui.sidebar.agents]
rows = [["state_icon", "workspace"], ["state_text", "agent", "$summary"], ["terminal_title", "terminal_title_stripped", "$terminal_title"], ["session_name"]]
row_gap = 1

[ui.sidebar.agents.rows_by_agent]
claude = [["terminal_title_stripped"], ["agent", "$model"]]

[ui.sidebar.spaces]
rows = [["workspace"], ["$jj_status"]]
row_gap = 3
"#,
        )
        .expect("sidebar token config");

        assert_eq!(
            config.ui.sidebar.agents.rows[1],
            vec![
                AgentSidebarToken::StateText,
                AgentSidebarToken::Agent,
                AgentSidebarToken::Custom("summary".into()),
            ]
        );
        assert_eq!(
            config.ui.sidebar.agents.rows[2],
            vec![
                AgentSidebarToken::TerminalTitle,
                AgentSidebarToken::TerminalTitleStripped,
                AgentSidebarToken::Custom("terminal_title".into()),
            ]
        );
        assert_eq!(
            config.ui.sidebar.agents.rows[3],
            vec![AgentSidebarToken::SessionName]
        );
        assert_eq!(
            config.ui.sidebar.agents.rows_by_agent["claude"],
            vec![
                vec![AgentSidebarToken::TerminalTitleStripped],
                vec![
                    AgentSidebarToken::Agent,
                    AgentSidebarToken::Custom("model".into()),
                ],
            ]
        );
        assert_eq!(config.ui.sidebar.agents.row_gap, 1);
        assert_eq!(
            config.ui.sidebar.spaces.rows[1],
            vec![SpaceSidebarToken::Custom("jj_status".into())]
        );
        assert_eq!(config.ui.sidebar.spaces.row_gap, 3);
    }

    #[test]
    fn parses_occurrence_styles_without_changing_plain_tokens() {
        let config: crate::config::Config = toml::from_str(
            r##"
[ui.sidebar.agents]
rows = [[{ token = "workspace", fg = "#abc", bold = false }, "workspace"], [{ token = "$summary", dim = false }]]

[ui.sidebar.agents.rows_by_agent]
claude = [[{ token = "agent", fg = "#112233", bold = true, dim = false }]]

[ui.sidebar.spaces]
rows = [[{ token = "git_status", fg = "#ff00aa" }], [{ token = "$jj", bold = true }]]
"##,
        )
        .unwrap();

        let (token, style) = config.ui.sidebar.agents.rows[0][0].parts();
        assert_eq!(token, &AgentSidebarToken::Workspace);
        assert_eq!(style.bold, Some(false));
        assert_eq!(
            style.fg.unwrap().ratatui(),
            ratatui::style::Color::Rgb(0xaa, 0xbb, 0xcc)
        );
        assert_eq!(
            config.ui.sidebar.agents.rows[0][1],
            AgentSidebarToken::Workspace
        );

        let (token, style) = config.ui.sidebar.agents.rows_by_agent["claude"][0][0].parts();
        assert_eq!(token, &AgentSidebarToken::Agent);
        assert_eq!(style.bold, Some(true));
        assert_eq!(style.dim, Some(false));

        let (token, style) = config.ui.sidebar.spaces.rows[0][0].parts();
        assert_eq!(token, &SpaceSidebarToken::GitStatus);
        assert_eq!(
            style.fg.unwrap().ratatui(),
            ratatui::style::Color::Rgb(0xff, 0x00, 0xaa)
        );
        let (token, style) = config.ui.sidebar.spaces.rows[1][0].parts();
        assert_eq!(token, &SpaceSidebarToken::Custom("jj".into()));
        assert_eq!(style.bold, Some(true));
    }

    #[test]
    fn conditional_sidebar_rules_round_trip() {
        let input = r##"
[agents]
rows = [[{ token = "machine", fg = "#fff", rules = [{ equals = "Local", fg = "#f00" }, { starts_with = "fed", ignore_case = true, bold = true }] }]]
[agents.rows_by_agent]
pi = [[{ token = "$load", rules = [{ gt = 80, dim = false }, { lt = 20.5, dim = true }] }]]
[spaces]
rows = [[{ token = "$status", rules = [{ contains = "error", bold = true }] }]]
"##;
        let config: SidebarConfig = toml::from_str(input).expect("conditional sidebar config");
        let encoded = toml::to_string(&config).unwrap();
        assert!(encoded.contains("rules"));
        assert_eq!(toml::from_str::<SidebarConfig>(&encoded).unwrap(), config);
    }

    #[test]
    fn conditional_sidebar_rules_reject_invalid_conditions_and_nontext_tokens() {
        for rule in [
            "{ bold = true }",
            "{ equals = 'x', contains = 'x' }",
            "{ regex = 'x' }",
            "{ gt = '80' }",
            "{ equals = 80 }",
            "{ gt = nan }",
            "{ lt = inf }",
            "{ gt = 80, ignore_case = false }",
            "{ equals = 'x', underline = true }",
            "{ equals = 'x', fg = 'red' }",
        ] {
            let input = format!("[agents]\nrows = [[{{ token = 'machine', rules = [{rule}] }}]]");
            assert!(toml::from_str::<SidebarConfig>(&input).is_err(), "{rule}");
        }
        for (section, token) in [
            ("agents", "state_icon"),
            ("spaces", "state_icon"),
            ("spaces", "git_status"),
        ] {
            let input = format!(
                "[{section}]\nrows = [[{{ token = '{token}', rules = [{{ equals = 'x' }}] }}]]"
            );
            assert!(toml::from_str::<SidebarConfig>(&input).is_err());
        }
        for count in [16, 17] {
            let rules = std::iter::repeat_n("{ equals = 'x' }", count)
                .collect::<Vec<_>>()
                .join(",");
            let input = format!("[agents]\nrows = [[{{ token = 'machine', rules = [{rules}] }}]]");
            assert_eq!(toml::from_str::<SidebarConfig>(&input).is_ok(), count == 16);
        }
    }

    #[test]
    fn rejects_invalid_occurrence_styles() {
        for entry in [
            r##"{ token = "workspace", fg = "red" }"##,
            r##"{ token = "workspace", fg = "#abcd" }"##,
            r##"{ token = "workspace", underline = true }"##,
        ] {
            let input = format!("[ui.sidebar.agents]\nrows = [[{entry}]]\n");
            assert!(
                toml::from_str::<crate::config::Config>(&input).is_err(),
                "accepted {entry}"
            );
        }
    }

    #[test]
    fn rejects_unknown_bare_and_malformed_custom_tokens() {
        for token in ["summary", "$", "$bad.name"] {
            let input = format!("[ui.sidebar.agents]\\nrows = [[\"{token}\"]]\\n");
            assert!(toml::from_str::<crate::config::Config>(&input).is_err());
        }
    }

    #[test]
    fn rejects_oversized_sidebar_layouts() {
        let too_many_rows = std::iter::repeat_n("[\"agent\"]", MAX_SIDEBAR_ROWS + 1)
            .collect::<Vec<_>>()
            .join(",");
        let input = format!("[ui.sidebar.agents]\nrows = [{too_many_rows}]\n");
        assert!(toml::from_str::<crate::config::Config>(&input).is_err());

        let too_many_tokens = std::iter::repeat_n("\"workspace\"", MAX_SIDEBAR_TOKENS_PER_ROW + 1)
            .collect::<Vec<_>>()
            .join(",");
        let input = format!("[ui.sidebar.spaces]\nrows = [[{too_many_tokens}]]\n");
        assert!(toml::from_str::<crate::config::Config>(&input).is_err());

        let input = format!("[ui.sidebar.agents.rows_by_agent]\nclaude = [{too_many_rows}]\n");
        assert!(toml::from_str::<crate::config::Config>(&input).is_err());
    }

    #[test]
    fn accepts_every_canonical_agent_override_key() {
        let agents = Agent::ALL;
        let entries = agents
            .iter()
            .map(|agent| format!("{} = [[\"agent\"]]", crate::detect::agent_label(*agent)))
            .collect::<Vec<_>>()
            .join("\n");
        let input = format!("[ui.sidebar.agents.rows_by_agent]\n{entries}\n");
        let config: crate::config::Config = toml::from_str(&input).expect("canonical keys");

        assert_eq!(config.ui.sidebar.agents.rows_by_agent.len(), agents.len());
    }

    #[test]
    fn rejects_alias_case_whitespace_and_unknown_override_keys() {
        for key in ["claude-code", "Claude", "' claude '", "unknown"] {
            let input = format!("[ui.sidebar.agents.rows_by_agent]\n{key} = [[\"agent\"]]\n");
            assert!(
                toml::from_str::<crate::config::Config>(&input).is_err(),
                "accepted key {key:?}"
            );
        }
    }
}
