//! Notification log section of the expanded sidebar.
//!
//! The endpoint owns the log. This client draws it, keeps the highlighted entry
//! and scroll offset, and changes entries only through endpoint methods.

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Widget},
};

use super::*;
use crate::api::schema::{
    NotificationAcceptParams, NotificationActionState, NotificationLogEntry,
    NotificationSelectParams,
};

/// Rows above the list: the rule, the header, and a blank row.
const HEADER_ROWS: u16 = 3;

pub(super) struct NotificationLogHit {
    pub(super) rect: Rect,
    pub(super) id: String,
}

/// One entry as the sidebar draws it.
pub(super) struct NotificationLogRow<'a> {
    pub(super) entry: &'a NotificationLogEntry,
    /// Linked workspace label and body, when either exists.
    pub(super) detail: Option<String>,
    /// The entry links to a workspace that no longer exists.
    pub(super) stale: bool,
}

impl NotificationLogRow<'_> {
    pub(super) fn height(&self) -> u16 {
        1 + u16::from(self.detail.is_some())
    }
}

/// Entries the endpoint sent for the boot that `snapshot` describes.
pub(super) fn endpoint_entries<'a>(
    endpoint: &'a ClientShellEndpoint,
    snapshot: &ClientShellSnapshot,
) -> &'a [NotificationLogEntry] {
    endpoint
        .notification_log
        .as_ref()
        .filter(|(_, log)| log.boot_id == snapshot.boot_id)
        .map_or(&[], |(_, log)| log.entries.as_slice())
}

pub(super) fn log_rows<'a>(
    entries: &'a [NotificationLogEntry],
    snapshot: &ClientShellSnapshot,
) -> Vec<NotificationLogRow<'a>> {
    entries
        .iter()
        .map(|entry| {
            let workspace = entry.workspace_id.as_deref().map(|workspace_id| {
                snapshot
                    .workspaces
                    .iter()
                    .find(|workspace| workspace.workspace_id == workspace_id)
            });
            let parts = [
                workspace
                    .flatten()
                    .map(|workspace| workspace.label.as_str()),
                entry.body.as_deref(),
            ];
            let detail = parts.into_iter().flatten().collect::<Vec<_>>().join(" · ");
            NotificationLogRow {
                entry,
                detail: (!detail.is_empty()).then_some(detail),
                stale: matches!(workspace, Some(None)),
            }
        })
        .collect()
}

fn row_heights(rows: &[NotificationLogRow<'_>]) -> (Vec<u16>, Vec<u16>) {
    (
        rows.iter().map(NotificationLogRow::height).collect(),
        vec![0; rows.len()],
    )
}

pub(super) fn render_notification_log(
    buffer: &mut Buffer,
    area: Rect,
    snapshot: &ClientShellSnapshot,
    entries: &[NotificationLogEntry],
    config: &ClientShellConfig,
    scroll: &mut usize,
    selected_id: Option<&str>,
    hits: &mut ShellHitMap,
) {
    let palette = &config.palette;
    if area.height == 0 {
        return;
    }
    super::render::put_text(
        buffer,
        area.x,
        area.y,
        area.width,
        &"─".repeat(area.width as usize),
        Style::default().fg(palette.surface_dim),
    );
    if area.height < 2 {
        return;
    }
    const TITLE: &str = " notifications";
    super::render::put_text(
        buffer,
        area.x,
        area.y + 1,
        area.width,
        TITLE,
        super::render::section_header_style(palette),
    );
    let unread = entries.iter().filter(|entry| !entry.read).count();
    if unread > 0 {
        // A narrow sidebar keeps the bare count so the title stays readable.
        let mut count = format!("{unread} new");
        if super::render::display_width(TITLE) + 1 + super::render::display_width(&count)
            > area.width
        {
            count = unread.to_string();
        }
        super::render::put_right_text(
            buffer,
            area,
            area.y + 1,
            &count,
            Style::default()
                .fg(palette.accent)
                .add_modifier(Modifier::BOLD),
        );
    }

    let body = Rect::new(
        area.x,
        area.y.saturating_add(HEADER_ROWS),
        area.width,
        area.height.saturating_sub(HEADER_ROWS),
    );
    hits.notification_log_body = body;
    if body.is_empty() || entries.is_empty() {
        *scroll = 0;
        if !body.is_empty() {
            super::render::put_text(
                buffer,
                body.x,
                body.y,
                body.width,
                " no notifications",
                Style::default()
                    .fg(palette.overlay0)
                    .add_modifier(Modifier::DIM),
            );
        }
        return;
    }

    let rows = log_rows(entries, snapshot);
    let (heights, gaps) = row_heights(&rows);
    let metrics = super::scroll::list_scroll_metrics(&heights, &gaps, body.height, *scroll);
    hits.notification_log_max_scroll = metrics.max_offset_from_bottom;
    hits.notification_log_scroll_metrics = Some(metrics);
    *scroll = metrics
        .max_offset_from_bottom
        .saturating_sub(metrics.offset_from_bottom);
    let show_scrollbar = metrics.max_offset_from_bottom > 0 && body.width > 1;
    let content_width = body.width.saturating_sub(u16::from(show_scrollbar));
    let selection_bg = if palette.selection_bg == ratatui::style::Color::Reset {
        palette.active_row_bg
    } else {
        palette.selection_bg
    };
    let mut y = body.y;
    for (index, row) in rows.iter().enumerate().skip(*scroll) {
        let height = heights[index].min(body.height);
        if y.saturating_add(height) > body.bottom() {
            break;
        }
        let rect = Rect::new(body.x, y, content_width, height);
        let selected = selected_id == Some(row.entry.id.as_str());
        render_row(buffer, rect, row, selected.then_some(selection_bg), palette);
        hits.notification_log.push(NotificationLogHit {
            rect,
            id: row.entry.id.clone(),
        });
        y = y.saturating_add(height);
    }

    if show_scrollbar {
        let track = Rect::new(body.right().saturating_sub(1), body.y, 1, body.height);
        hits.notification_log_scrollbar = track;
        super::scroll::render_list_scrollbar(buffer, track, metrics, palette);
    }
}

fn render_row(
    buffer: &mut Buffer,
    rect: Rect,
    row: &NotificationLogRow<'_>,
    background: Option<ratatui::style::Color>,
    palette: &Palette,
) {
    let unread = !row.entry.read;
    let dim = |style: Style| {
        if row.stale {
            style.fg(palette.overlay0).add_modifier(Modifier::DIM)
        } else {
            style
        }
    };
    let title_style = dim(if unread {
        Style::default()
            .fg(palette.text)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(palette.subtext0)
    });
    let marker = if unread { "●" } else { " " };
    let width = usize::from(rect.width);
    let badge = action_badge(row.entry, palette);
    // The badge keeps one space after the title and one at the right edge.
    let badge_width = badge.as_ref().map_or(0, |(text, _)| {
        usize::from(super::render::display_width(text)) + 2
    });
    let title = Line::from(vec![
        Span::raw(" "),
        Span::styled(marker, dim(Style::default().fg(palette.accent))),
        Span::raw(" "),
        Span::styled(
            crate::ui::truncate_end(&row.entry.title, width.saturating_sub(3 + badge_width)),
            title_style,
        ),
    ]);
    let row_style = background.map_or_else(Style::default, |bg| Style::default().bg(bg));
    Paragraph::new(title)
        .style(row_style)
        .render(Rect::new(rect.x, rect.y, rect.width, 1), buffer);
    if let Some((text, style)) = badge.filter(|_| width > 3 + badge_width) {
        let area = Rect::new(rect.x, rect.y, rect.width.saturating_sub(1), 1);
        super::render::put_right_text(buffer, area, rect.y, &text, dim(style.patch(row_style)));
    }
    if let Some(detail) = row.detail.as_deref().filter(|_| rect.height > 1) {
        let detail = Line::from(vec![
            Span::raw("   "),
            Span::styled(
                crate::ui::truncate_end(detail, width.saturating_sub(3)),
                dim(Style::default().fg(palette.overlay0)),
            ),
        ]);
        Paragraph::new(detail)
            .style(row_style)
            .render(Rect::new(rect.x, rect.y + 1, rect.width, 1), buffer);
    }
}

/// `[label]` while the action waits, `✓ label` once it ran.
fn action_badge(entry: &NotificationLogEntry, palette: &Palette) -> Option<(String, Style)> {
    let action = entry.action.as_ref()?;
    match entry.action_state? {
        NotificationActionState::Pending => Some((
            format!("[{}]", action.label()),
            Style::default()
                .fg(palette.accent)
                .add_modifier(Modifier::BOLD),
        )),
        NotificationActionState::Accepted => Some((
            format!("✓ {}", action.label()),
            Style::default().fg(palette.overlay0),
        )),
    }
}

impl ClientShellState {
    /// The active endpoint's log entries for the current snapshot.
    pub(super) fn notification_entries(&self) -> &[NotificationLogEntry] {
        let Some(snapshot) = self.snapshot.as_deref() else {
            return &[];
        };
        self.endpoints
            .iter()
            .find(|endpoint| endpoint.endpoint_id == self.active_endpoint_id)
            .map_or(&[], |endpoint| endpoint_entries(endpoint, snapshot))
    }

    /// Focuses the entry's workspace when it still exists and marks the entry read.
    pub(super) fn open_notification(&mut self, id: &str, outcome: &mut ClientShellInput) {
        let Some(entry) = self
            .notification_entries()
            .iter()
            .find(|entry| entry.id == id)
            .cloned()
        else {
            return;
        };
        self.selected_notification_id = Some(entry.id.clone());
        outcome.repaint = true;
        let workspace_id = entry.workspace_id.filter(|workspace_id| {
            self.snapshot.as_deref().is_some_and(|snapshot| {
                snapshot
                    .workspaces
                    .iter()
                    .any(|workspace| &workspace.workspace_id == workspace_id)
            })
        });
        if let Some(workspace_id) = workspace_id {
            self.reveal_workspace(&workspace_id);
            self.push_endpoint_method(
                crate::api::schema::Method::WorkspaceFocus(crate::api::schema::WorkspaceTarget {
                    workspace_id,
                }),
                outcome,
            );
        }
        if !entry.read {
            self.push_endpoint_method(
                crate::api::schema::Method::NotificationMarkRead(NotificationSelectParams {
                    id: Some(entry.id),
                    source: None,
                }),
                outcome,
            );
        }
    }

    /// Opens the next or previous entry, wrapping like agents mode.
    pub(super) fn open_adjacent_notification(
        &mut self,
        toward_end: bool,
        outcome: &mut ClientShellInput,
    ) {
        let entries = self.notification_entries();
        if entries.is_empty() {
            return;
        }
        let current = self
            .selected_notification_id
            .as_deref()
            .and_then(|id| entries.iter().position(|entry| entry.id == id));
        let next = match (current, toward_end) {
            (Some(current), true) => (current + 1) % entries.len(),
            (Some(current), false) => (current + entries.len() - 1) % entries.len(),
            (None, true) => 0,
            (None, false) => entries.len() - 1,
        };
        let id = entries[next].id.clone();
        self.reveal_notification(next);
        self.open_notification(&id, outcome);
    }

    /// A click opens the entry as `j`/`k` do, then asks before running a pending action.
    pub(super) fn click_notification(&mut self, id: &str, outcome: &mut ClientShellInput) {
        self.open_notification(id, outcome);
        let Some(entry) = self
            .notification_entries()
            .iter()
            .find(|entry| entry.id == id)
        else {
            return;
        };
        let Some(action) = entry.pending_action() else {
            return;
        };
        let title = format!("{}: {}?", action.label(), entry.title);
        let detail = entry.body.clone().unwrap_or_default();
        self.overlay = Some(ClientShellOverlay::ConfirmClose(
            ClientConfirmCloseOverlay {
                workspace_id: entry.workspace_id.clone().unwrap_or_default(),
                close_group: false,
                tab_target: None,
                title,
                detail,
                notification_id: Some(entry.id.clone()),
            },
        ));
        outcome.repaint = true;
    }

    /// Runs the highlighted entry's pending action without asking.
    pub(super) fn accept_selected_notification(&mut self, outcome: &mut ClientShellInput) {
        let Some(id) = self.selected_notification_id.clone().filter(|id| {
            self.notification_entries()
                .iter()
                .any(|entry| entry.id == *id && entry.pending_action().is_some())
        }) else {
            return;
        };
        self.accept_notification(id, outcome);
    }

    pub(super) fn accept_notification(&mut self, id: String, outcome: &mut ClientShellInput) {
        self.push_endpoint_method_with_kind(
            crate::api::schema::Method::NotificationAccept(NotificationAcceptParams { id }),
            PendingEndpointKind::Generic,
            outcome,
        );
        outcome.repaint = true;
    }

    /// Removes the highlighted entry and highlights the one that takes its place.
    pub(super) fn dismiss_selected_notification(&mut self, outcome: &mut ClientShellInput) {
        let entries = self.notification_entries();
        let Some(position) = self
            .selected_notification_id
            .as_deref()
            .and_then(|id| entries.iter().position(|entry| entry.id == id))
        else {
            return;
        };
        let id = entries[position].id.clone();
        let next = entries
            .get(position + 1)
            .or_else(|| position.checked_sub(1).and_then(|index| entries.get(index)))
            .map(|entry| entry.id.clone());
        if self.push_endpoint_method_with_kind(
            crate::api::schema::Method::NotificationDismiss(NotificationSelectParams {
                id: Some(id),
                source: None,
            }),
            PendingEndpointKind::Generic,
            outcome,
        ) {
            self.selected_notification_id = next;
        }
        outcome.repaint = true;
    }

    /// Scrolls the log so the entry at `position` is visible on the next render.
    fn reveal_notification(&mut self, position: usize) {
        let Some(snapshot) = self.snapshot.as_deref() else {
            return;
        };
        let rows = log_rows(self.notification_entries(), snapshot);
        let (heights, gaps) = row_heights(&rows);
        self.notification_scroll = super::scroll::list_scroll_start_to_reveal(
            &heights,
            &gaps,
            self.hits.notification_log_body.height,
            self.notification_scroll,
            position,
        );
    }
}
