use super::*;
use crate::api::schema::{Method, PaneDirection, SplitDirection};
use crate::input::TerminalKey;
use crate::raw_input::RawInputEvent;

const CTRL_P: u8 = 0x10;
const CTRL_T: u8 = 0x14;
const CTRL_S: u8 = 0x13;
const CTRL_A: u8 = 0x01;

fn shell(snapshot: ClientShellSnapshot) -> ClientShellState {
    shell_with_config(snapshot, &Config::default())
}

fn shell_with_config(snapshot: ClientShellSnapshot, config: &Config) -> ClientShellState {
    let mut state = ClientShellState::new(ClientShellConfig::from_config(config));
    state.set_snapshot(Box::new(snapshot));
    state.set_pane_surface(surface());
    state
}

fn key(state: &mut ClientShellState, code: KeyCode, modifiers: KeyModifiers) -> ClientShellInput {
    state.handle_raw_events(vec![RawInputEvent::Key(TerminalKey::new(code, modifiers))])
}

/// The single endpoint method a key sent; panics on anything else.
fn method(outcome: &ClientShellInput) -> &Method {
    let [ClientShellAction::Endpoint { request, .. }] = &outcome.actions[..] else {
        panic!("expected one endpoint request, got {:?}", outcome.actions);
    };
    &request.method
}

fn mode_bar_text(state: &mut ClientShellState) -> String {
    let frame = state.compose(140, 20).expect("mode frame");
    frame_rows(&frame).join("\n")
}

fn snapshot_with_tabs() -> ClientShellSnapshot {
    let mut snapshot = snapshot();
    let first = snapshot.tabs[0].clone();
    snapshot.tabs = (1..=3)
        .map(|number| ClientShellTab {
            tab_id: format!("tab_{number}"),
            number,
            label: number.to_string(),
            focused: number == 2,
            ..first.clone()
        })
        .collect();
    snapshot.focused_tab_id = Some("tab_2".into());
    snapshot.panes[0].tab_id = "tab_2".into();
    snapshot.workspaces[0].active_tab_id = "tab_2".into();
    snapshot
}

fn snapshot_with_workspaces() -> ClientShellSnapshot {
    let mut snapshot = snapshot();
    let first = snapshot.workspaces[0].clone();
    snapshot.workspaces = (1..=3)
        .map(|number| ClientShellWorkspace {
            workspace_id: format!("ws_{number}"),
            number,
            label: format!("space {number}"),
            focused: number == 1,
            ..first.clone()
        })
        .collect();
    snapshot
}

fn snapshot_with_agents() -> ClientShellSnapshot {
    let mut snapshot = snapshot();
    let mut second = snapshot.panes[0].clone();
    second.pane_id = "pane_2".into();
    second.focused = false;
    snapshot.panes.push(second);
    snapshot.agents = ["pane_1", "pane_2"]
        .into_iter()
        .enumerate()
        .map(|(index, pane_id)| ClientShellAgent {
            pane_id: pane_id.into(),
            workspace_id: "ws_1".into(),
            tab_id: "tab_1".into(),
            name: Some(format!("agent {index}")),
            display_agent: None,
            agent: None,
            title: None,
            terminal_title: None,
            terminal_title_stripped: None,
            agent_status: AgentStatus::Idle,
            state_change_seq: index as u64 + 1,
            state_labels: Vec::new(),
            tokens: Vec::new(),
            focused: index == 0,
        })
        .collect();
    snapshot
}

#[test]
fn mode_keys_open_switch_and_close_modes_without_reaching_the_pane() {
    let mut state = shell(snapshot());

    for (byte, scope) in [
        (CTRL_P, ScopeMode::Pane),
        (CTRL_T, ScopeMode::Tab),
        (CTRL_S, ScopeMode::Workspace),
        (CTRL_A, ScopeMode::Agent),
    ] {
        let opened = state.handle_input_bytes(&[byte]);
        assert!(opened.requests.is_empty(), "{scope:?} key reached the pane");
        assert!(opened.actions.is_empty());
        assert!(opened.repaint);
        assert_eq!(state.mode, ClientShellMode::Scope(scope));
    }

    // Another mode's key switches, the active mode's key closes.
    state.handle_input_bytes(&[CTRL_P]);
    assert_eq!(state.mode, ClientShellMode::Scope(ScopeMode::Pane));
    let closed = state.handle_input_bytes(&[CTRL_P]);
    assert!(closed.requests.is_empty());
    assert_eq!(state.mode, ClientShellMode::Terminal);

    for exit in [&b"\x1b"[..], b"\r"] {
        state.handle_input_bytes(&[CTRL_T]);
        let outcome = state.handle_input_bytes(exit);
        assert!(outcome.requests.is_empty() && outcome.actions.is_empty());
        assert_eq!(state.mode, ClientShellMode::Terminal, "{exit:?}");
    }

    // Unbound keys stay in the mode instead of typing into the pane.
    state.handle_input_bytes(&[CTRL_S]);
    let ignored = state.handle_input_bytes(b"z");
    assert!(ignored.requests.is_empty() && ignored.actions.is_empty());
    assert_eq!(state.mode, ClientShellMode::Scope(ScopeMode::Workspace));

    // The prefix still works from a mode.
    state.handle_input_bytes(&[0x02]);
    assert_eq!(state.mode, ClientShellMode::Prefix);
}

#[test]
fn pane_mode_moves_focus_and_panes_and_closes_after_a_split() {
    let mut state = shell(snapshot());
    state.handle_input_bytes(&[CTRL_P]);

    for (input, direction) in [
        (&b"h"[..], PaneDirection::Left),
        (b"j", PaneDirection::Down),
        (b"k", PaneDirection::Up),
        (b"l", PaneDirection::Right),
        (b"\x1b[D", PaneDirection::Left),
        (b"\x1b[B", PaneDirection::Down),
        (b"\x1b[A", PaneDirection::Up),
        (b"\x1b[C", PaneDirection::Right),
    ] {
        let outcome = state.handle_input_bytes(input);
        assert!(
            matches!(
                method(&outcome),
                Method::PaneFocusDirection(params)
                    if params.direction == direction
                        && params.pane_id.as_deref() == Some("pane_1")
            ),
            "{input:?}"
        );
        assert_eq!(state.mode, ClientShellMode::Scope(ScopeMode::Pane));
    }

    for (input, direction) in [
        (&b"H"[..], PaneDirection::Left),
        (b"J", PaneDirection::Down),
        (b"\x1b[1;2A", PaneDirection::Up),
        (b"\x1b[1;2C", PaneDirection::Right),
    ] {
        let outcome = state.handle_input_bytes(input);
        assert!(
            matches!(
                method(&outcome),
                Method::PaneSwap(params) if params.direction == Some(direction)
            ),
            "{input:?}"
        );
        assert_eq!(state.mode, ClientShellMode::Scope(ScopeMode::Pane));
    }
    // A host reporting Shift explicitly moves the pane too.
    let kitty_shift = key(&mut state, KeyCode::Char('l'), KeyModifiers::SHIFT);
    assert!(matches!(
        method(&kitty_shift),
        Method::PaneSwap(params) if params.direction == Some(PaneDirection::Right)
    ));

    let down = state.handle_input_bytes(b"d");
    assert!(matches!(
        method(&down),
        Method::PaneSplit(params) if params.direction == SplitDirection::Down && params.focus
    ));
    assert_eq!(state.mode, ClientShellMode::Terminal);

    state.handle_input_bytes(&[CTRL_P]);
    let right = state.handle_input_bytes(b"r");
    assert!(matches!(
        method(&right),
        Method::PaneSplit(params) if params.direction == SplitDirection::Right && params.focus
    ));
    assert_eq!(state.mode, ClientShellMode::Terminal);
}

#[test]
fn pane_mode_closes_the_focused_pane_and_stays_open() {
    // Two panes in one tab, so closing one needs no confirmation.
    let mut state = shell(snapshot_with_agents());
    state.handle_input_bytes(&[CTRL_P]);

    // Panes have no rename key, and v is not a split key.
    let ignored = state.handle_input_bytes(b"v");
    assert!(ignored.actions.is_empty() && ignored.requests.is_empty());
    assert!(state.overlay.is_none());
    assert_eq!(state.mode, ClientShellMode::Scope(ScopeMode::Pane));

    let close = state.handle_input_bytes(b"x");
    assert!(matches!(
        method(&close),
        Method::PaneClose(target) if target.pane_id == "pane_1"
    ));
    assert_eq!(state.mode, ClientShellMode::Scope(ScopeMode::Pane));
}

#[test]
fn pane_mode_close_keeps_the_last_pane_confirmation() {
    let mut state = shell(snapshot());
    state.handle_input_bytes(&[CTRL_P]);

    let close = state.handle_input_bytes(b"x");
    assert!(close.actions.is_empty(), "{:?}", close.actions);
    assert!(matches!(
        state.overlay.as_ref(),
        Some(ClientShellOverlay::ConfirmClose(_))
    ));
    assert_eq!(state.mode, ClientShellMode::Scope(ScopeMode::Pane));
}

#[test]
fn spaces_mode_renames_and_closes_the_focused_workspace() {
    let mut state = shell(snapshot_with_workspaces());
    state.handle_input_bytes(&[CTRL_S]);

    state.handle_input_bytes(b"r");
    assert!(matches!(
        state.overlay.as_ref(),
        Some(ClientShellOverlay::Rename(ClientRenameOverlay {
            target: ClientRenameTarget::Workspace { workspace_id },
            ..
        })) if workspace_id == "ws_1"
    ));
    state.handle_input_bytes(b"\x15renamed\r");
    assert!(state.overlay.is_none());
    assert_eq!(state.mode, ClientShellMode::Scope(ScopeMode::Workspace));

    // Closing asks first, as the keybinding does. Cancelling keeps the mode open.
    state.handle_input_bytes(b"x");
    assert!(matches!(
        state.overlay.as_ref(),
        Some(ClientShellOverlay::ConfirmClose(_))
    ));
    state.handle_input_bytes(b"\x1b");
    assert!(state.overlay.is_none());
    assert_eq!(state.mode, ClientShellMode::Scope(ScopeMode::Workspace));

    let close = state.handle_input_bytes(b"x");
    assert!(close.actions.is_empty());
    assert!(matches!(
        state.overlay.as_ref(),
        Some(ClientShellOverlay::ConfirmClose(ClientConfirmCloseOverlay {
            workspace_id,
            ..
        })) if workspace_id == "ws_1"
    ));
    let confirm = state.handle_input_bytes(b"\r");
    assert!(matches!(
        method(&confirm),
        Method::WorkspaceClose(params) if params.workspace_id == "ws_1"
    ));
    assert_eq!(state.mode, ClientShellMode::Scope(ScopeMode::Workspace));
}

#[test]
fn spaces_mode_close_skips_the_dialog_when_confirmation_is_off() {
    let config = toml::from_str::<Config>("[ui]\nconfirm_close = false\n").expect("config");
    let mut state = shell_with_config(snapshot_with_workspaces(), &config);
    state.handle_input_bytes(&[CTRL_S]);

    let close = state.handle_input_bytes(b"x");
    assert!(matches!(
        method(&close),
        Method::WorkspaceClose(params) if params.workspace_id == "ws_1"
    ));
    assert_eq!(state.mode, ClientShellMode::Scope(ScopeMode::Workspace));
}

#[test]
fn tab_mode_moves_focus_and_reorders_the_focused_tab() {
    let mut state = shell(snapshot_with_tabs());
    state.handle_input_bytes(&[CTRL_T]);

    for (input, tab_id) in [
        (&b"l"[..], "tab_3"),
        (b"\x1b[C", "tab_3"),
        (b"j", "tab_3"),
        (b"h", "tab_1"),
        (b"\x1b[D", "tab_1"),
        (b"k", "tab_1"),
    ] {
        let outcome = state.handle_input_bytes(input);
        assert!(
            matches!(method(&outcome), Method::TabFocus(target) if target.tab_id == tab_id),
            "{input:?}"
        );
    }

    let next = state.handle_input_bytes(b"L");
    assert!(matches!(
        method(&next),
        Method::TabMove(params) if params.tab_id == "tab_2" && params.insert_index == 3
    ));
    let previous = state.handle_input_bytes(b"\x1b[1;2D");
    assert!(matches!(
        method(&previous),
        Method::TabMove(params) if params.tab_id == "tab_2" && params.insert_index == 0
    ));
    assert_eq!(state.mode, ClientShellMode::Scope(ScopeMode::Tab));
}

#[test]
fn tab_mode_renames_the_focused_tab_and_stays_open() {
    let mut state = shell(snapshot_with_tabs());
    state.handle_input_bytes(&[CTRL_T]);

    let rename = state.handle_input_bytes(b"r");
    assert!(rename.actions.is_empty() && rename.requests.is_empty());
    assert!(matches!(
        state.overlay.as_ref(),
        Some(ClientShellOverlay::Rename(ClientRenameOverlay {
            target: ClientRenameTarget::Tab { tab_id, .. },
            ..
        })) if tab_id == "tab_2"
    ));
    assert_eq!(state.mode, ClientShellMode::Scope(ScopeMode::Tab));
    // The dialog owns the keys until it closes, then the mode is still open.
    let saved = state.handle_input_bytes(b"\x15renamed\r");
    assert!(
        matches!(
            method(&saved),
            Method::TabRename(params) if params.tab_id == "tab_2" && params.label == "renamed"
        ),
        "{:?}",
        saved.actions
    );
    assert!(state.overlay.is_none());
    assert_eq!(state.mode, ClientShellMode::Scope(ScopeMode::Tab));
}

#[test]
fn spaces_mode_moves_focus_reorders_and_creates_workspaces() {
    let mut state = shell(snapshot_with_workspaces());
    state.handle_input_bytes(&[CTRL_S]);

    for (input, workspace_id) in [
        (&b"j"[..], "ws_2"),
        (b"\x1b[B", "ws_2"),
        (b"l", "ws_2"),
        (b"k", "ws_3"),
        (b"\x1b[A", "ws_3"),
    ] {
        let outcome = state.handle_input_bytes(input);
        assert!(
            matches!(
                method(&outcome),
                Method::WorkspaceFocus(target) if target.workspace_id == workspace_id
            ),
            "{input:?}"
        );
    }

    let down = state.handle_input_bytes(b"J");
    assert!(matches!(
        method(&down),
        Method::WorkspaceMove(params) if params.workspace_id == "ws_1" && params.insert_index == 2
    ));
    // Moving up from the top wraps to the end, like tab moves.
    let up = state.handle_input_bytes(b"\x1b[1;2A");
    assert!(matches!(
        method(&up),
        Method::WorkspaceMove(params) if params.workspace_id == "ws_1" && params.insert_index == 3
    ));
    assert_eq!(state.mode, ClientShellMode::Scope(ScopeMode::Workspace));

    let created = state.handle_input_bytes(b"n");
    assert!(matches!(
        method(&created),
        Method::WorkspaceCreate(params)
            if params.focus && params.source_workspace_id.as_deref() == Some("ws_1")
    ));
    assert_eq!(state.mode, ClientShellMode::Terminal);
}

#[test]
fn agents_mode_only_moves_focus() {
    let mut state = shell(snapshot_with_agents());
    state.handle_input_bytes(&[CTRL_A]);

    for input in [&b"j"[..], b"\x1b[B", b"l", b"k", b"\x1b[A", b"h"] {
        let outcome = state.handle_input_bytes(input);
        assert!(
            matches!(method(&outcome), Method::PaneFocus(target) if target.pane_id == "pane_2"),
            "{input:?}"
        );
    }

    for input in [&b"J"[..], b"\x1b[1;2B", b"d", b"n", b"r", b"x"] {
        let outcome = state.handle_input_bytes(input);
        assert!(outcome.actions.is_empty(), "{input:?}");
        assert!(outcome.requests.is_empty(), "{input:?}");
        assert_eq!(state.mode, ClientShellMode::Scope(ScopeMode::Agent));
    }
}

#[test]
fn mode_bar_shows_each_modes_hotkeys() {
    let mut state = shell(snapshot());

    for (byte, expected) in [
        (
            CTRL_P,
            &[
                "PANE",
                "hjkl ←↓↑→ focus",
                "HJKL move",
                "d split down",
                "r split right",
                "x close",
                "esc done",
            ][..],
        ),
        (
            CTRL_T,
            &["TAB", "h/l ←/→ focus", "H/L move", "r rename", "esc done"],
        ),
        (
            CTRL_S,
            &[
                "SPACES",
                "j/k ↑/↓ focus",
                "J/K move",
                "n new",
                "r rename",
                "x close",
                "esc done",
            ],
        ),
        (CTRL_A, &["AGENTS", "j/k ↑/↓ focus", "esc done"]),
    ] {
        state.handle_input_bytes(&[byte]);
        let text = mode_bar_text(&mut state);
        for hint in expected {
            assert!(text.contains(hint), "missing {hint:?} in {text}");
        }
    }

    state.handle_input_bytes(b"\x1b");
    let text = mode_bar_text(&mut state);
    assert!(!text.contains("AGENTS"), "{text}");
}

#[test]
fn mode_keys_are_configurable_and_yield_to_user_bindings() {
    let config = toml::from_str::<Config>(
        r#"
[keys]
prefix = "ctrl+a"
pane_mode = "prefix+p"
tab_mode = ""
"#,
    )
    .expect("mode keybinds");
    assert!(config.collect_diagnostics().is_empty());
    let mut state = shell_with_config(snapshot(), &config);

    // A user prefix on ctrl+a displaces the agents mode default silently.
    state.handle_input_bytes(&[CTRL_A]);
    assert_eq!(state.mode, ClientShellMode::Prefix);
    state.handle_input_bytes(b"p");
    assert_eq!(state.mode, ClientShellMode::Scope(ScopeMode::Pane));
    // A prefix-bound mode closes on its bare key.
    state.handle_input_bytes(b"p");
    assert_eq!(state.mode, ClientShellMode::Terminal);

    for byte in [CTRL_P, CTRL_T] {
        let typed = state.handle_input_bytes(&[byte]);
        assert_eq!(typed.requests.len(), 1, "{byte:#x} should reach the pane");
        assert_eq!(state.mode, ClientShellMode::Terminal);
    }
    state.handle_input_bytes(&[CTRL_S]);
    assert_eq!(state.mode, ClientShellMode::Scope(ScopeMode::Workspace));
}
