use super::*;
use crate::api::schema::{
    Method, NotificationAction, NotificationActionState, NotificationLogEntry,
};
use crate::raw_input::RawInputEvent;

const CTRL_N: u8 = 0x0e;

fn log_config() -> Config {
    toml::from_str(
        "[ui.sidebar.notifications]\nenabled = true\n[keys]\nnotification_mode = \"ctrl+n\"\n",
    )
    .expect("notification log config")
}

fn entry(id: &str, title: &str, workspace_id: Option<&str>, read: bool) -> NotificationLogEntry {
    NotificationLogEntry {
        id: id.into(),
        title: title.into(),
        body: None,
        workspace_id: workspace_id.map(str::to_owned),
        source: Some("jobs".into()),
        created_unix_ms: 1,
        read,
        action: None,
        action_state: None,
    }
}

/// Two workspaces and a log, newest entry first.
fn shell_with_log(config: &Config, entries: Vec<NotificationLogEntry>) -> ClientShellState {
    let mut snapshot = snapshot();
    let mut second = snapshot.workspaces[0].clone();
    second.workspace_id = "ws_2".into();
    second.number = 2;
    second.label = "builds".into();
    second.focused = false;
    snapshot.workspaces.push(second);
    let mut state = ClientShellState::new(ClientShellConfig::from_config(config));
    state.set_snapshot(Box::new(snapshot));
    state.set_pane_surface(surface());
    set_log(&mut state, 1, entries);
    state
}

fn set_log(state: &mut ClientShellState, revision: u64, entries: Vec<NotificationLogEntry>) {
    state.set_endpoint_notification_log(
        &ClientEndpointId::Local,
        None,
        crate::protocol::endpoint::EndpointNotificationLog {
            boot_id: "boot-1".into(),
            revision,
            entries,
        },
    );
}

fn sample_entries() -> Vec<NotificationLogEntry> {
    vec![
        entry("n3", "finished deploy", Some("ws_2"), false),
        entry("n2", "started deploy", Some("ws_2"), true),
        entry("n1", "workspace gone", Some("ws_gone"), false),
    ]
}

fn compose_rows(state: &mut ClientShellState, rows: u16) -> (FrameData, Vec<String>) {
    let frame = state.compose(106, rows).expect("frame");
    let text = frame_rows(&frame);
    (FrameData::clone(&frame), text)
}

fn click(state: &mut ClientShellState, rect: Rect) -> ClientShellInput {
    state.handle_raw_events(vec![RawInputEvent::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: rect.x + 2,
        row: rect.y,
        modifiers: KeyModifiers::empty(),
    })])
}

fn methods(outcome: &ClientShellInput) -> Vec<&Method> {
    outcome
        .actions
        .iter()
        .filter_map(|action| match action {
            ClientShellAction::Endpoint { request, .. } => Some(&request.method),
            _ => None,
        })
        .collect()
}

fn hit(state: &ClientShellState, id: &str) -> Rect {
    state
        .hits
        .notification_log
        .iter()
        .find(|hit| hit.id == id)
        .unwrap_or_else(|| panic!("entry {id} is not visible"))
        .rect
}

#[test]
fn log_section_stays_hidden_until_enabled() {
    let mut state = shell_with_log(&Config::default(), sample_entries());
    let (_, rows) = compose_rows(&mut state, 30);

    assert!(
        !rows.iter().any(|row| row.contains("notifications")),
        "{rows:#?}"
    );
    assert!(state.hits.notification_log.is_empty());
    assert_eq!(state.hits.notification_log_body, Rect::default());
    // Spaces and agents keep the full height.
    assert_eq!(
        state.hits.sidebar_section_span.height,
        state.hits.sidebar_divider.height
    );

    // The mode key does nothing while the section is off.
    let config = toml::from_str::<Config>("[keys]\nnotification_mode = \"ctrl+n\"\n").unwrap();
    let mut state = shell_with_log(&config, sample_entries());
    state.handle_input_bytes(&[CTRL_N]);
    assert_eq!(state.mode, ClientShellMode::Terminal);
}

#[test]
fn enabled_log_takes_the_bottom_third_and_marks_unread_entries() {
    let mut state = shell_with_log(&log_config(), sample_entries());
    let (frame, rows) = compose_rows(&mut state, 30);

    let body = state.hits.notification_log_body;
    let span = state.hits.sidebar_section_span;
    let sidebar = state.hits.sidebar_divider;
    assert_eq!(span.y, sidebar.y);
    assert_eq!(span.height, sidebar.height - sidebar.height / 3);
    assert_eq!(body.y, span.bottom() + 3, "rule, header, blank row");
    assert!(rows[usize::from(span.bottom() + 1)].contains(" notifications"));
    assert!(rows[usize::from(span.bottom() + 1)].contains("2 new"));

    let row_text = |id: &str| {
        let rect = hit(&state, id);
        rows[usize::from(rect.y)]
            .chars()
            .skip(usize::from(rect.x))
            .take(usize::from(rect.width))
            .collect::<String>()
    };
    assert!(
        row_text("n3").starts_with(" ● finished deploy"),
        "{rows:#?}"
    );
    assert!(row_text("n2").starts_with("   started deploy"), "{rows:#?}");
    // Each entry names its workspace on a second line.
    let n3 = hit(&state, "n3");
    assert_eq!(n3.height, 2);
    assert!(rows[usize::from(n3.y + 1)].contains("builds"), "{rows:#?}");

    // An entry whose workspace closed is dimmed and has no workspace line.
    let gone = hit(&state, "n1");
    assert_eq!(gone.height, 1);
    let cell =
        &frame.cells[usize::from(gone.y) * usize::from(frame.width) + usize::from(gone.x) + 3];
    assert!(
        cell.modifier & crate::protocol::modifier_to_u16(Modifier::DIM) != 0,
        "stale entry should be dim"
    );
}

#[test]
fn clicking_an_entry_focuses_its_workspace_and_marks_it_read() {
    let mut state = shell_with_log(&log_config(), sample_entries());
    compose_rows(&mut state, 30);

    let rect = hit(&state, "n3");
    let outcome = click(&mut state, rect);
    let sent = methods(&outcome);
    assert_eq!(sent.len(), 2, "{sent:?}");
    assert!(
        matches!(sent[0], Method::WorkspaceFocus(target) if target.workspace_id == "ws_2"),
        "{sent:?}"
    );
    assert!(
        matches!(sent[1], Method::NotificationMarkRead(select)
            if select.id.as_deref() == Some("n3") && select.source.is_none()),
        "{sent:?}"
    );

    // A read entry only focuses.
    let rect = hit(&state, "n2");
    let outcome = click(&mut state, rect);
    let sent = methods(&outcome);
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert!(matches!(sent[0], Method::WorkspaceFocus(_)));

    // An entry for a closed workspace is marked read without a focus request.
    let rect = hit(&state, "n1");
    let outcome = click(&mut state, rect);
    let sent = methods(&outcome);
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert!(
        matches!(sent[0], Method::NotificationMarkRead(select) if select.id.as_deref() == Some("n1"))
    );
}

#[test]
fn notifications_mode_opens_entries_in_order_and_dismisses_the_selected_one() {
    let mut state = shell_with_log(&log_config(), sample_entries());
    compose_rows(&mut state, 30);

    state.handle_input_bytes(&[CTRL_N]);
    assert_eq!(state.mode, ClientShellMode::Scope(ScopeMode::Notification));
    let (_, rows) = compose_rows(&mut state, 30);
    let bar = rows.join("\n");
    for hint in ["NOTIFICATIONS", "j/k ↑/↓ open", "x dismiss", "esc done"] {
        assert!(bar.contains(hint), "missing {hint:?} in {bar}");
    }
    // Nothing is selected yet, so x has nothing to dismiss.
    assert!(state.handle_input_bytes(b"x").actions.is_empty());

    // j opens the newest entry.
    let first = state.handle_input_bytes(b"j");
    assert!(matches!(
        methods(&first)[..],
        [Method::WorkspaceFocus(_), Method::NotificationMarkRead(_)]
    ));
    assert_eq!(state.selected_notification_id.as_deref(), Some("n3"));
    assert_eq!(state.mode, ClientShellMode::Scope(ScopeMode::Notification));

    // The selection is highlighted.
    let selection_bg = state.config.palette.selection_bg;
    let frame = state.compose(106, 30).expect("frame");
    let rect = hit(&state, "n3");
    let cell = &frame.cells[usize::from(rect.y) * usize::from(frame.width) + usize::from(rect.x)];
    assert_eq!(cell.bg, crate::protocol::color_to_u32(selection_bg));

    // k wraps from the first entry to the last.
    state.handle_input_bytes(b"k");
    assert_eq!(state.selected_notification_id.as_deref(), Some("n1"));
    state.handle_input_bytes(b"\x1b[B");
    assert_eq!(state.selected_notification_id.as_deref(), Some("n3"));

    let dismissed = state.handle_input_bytes(b"x");
    assert!(
        matches!(methods(&dismissed)[..], [Method::NotificationDismiss(select)]
            if select.id.as_deref() == Some("n3") && select.source.is_none()),
        "{:?}",
        dismissed.actions
    );
    // The entry that takes its place is highlighted, and the mode stays open.
    assert_eq!(state.selected_notification_id.as_deref(), Some("n2"));
    assert_eq!(state.mode, ClientShellMode::Scope(ScopeMode::Notification));

    state.handle_input_bytes(b"\x1b");
    assert_eq!(state.mode, ClientShellMode::Terminal);
}

#[test]
fn moving_to_an_entry_out_of_view_scrolls_the_log() {
    let entries = (1..=20)
        .rev()
        .map(|number| entry(&format!("n{number}"), &format!("job {number}"), None, true))
        .collect::<Vec<_>>();
    let mut state = shell_with_log(&log_config(), entries);
    compose_rows(&mut state, 30);
    assert!(state.hits.notification_log_max_scroll > 0);
    assert!(state.hits.notification_log_scrollbar.height > 0);

    state.handle_input_bytes(&[CTRL_N]);
    state.handle_input_bytes(b"k");
    assert_eq!(state.selected_notification_id.as_deref(), Some("n1"));
    compose_rows(&mut state, 30);
    assert!(state.hits.notification_log.iter().any(|hit| hit.id == "n1"));

    // The wheel scrolls back to the top.
    let body = state.hits.notification_log_body;
    for _ in 0..30 {
        state.handle_raw_events(vec![RawInputEvent::Mouse(MouseEvent {
            kind: MouseEventKind::ScrollUp,
            column: body.x,
            row: body.y,
            modifiers: KeyModifiers::empty(),
        })]);
    }
    compose_rows(&mut state, 30);
    assert_eq!(state.notification_scroll, 0);
    assert!(state
        .hits
        .notification_log
        .iter()
        .any(|hit| hit.id == "n20"));
}

#[test]
fn a_new_boot_or_older_revision_does_not_show_stale_entries() {
    let mut state = shell_with_log(&log_config(), sample_entries());
    // An older revision from the same connection is ignored.
    set_log(&mut state, 0, Vec::new());
    assert_eq!(state.notification_entries().len(), 3);
    set_log(&mut state, 2, vec![entry("n9", "only", None, false)]);
    assert_eq!(state.notification_entries().len(), 1);

    // A log for another boot is not shown against this snapshot.
    state.set_endpoint_notification_log(
        &ClientEndpointId::Local,
        None,
        crate::protocol::endpoint::EndpointNotificationLog {
            boot_id: "boot-0".into(),
            revision: 7,
            entries: sample_entries(),
        },
    );
    assert!(state.notification_entries().is_empty());
    let (_, rows) = compose_rows(&mut state, 30);
    assert!(
        rows.iter().any(|row| row.contains("no notifications")),
        "{rows:#?}"
    );
}

#[test]
fn dragging_the_section_divider_resizes_only_spaces_and_agents() {
    let mut state = shell_with_log(&log_config(), sample_entries());
    compose_rows(&mut state, 30);
    let span = state.hits.sidebar_section_span;
    let divider = state.hits.sidebar_section_divider;
    let log_top = span.bottom();

    let mouse = |kind, row| {
        RawInputEvent::Mouse(MouseEvent {
            kind,
            column: divider.x + 1,
            row,
            modifiers: KeyModifiers::empty(),
        })
    };
    state.handle_raw_events(vec![
        mouse(MouseEventKind::Down(MouseButton::Left), divider.y),
        mouse(
            MouseEventKind::Drag(MouseButton::Left),
            span.y + span.height / 4,
        ),
        mouse(
            MouseEventKind::Up(MouseButton::Left),
            span.y + span.height / 4,
        ),
    ]);
    assert!((state.sidebar_section_split - 0.25).abs() < 0.05);
    compose_rows(&mut state, 30);
    assert_eq!(state.hits.sidebar_section_span.bottom(), log_top);
    assert!(state.hits.sidebar_section_divider.y < divider.y);
}

fn with_action(
    mut entry: NotificationLogEntry,
    label: &str,
    state: NotificationActionState,
) -> NotificationLogEntry {
    entry.action = Some(NotificationAction {
        argv: vec!["run".into()],
        label: Some(label.into()),
    });
    entry.action_state = Some(state);
    entry
}

fn action_entries() -> Vec<NotificationLogEntry> {
    vec![
        with_action(
            entry("n3", "finished deploy", Some("ws_2"), false),
            "deploy",
            NotificationActionState::Pending,
        ),
        with_action(
            entry("n2", "ran migration", None, true),
            "migrate",
            NotificationActionState::Accepted,
        ),
        entry("n1", "plain entry", None, false),
    ]
}

fn row_text(state: &ClientShellState, rows: &[String], id: &str) -> String {
    let rect = hit(state, id);
    rows[usize::from(rect.y)]
        .chars()
        .skip(usize::from(rect.x))
        .take(usize::from(rect.width))
        .collect()
}

fn accepted_id(outcome: &ClientShellInput) -> Option<String> {
    match methods(outcome)[..] {
        [Method::NotificationAccept(params)] => Some(params.id.clone()),
        _ => None,
    }
}

#[test]
fn entries_show_whether_their_action_waits_or_ran() {
    let mut state = shell_with_log(&log_config(), action_entries());
    let (_, rows) = compose_rows(&mut state, 30);

    let pending = row_text(&state, &rows, "n3");
    assert!(pending.starts_with(" ● finished"), "{pending:?}");
    assert!(pending.trim_end().ends_with("[deploy]"), "{pending:?}");
    let accepted = row_text(&state, &rows, "n2");
    assert!(accepted.trim_end().ends_with("✓ migrate"), "{accepted:?}");
    let plain = row_text(&state, &rows, "n1");
    assert!(!plain.contains('[') && !plain.contains('✓'), "{plain:?}");
}

#[test]
fn notifications_mode_a_runs_the_selected_pending_action_without_asking() {
    let mut state = shell_with_log(&log_config(), action_entries());
    compose_rows(&mut state, 30);

    state.handle_input_bytes(&[CTRL_N]);
    let (_, rows) = compose_rows(&mut state, 30);
    assert!(rows.join("\n").contains("a accept"), "{rows:#?}");
    // Nothing is selected yet.
    assert!(state.handle_input_bytes(b"a").actions.is_empty());

    state.handle_input_bytes(b"j");
    let accepted = state.handle_input_bytes(b"a");
    assert_eq!(accepted_id(&accepted).as_deref(), Some("n3"));
    assert!(state.overlay.is_none());
    assert_eq!(state.mode, ClientShellMode::Scope(ScopeMode::Notification));

    // An accepted entry and an entry without an action have nothing to run.
    for id in ["n2", "n1"] {
        state.handle_input_bytes(b"j");
        assert_eq!(state.selected_notification_id.as_deref(), Some(id));
        let outcome = state.handle_input_bytes(b"a");
        assert!(outcome.actions.is_empty(), "{id}: {:?}", outcome.actions);
    }
}

#[test]
fn clicking_an_entry_with_a_pending_action_asks_before_running_it() {
    let mut state = shell_with_log(&log_config(), action_entries());
    compose_rows(&mut state, 30);

    let rect = hit(&state, "n3");
    let opened = click(&mut state, rect);
    // The click focuses the space and marks the entry read, as for any entry.
    assert!(matches!(
        methods(&opened)[..],
        [Method::WorkspaceFocus(_), Method::NotificationMarkRead(_)]
    ));
    assert!(matches!(
        state.overlay.as_ref(),
        Some(ClientShellOverlay::ConfirmClose(ClientConfirmCloseOverlay {
            notification_id: Some(id),
            title,
            ..
        })) if id == "n3" && title == "deploy: finished deploy?"
    ));
    let (_, rows) = compose_rows(&mut state, 30);
    let screen = rows.join("\n");
    assert!(screen.contains("deploy: finished deploy?"), "{screen}");
    assert!(screen.contains("↵ run"), "{screen}");

    // Esc and n cancel without leaving the terminal.
    for cancel in [&b"\x1b"[..], b"n"] {
        let outcome = state.handle_input_bytes(cancel);
        assert!(outcome.actions.is_empty(), "{cancel:?}");
        assert!(state.overlay.is_none(), "{cancel:?}");
        assert_eq!(state.mode, ClientShellMode::Terminal);
        click(&mut state, rect);
    }

    // Enter and y run the action.
    for run in [&b"\r"[..], b"y"] {
        let outcome = state.handle_input_bytes(run);
        assert_eq!(accepted_id(&outcome).as_deref(), Some("n3"), "{run:?}");
        assert!(state.overlay.is_none());
        click(&mut state, rect);
    }

    // So does the dialog's run button.
    compose_rows(&mut state, 30);
    let button = state.hits.overlay_primary;
    let outcome = click(&mut state, button);
    assert_eq!(accepted_id(&outcome).as_deref(), Some("n3"));
    assert!(state.overlay.is_none());

    // Entries whose action ran, or that have none, open without a dialog.
    for id in ["n2", "n1"] {
        compose_rows(&mut state, 30);
        let rect = hit(&state, id);
        let outcome = click(&mut state, rect);
        assert!(accepted_id(&outcome).is_none());
        assert!(state.overlay.is_none(), "{id}");
    }
}
