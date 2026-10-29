use std::collections::HashMap;

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    text::Line,
    widgets::{Paragraph, Widget},
};

use super::*;

pub(super) struct AgentRow {
    pub(super) pane_id: String,
    pub(super) workspace_id: String,
    pub(super) workspace_label: String,
    pub(super) status: crate::api::schema::AgentStatus,
    pub(super) focused: bool,
    pub(super) under_heading: bool,
    pub(super) rows: Vec<Vec<crate::ui::ResolvedToken>>,
}

/// One entry in the agents panel list.
pub(super) enum AgentListItem<T> {
    /// Names the space of the agents that follow it.
    Heading {
        label: String,
        current: bool,
    },
    Agent(T),
}

/// Whether agents appear under a heading for their space. Headings follow the
/// spaces order, so a priority sort or a custom agent view shows a flat list.
pub(super) fn groups_by_space(config: &ClientShellConfig, agent_view_label: Option<&str>) -> bool {
    config.agents.group_by_space
        && config.agent_panel_sort == crate::config::AgentPanelSortConfig::Spaces
        && agent_view_label.is_none()
}

/// Adds a heading wherever the space changes. `space` gives the key that
/// identifies a row's space, the heading text, and whether it is focused.
pub(super) fn with_space_headings<T, K: PartialEq>(
    rows: Vec<T>,
    space: impl Fn(&T) -> (K, String, bool),
) -> Vec<AgentListItem<T>> {
    let mut items = Vec::with_capacity(rows.len());
    let mut previous = None;
    for row in rows {
        let (key, label, current) = space(&row);
        if previous.as_ref() != Some(&key) {
            items.push(AgentListItem::Heading { label, current });
            previous = Some(key);
        }
        items.push(AgentListItem::Agent(row));
    }
    items
}

/// The list position that reveals the matching agent, starting at its space
/// heading when it is the first agent under one.
pub(super) fn agent_list_position<T>(
    items: &[AgentListItem<T>],
    matches: impl Fn(&T) -> bool,
) -> Option<usize> {
    let index = items
        .iter()
        .position(|item| matches!(item, AgentListItem::Agent(row) if matches(row)))?;
    Some(
        match index.checked_sub(1).map(|previous| &items[previous]) {
            Some(AgentListItem::Heading { .. }) => index - 1,
            _ => index,
        },
    )
}

/// Terminal rows each list item takes, and the blank rows after it.
pub(super) fn agent_list_heights<T>(
    items: &[AgentListItem<T>],
    row_gap: u16,
    row_lines: impl Fn(&T) -> usize,
) -> (Vec<u16>, Vec<u16>) {
    let heights = items
        .iter()
        .map(|item| match item {
            AgentListItem::Heading { .. } => 1,
            AgentListItem::Agent(row) => row_lines(row).max(1).min(u16::MAX as usize) as u16,
        })
        .collect();
    let gaps = items
        .iter()
        .enumerate()
        .map(|(index, item)| match item {
            _ if index + 1 == items.len() => 0,
            AgentListItem::Heading { .. } => 0,
            AgentListItem::Agent(_) => row_gap,
        })
        .collect();
    (heights, gaps)
}

pub(super) fn ordered_agent_pane_ids(
    snapshot: &ClientShellSnapshot,
    sort: crate::config::AgentPanelSortConfig,
) -> Vec<String> {
    if snapshot.agent_view_label.is_some() {
        return snapshot
            .agent_order
            .iter()
            .filter(|pane_id| {
                snapshot
                    .agents
                    .iter()
                    .any(|agent| agent.pane_id == pane_id.as_str())
            })
            .cloned()
            .collect();
    }
    let mut agents = snapshot.agents.iter().collect::<Vec<_>>();
    if sort == crate::config::AgentPanelSortConfig::Priority {
        agents.sort_by_key(|agent| {
            (
                std::cmp::Reverse(status_priority(agent.agent_status)),
                std::cmp::Reverse(agent.state_change_seq),
            )
        });
    }
    agents
        .into_iter()
        .map(|agent| agent.pane_id.clone())
        .collect()
}

pub(super) fn render_agent_panel(
    buffer: &mut Buffer,
    area: Rect,
    snapshot: &ClientShellSnapshot,
    config: &ClientShellConfig,
    agent_scroll: &mut usize,
    hits: &mut ShellHitMap,
) {
    if !render_agent_panel_header(
        buffer,
        area,
        snapshot.agent_view_label.as_deref(),
        config,
        hits,
    ) {
        return;
    }

    let rows = agent_list_items(snapshot, config);
    render_agent_list(
        buffer,
        area,
        &rows,
        snapshot
            .agent_view_label
            .as_ref()
            .map(|_| " no matching agents"),
        config,
        agent_scroll,
        hits,
        |row| row.rows.len(),
        |buffer, rect, row, hits| {
            hits.agents.push((rect, row.pane_id.clone()));
            render_agent_row(buffer, rect, row, config);
        },
    );
}

pub(super) fn render_agent_panel_header(
    buffer: &mut Buffer,
    area: Rect,
    agent_view_label: Option<&str>,
    config: &ClientShellConfig,
    hits: &mut ShellHitMap,
) -> bool {
    if area.height == 0 {
        return false;
    }
    put_text(
        buffer,
        area.x,
        area.y,
        area.width,
        &"─".repeat(area.width as usize),
        Style::default().fg(config.palette.surface_dim),
    );
    if area.height < 2 {
        return false;
    }
    put_text(
        buffer,
        area.x,
        area.y + 1,
        area.width,
        " agents",
        Style::default()
            .fg(config.palette.overlay0)
            .add_modifier(Modifier::BOLD),
    );
    let sort_label = agent_view_label.unwrap_or(match config.agent_panel_sort {
        crate::config::AgentPanelSortConfig::Spaces => "grouped",
        crate::config::AgentPanelSortConfig::Priority => "priority",
    });
    let sort_width = display_width(sort_label).min(area.width as usize) as u16;
    let sort_rect = Rect::new(
        area.right().saturating_sub(sort_width),
        area.y + 1,
        sort_width,
        1,
    );
    hits.agent_sort_toggle = if config.mouse_capture && agent_view_label.is_none() {
        sort_rect
    } else {
        Rect::default()
    };
    put_text(
        buffer,
        sort_rect.x,
        sort_rect.y,
        sort_rect.width,
        sort_label,
        Style::default()
            .fg(if agent_view_label.is_some() {
                config.palette.accent
            } else {
                config.palette.overlay0
            })
            .add_modifier(Modifier::BOLD),
    );
    true
}

pub(super) fn render_agent_list<T>(
    buffer: &mut Buffer,
    area: Rect,
    rows: &[AgentListItem<T>],
    empty_message: Option<&str>,
    config: &ClientShellConfig,
    agent_scroll: &mut usize,
    hits: &mut ShellHitMap,
    row_lines: impl Fn(&T) -> usize,
    mut render_row: impl FnMut(&mut Buffer, Rect, &T, &mut ShellHitMap),
) {
    let body = Rect::new(
        area.x,
        area.y.saturating_add(3),
        area.width,
        area.height.saturating_sub(3),
    );
    hits.agent_body = body;
    if body.is_empty() || rows.is_empty() {
        *agent_scroll = 0;
        if let Some(message) = empty_message.filter(|_| !body.is_empty()) {
            put_text(
                buffer,
                body.x,
                body.y,
                body.width,
                message,
                Style::default()
                    .fg(config.palette.overlay0)
                    .add_modifier(Modifier::DIM),
            );
        }
        return;
    }

    let (row_heights, gaps) = agent_list_heights(rows, config.agents.row_gap, row_lines);
    let metrics =
        super::scroll::list_scroll_metrics(&row_heights, &gaps, body.height, *agent_scroll);
    hits.agent_max_scroll = metrics.max_offset_from_bottom;
    hits.agent_scroll_metrics = Some(metrics);
    *agent_scroll = metrics
        .max_offset_from_bottom
        .saturating_sub(metrics.offset_from_bottom);
    let show_scrollbar = metrics.max_offset_from_bottom > 0 && body.width > 1;
    let content_width = body.width.saturating_sub(u16::from(show_scrollbar));
    let mut y = body.y;
    for (index, row) in rows.iter().enumerate().skip(*agent_scroll) {
        let height = row_heights[index].min(body.height);
        if y.saturating_add(height) > body.bottom() {
            break;
        }
        let rect = Rect::new(body.x, y, content_width, height);
        match row {
            AgentListItem::Heading { label, current } => {
                render_space_heading(buffer, rect, label, *current, config);
            }
            AgentListItem::Agent(row) => render_row(buffer, rect, row, hits),
        }
        y = y.saturating_add(height).saturating_add(gaps[index]);
    }

    if show_scrollbar {
        let track = Rect::new(body.right().saturating_sub(1), body.y, 1, body.height);
        hits.agent_scrollbar = track;
        super::scroll::render_list_scrollbar(buffer, track, metrics, &config.palette);
    }
}

pub(super) fn agent_list_items(
    snapshot: &ClientShellSnapshot,
    config: &ClientShellConfig,
) -> Vec<AgentListItem<AgentRow>> {
    let grouped = groups_by_space(config, snapshot.agent_view_label.as_deref());
    let rows = ordered_agent_pane_ids(snapshot, config.agent_panel_sort)
        .into_iter()
        .filter_map(|pane_id| agent_row(snapshot, &pane_id, config, None, grouped))
        .collect::<Vec<_>>();
    if !grouped {
        return rows.into_iter().map(AgentListItem::Agent).collect();
    }
    let focused_workspace_id = snapshot.focused_workspace_id.as_deref();
    with_space_headings(rows, |row| {
        (
            row.workspace_id.clone(),
            row.workspace_label.clone(),
            focused_workspace_id == Some(row.workspace_id.as_str()),
        )
    })
}

pub(super) fn agent_row(
    snapshot: &ClientShellSnapshot,
    pane_id: &str,
    config: &ClientShellConfig,
    machine: Option<&str>,
    under_heading: bool,
) -> Option<AgentRow> {
    let agent = snapshot
        .agents
        .iter()
        .find(|agent| agent.pane_id == pane_id)?;
    let workspace = snapshot
        .workspaces
        .iter()
        .find(|workspace| workspace.workspace_id == agent.workspace_id)?;
    let tab = snapshot.tabs.iter().find(|tab| tab.tab_id == agent.tab_id);
    let pane = snapshot
        .panes
        .iter()
        .find(|pane| pane.pane_id == agent.pane_id);
    let tab_count = snapshot
        .tabs
        .iter()
        .filter(|candidate| candidate.workspace_id == agent.workspace_id)
        .count();
    let tab_label = tab
        .filter(|tab| tab_count > 1 || tab.custom_label)
        .map(|tab| tab.label.as_str());
    let agent_label = agent
        .display_agent
        .as_deref()
        .or(agent.name.as_deref())
        .or(agent.agent.as_deref())
        .or(agent.title.as_deref());
    let labels = agent
        .state_labels
        .iter()
        .cloned()
        .collect::<HashMap<_, _>>();
    let tokens = agent.tokens.iter().cloned().collect::<HashMap<_, _>>();
    let state_text = labels
        .get(status_text(agent.agent_status))
        .map(String::as_str)
        .unwrap_or_else(|| sidebar_status_text(agent.agent_status));
    let canonical_agent = agent
        .agent
        .as_deref()
        .and_then(crate::detect::parse_agent_label);
    let rows = crate::ui::sidebar_agent_rows(
        &config.agents,
        crate::ui::AgentTokenContext {
            machine,
            workspace: &workspace.label,
            tab: tab_label,
            pane: agent
                .title
                .as_deref()
                .or_else(|| pane.and_then(|pane| pane.label.as_deref())),
            agent_label,
            terminal_title: agent.terminal_title.as_deref(),
            terminal_title_stripped: agent.terminal_title_stripped.as_deref(),
            canonical_agent,
            tokens: &tokens,
            under_heading,
        },
        state_text,
    );
    Some(AgentRow {
        pane_id: agent.pane_id.clone(),
        workspace_id: workspace.workspace_id.clone(),
        workspace_label: workspace.label.clone(),
        status: agent.agent_status,
        focused: agent.focused,
        under_heading,
        rows,
    })
}

pub(super) fn render_agent_row(
    buffer: &mut Buffer,
    rect: Rect,
    row: &AgentRow,
    config: &ClientShellConfig,
) {
    let palette = &config.palette;
    let row_style = if row.focused {
        Style::default().bg(palette.active_row_bg)
    } else {
        Style::default()
    };
    let name_style = if row.focused {
        Style::default()
            .fg(palette.text)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .fg(palette.subtext0)
            .add_modifier(Modifier::BOLD)
    };
    let status_style = Style::default().fg(status_color(row.status, palette));
    let secondary = Style::default().fg(palette.overlay0);
    let icon = (
        status_icon(row.status, config.status_indicators),
        Style::default().fg(status_color(row.status, palette)),
    );
    let rows = if row.rows.is_empty() {
        vec![vec![crate::ui::ResolvedToken {
            kind: crate::ui::ResolvedTokenKind::StateIcon,
            style: Default::default(),
        }]]
    } else {
        row.rows.clone()
    };
    let nested = usize::from(row.under_heading);
    for (index, tokens) in rows.iter().take(rect.height as usize).enumerate() {
        let indent = if index == 0 { 1 } else { 3 } + nested;
        let mut spans = vec![ratatui::text::Span::raw(" ".repeat(indent))];
        spans.extend(crate::ui::resolved_token_spans(
            tokens,
            icon,
            status_style,
            name_style,
            secondary,
            secondary,
            palette,
            rect.width.saturating_sub(indent as u16) as usize,
        ));
        Paragraph::new(Line::from(spans)).style(row_style).render(
            Rect::new(rect.x, rect.y + index as u16, rect.width, 1),
            buffer,
        );
    }
}

/// ` name ───────` across the row, brighter for the focused space.
fn render_space_heading(
    buffer: &mut Buffer,
    rect: Rect,
    label: &str,
    current: bool,
    config: &ClientShellConfig,
) {
    let palette = &config.palette;
    let width = usize::from(rect.width);
    let name = crate::ui::truncate_end(label, width.saturating_sub(3));
    let (rule_x, _) = buffer.set_stringn(
        rect.x,
        rect.y,
        format!(" {name} "),
        width,
        Style::default()
            .fg(if current {
                palette.text
            } else {
                palette.subtext0
            })
            .add_modifier(Modifier::BOLD),
    );
    let rule_width = rect.right().saturating_sub(rule_x);
    buffer.set_stringn(
        rule_x,
        rect.y,
        "─".repeat(usize::from(rule_width)),
        usize::from(rule_width),
        Style::default().fg(palette.surface_dim),
    );
}

fn put_text(buffer: &mut Buffer, x: u16, y: u16, width: u16, text: &str, style: Style) {
    for (offset, character) in text.chars().take(width as usize).enumerate() {
        if let Some(cell) = buffer.cell_mut((x + offset as u16, y)) {
            cell.set_char(character).set_style(style);
        }
    }
}

fn display_width(text: &str) -> usize {
    unicode_width::UnicodeWidthStr::width(text)
}

fn sidebar_status_text(status: crate::api::schema::AgentStatus) -> &'static str {
    use crate::api::schema::AgentStatus;
    match status {
        AgentStatus::Blocked => "blocked",
        AgentStatus::Done => "done",
        AgentStatus::Working => "working",
        AgentStatus::Idle | AgentStatus::Unknown => "idle",
    }
}
