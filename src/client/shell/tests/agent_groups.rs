use super::*;
use crate::raw_input::RawInputEvent;

/// Two spaces: "alpha" (focused) with `alpha_agents` agents and "beta" with one.
fn grouped_snapshot(alpha_agents: usize) -> ClientShellSnapshot {
    let mut snapshot = snapshot();
    let mut beta = snapshot.workspaces[0].clone();
    snapshot.workspaces[0].label = "alpha".into();
    beta.workspace_id = "ws_2".into();
    beta.active_tab_id = "tab_2".into();
    beta.number = 2;
    beta.label = "beta".into();
    beta.focused = false;
    snapshot.workspaces.push(beta);
    let mut beta_tab = snapshot.tabs[0].clone();
    beta_tab.tab_id = "tab_2".into();
    beta_tab.workspace_id = "ws_2".into();
    beta_tab.label = "review".into();
    beta_tab.custom_label = true;
    beta_tab.focused = false;
    snapshot.tabs.push(beta_tab);

    let spaces = std::iter::repeat_n(("ws_1", "tab_1"), alpha_agents)
        .chain([("ws_2", "tab_2")])
        .enumerate();
    snapshot.panes.clear();
    snapshot.agents.clear();
    for (index, (workspace_id, tab_id)) in spaces {
        let pane_id = format!("pane_{}", index + 1);
        snapshot.panes.push(ClientShellPane {
            pane_id: pane_id.clone(),
            workspace_id: workspace_id.into(),
            tab_id: tab_id.into(),
            label: None,
            cwd: Some("/repo".into()),
            foreground_cwd: Some("/repo".into()),
            focused: index == 0,
            right_click_passthrough: false,
        });
        snapshot.agents.push(ClientShellAgent {
            pane_id,
            workspace_id: workspace_id.into(),
            tab_id: tab_id.into(),
            name: Some(format!("agent {}", index + 1)),
            display_agent: None,
            agent: Some("pi".into()),
            title: None,
            terminal_title: None,
            terminal_title_stripped: None,
            agent_status: AgentStatus::Idle,
            state_change_seq: index as u64 + 1,
            state_labels: Vec::new(),
            tokens: Vec::new(),
            focused: index == 0,
        });
    }
    snapshot
}

fn grouped_config() -> Config {
    let mut config = Config::default();
    config.ui.sidebar.agents.group_by_space = true;
    config
}

fn shell(snapshot: ClientShellSnapshot, config: &Config) -> ClientShellState {
    let mut state = ClientShellState::new(ClientShellConfig::from_config(config));
    state.set_snapshot(Box::new(snapshot));
    state.set_pane_surface(surface());
    state
}

/// The sidebar text on each row of the agent panel body.
fn agent_body_rows(state: &mut ClientShellState, rows: u16) -> Vec<String> {
    let frame = state.compose(106, rows).expect("agent panel frame");
    let body = state.hits.agent_body;
    frame_rows(&frame)
        .into_iter()
        .skip(usize::from(body.y))
        .take(usize::from(body.height))
        .map(|row| {
            row.chars()
                .skip(usize::from(body.x))
                .take(usize::from(body.width))
                .collect::<String>()
                .trim_end()
                .to_owned()
        })
        .collect()
}

fn is_heading(row: &str, name: &str) -> bool {
    row.starts_with(&format!(" {name} ─"))
}

/// The agent panel body row that is the hit target for `pane_id`, if visible.
fn agent_row_index(state: &ClientShellState, pane_id: &str) -> Option<usize> {
    let body = state.hits.agent_body;
    state
        .hits
        .agents
        .iter()
        .find(|(_, hit)| hit == pane_id)
        .map(|(rect, _)| usize::from(rect.y - body.y))
}

#[test]
fn agents_appear_under_a_heading_for_their_space() {
    let mut state = shell(grouped_snapshot(2), &grouped_config());
    let rows = agent_body_rows(&mut state, 30);

    let position = |predicate: &dyn Fn(&str) -> bool| {
        rows.iter()
            .position(|row| predicate(row))
            .unwrap_or_else(|| panic!("missing row in {rows:#?}"))
    };
    let agent_row = |pane_id: &str| {
        agent_row_index(&state, pane_id).unwrap_or_else(|| panic!("missing {pane_id} in {rows:#?}"))
    };
    let alpha = position(&|row| is_heading(row, "alpha"));
    let first = agent_row("pane_1");
    let second = agent_row("pane_2");
    let beta = position(&|row| is_heading(row, "beta"));
    let third = agent_row("pane_3");
    assert!(alpha < first && first < second && second < beta && beta < third);
    assert_eq!(beta, second + 1, "{rows:#?}");

    // One indented line per agent that names the tab, not the agent or space.
    for index in [first, second, third] {
        let row = &rows[index];
        assert!(row.starts_with("  "), "{row:?}");
        assert!(!row.contains("alpha") && !row.contains("beta"), "{row:?}");
        assert!(!row.contains("agent") && !row.contains("pi"), "{row:?}");
    }
    assert_eq!(second, first + 1, "{rows:#?}");
    // The alpha tab keeps its automatic label, so its rows say so.
    assert!(rows[first].contains("unnamed"), "{}", rows[first]);
    assert!(rows[second].contains("unnamed"), "{}", rows[second]);
    assert!(rows[third].contains("review"), "{}", rows[third]);
    assert!(!rows[third].contains("unnamed"), "{}", rows[third]);

    let hit_panes = state
        .hits
        .agents
        .iter()
        .map(|(_, pane_id)| pane_id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(hit_panes, ["pane_1", "pane_2", "pane_3"]);
    // Headings are not agent hit targets.
    let body = state.hits.agent_body;
    assert!(state
        .hits
        .agents
        .iter()
        .all(|(rect, _)| rect.y != body.y + alpha as u16 && rect.y != body.y + beta as u16));
}

#[test]
fn agent_list_stays_flat_without_the_option_or_with_priority_sort() {
    let mut priority = grouped_config();
    priority.ui.agent_panel_sort = crate::config::AgentPanelSortConfig::Priority;
    for config in [Config::default(), priority] {
        let mut state = shell(grouped_snapshot(2), &config);
        let rows = agent_body_rows(&mut state, 30);
        assert!(
            !rows
                .iter()
                .any(|row| is_heading(row, "alpha") || is_heading(row, "beta")),
            "{rows:#?}"
        );
        assert!(rows.iter().any(|row| row.contains("alpha")), "{rows:#?}");
        assert!(rows.iter().any(|row| row.contains("agent 1")), "{rows:#?}");
        assert!(!rows.iter().any(|row| row.contains("unnamed")), "{rows:#?}");
    }

    // Toggling the panel to priority drops the headings.
    let mut state = shell(grouped_snapshot(2), &grouped_config());
    assert!(agent_body_rows(&mut state, 30)
        .iter()
        .any(|row| is_heading(row, "beta")));
    let toggle = state.hits.agent_sort_toggle;
    state.handle_raw_events(vec![RawInputEvent::Mouse(crossterm::event::MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: toggle.x,
        row: toggle.y,
        modifiers: KeyModifiers::empty(),
    })]);
    assert!(!agent_body_rows(&mut state, 30)
        .iter()
        .any(|row| is_heading(row, "beta")));
}

#[test]
fn moving_to_an_agent_out_of_view_reveals_its_space_heading() {
    let mut state = shell(grouped_snapshot(12), &grouped_config());
    let rows = agent_body_rows(&mut state, 24);
    assert!(
        agent_row_index(&state, "pane_13").is_none(),
        "the last agent should start out of view: {rows:#?}"
    );

    // Previous from the first agent wraps to the last one, in "beta".
    let mut outcome = ClientShellInput::default();
    state.record_binding(
        crate::input::KeybindMatch::Action(crate::input::KeybindAction::PreviousAgent),
        &mut outcome,
    );
    let rows = agent_body_rows(&mut state, 24);
    let beta = rows
        .iter()
        .position(|row| is_heading(row, "beta"))
        .unwrap_or_else(|| panic!("beta heading should be visible: {rows:#?}"));
    assert_eq!(
        agent_row_index(&state, "pane_13"),
        Some(beta + 1),
        "{rows:#?}"
    );

    // The scroll range counts headings, so the end of the list is reachable.
    let body = state.hits.agent_body;
    for _ in 0..20 {
        state.handle_raw_events(vec![RawInputEvent::Mouse(crossterm::event::MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: body.x,
            row: body.y,
            modifiers: KeyModifiers::empty(),
        })]);
    }
    let rows = agent_body_rows(&mut state, 24);
    let last = rows
        .iter()
        .rposition(|row| !row.is_empty())
        .unwrap_or_else(|| panic!("{rows:#?}"));
    assert_eq!(agent_row_index(&state, "pane_13"), Some(last), "{rows:#?}");
}
