use super::*;
use crate::raw_input::RawInputEvent;
use ratatui::style::{Color, Modifier};

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
            name: None,
            display_agent: None,
            agent: Some("pi".into()),
            title: None,
            terminal_title: None,
            terminal_title_stripped: None,
            agent_status: AgentStatus::Idle,
            // Equal states sort by the latest change, so keep the pane order.
            state_change_seq: 100 - index as u64,
            state_labels: Vec::new(),
            tokens: Vec::new(),
            focused: index == 0,
        });
    }
    snapshot
}

/// `grouped_snapshot(2)` with a session name from each source the panel reads.
fn named_snapshot() -> ClientShellSnapshot {
    let mut snapshot = grouped_snapshot(2);
    snapshot.agents[0].terminal_title = Some("✳ fix login".into());
    snapshot.agents[0].terminal_title_stripped = Some("fix login".into());
    // A bare default title is not a name.
    snapshot.agents[1].terminal_title = Some("✳ pi".into());
    snapshot.agents[1].terminal_title_stripped = Some("pi".into());
    snapshot.agents[2].name = Some("reviewer".into());
    snapshot
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

fn hit_panes(state: &ClientShellState) -> Vec<&str> {
    state
        .hits
        .agents
        .iter()
        .map(|(_, pane_id)| pane_id.as_str())
        .collect()
}

#[test]
fn agents_appear_under_a_heading_for_their_space() {
    let mut state = shell(named_snapshot(), &Config::default());
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
    // The first heading starts the list; later headings get a blank row above.
    assert_eq!(alpha, 0, "{rows:#?}");
    assert_eq!(beta, second + 2, "{rows:#?}");
    assert!(rows[second + 1].is_empty(), "{rows:#?}");

    // One indented line per agent that names its session, not the agent, tab, or space.
    for index in [first, second, third] {
        let row = &rows[index];
        assert!(row.starts_with("  "), "{row:?}");
        assert!(!row.contains("alpha") && !row.contains("beta"), "{row:?}");
        assert!(!row.contains(" pi"), "{row:?}");
    }
    assert_eq!(second, first + 1, "{rows:#?}");
    assert!(rows[first].ends_with("fix login"), "{}", rows[first]);
    assert!(rows[second].ends_with("Unnamed"), "{}", rows[second]);
    assert!(rows[third].ends_with("reviewer"), "{}", rows[third]);

    assert_eq!(hit_panes(&state), ["pane_1", "pane_2", "pane_3"]);
    // Headings are not agent hit targets.
    let body = state.hits.agent_body;
    assert!(state
        .hits
        .agents
        .iter()
        .all(|(rect, _)| rect.y != body.y + alpha as u16 && rect.y != body.y + beta as u16));
}

#[test]
fn agents_sort_by_attention_inside_each_space() {
    let mut snapshot = grouped_snapshot(3);
    snapshot.agents[1].agent_status = AgentStatus::Working;
    snapshot.agents[2].agent_status = AgentStatus::Blocked;
    // The beta agent needs attention too, but beta stays after alpha.
    snapshot.agents[3].agent_status = AgentStatus::Blocked;
    let mut state = shell(snapshot, &Config::default());
    agent_body_rows(&mut state, 30);
    assert_eq!(hit_panes(&state), ["pane_3", "pane_2", "pane_1", "pane_4"]);

    // Agent navigation follows the panel order.
    assert_eq!(
        super::super::agent_sidebar::ordered_agent_pane_ids(state.snapshot.as_deref().unwrap()),
        ["pane_3", "pane_2", "pane_1", "pane_4"]
    );
    let mut outcome = ClientShellInput::default();
    state.record_binding(
        crate::input::KeybindMatch::Action(crate::input::KeybindAction::FocusAgent(0)),
        &mut outcome,
    );
    let focused = outcome.actions.iter().find_map(|action| match action {
        ClientShellAction::Endpoint { request, .. } => match &request.method {
            crate::api::schema::Method::PaneFocus(target) => Some(target.pane_id.clone()),
            _ => None,
        },
        _ => None,
    });
    assert_eq!(focused.as_deref(), Some("pane_3"));
}

#[test]
fn retired_sort_settings_keep_the_grouped_panel() {
    let config: Config = toml::from_str(
        "[ui]\nagent_panel_sort = \"priority\"\n\n[ui.sidebar.agents]\ngroup_by_space = false\n",
    )
    .expect("retired keys still parse");
    let mut state = shell(grouped_snapshot(2), &config);
    let rows = agent_body_rows(&mut state, 30);
    assert!(rows.iter().any(|row| is_heading(row, "beta")), "{rows:#?}");

    // The panel header has no sort toggle.
    let frame = state.compose(106, 30).expect("frame");
    let header = &frame_rows(&frame)[usize::from(state.hits.agent_body.y - 2)];
    assert!(header.contains(" agents"), "{header}");
    assert!(
        !header.contains("grouped") && !header.contains("priority"),
        "{header}"
    );
}

#[test]
fn custom_agent_views_show_a_flat_list_in_their_order() {
    let mut snapshot = grouped_snapshot(2);
    snapshot.agent_view_label = Some("focus".into());
    snapshot.agent_order = vec!["pane_3".into(), "pane_1".into()];
    let mut state = shell(snapshot, &Config::default());
    let rows = agent_body_rows(&mut state, 30);
    assert!(
        !rows
            .iter()
            .any(|row| is_heading(row, "alpha") || is_heading(row, "beta")),
        "{rows:#?}"
    );
    assert_eq!(hit_panes(&state), ["pane_3", "pane_1"]);
    // Without headings the rows name their space.
    assert!(rows.iter().any(|row| row.contains("alpha")), "{rows:#?}");
    assert!(!rows.iter().any(|row| row.contains("Unnamed")), "{rows:#?}");

    let frame = state.compose(106, 30).expect("frame");
    let header = &frame_rows(&frame)[usize::from(state.hits.agent_body.y - 2)];
    assert!(header.contains("focus"), "{header}");
}

#[test]
fn heading_spacing_adds_to_the_configured_row_gap() {
    let mut config = Config::default();
    config.ui.sidebar.agents.row_gap = 1;
    let mut state = shell(grouped_snapshot(2), &config);
    let rows = agent_body_rows(&mut state, 30);
    let second = agent_row_index(&state, "pane_2").expect("second agent row");
    let beta = rows
        .iter()
        .position(|row| is_heading(row, "beta"))
        .unwrap_or_else(|| panic!("{rows:#?}"));
    assert_eq!(
        agent_row_index(&state, "pane_1").map(|first| second - first),
        Some(2),
        "{rows:#?}"
    );
    assert_eq!(beta, second + 3, "{rows:#?}");
    assert!(
        rows[second + 1..beta].iter().all(String::is_empty),
        "{rows:#?}"
    );

    let items = super::super::agent_sidebar::agent_list_items(
        state.snapshot.as_deref().unwrap(),
        &state.config,
    );
    let (heights, gaps) =
        super::super::agent_sidebar::agent_list_heights(&items, 1, |row| row.rows.len());
    assert_eq!(heights.len(), gaps.len());
    // heading alpha, pane_1, pane_2, heading beta, pane_3
    assert_eq!(gaps, [0, 1, 2, 0, 0]);
}

#[test]
fn clicking_below_a_heading_gap_hits_the_right_agent() {
    let mut state = shell(grouped_snapshot(2), &Config::default());
    let rows = agent_body_rows(&mut state, 30);
    let body = state.hits.agent_body;
    let beta = rows
        .iter()
        .position(|row| is_heading(row, "beta"))
        .unwrap_or_else(|| panic!("{rows:#?}"));
    let click = |state: &mut ClientShellState, row: usize| {
        state.handle_raw_events(vec![RawInputEvent::Mouse(crossterm::event::MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: body.x + 2,
            row: body.y + row as u16,
            modifiers: KeyModifiers::empty(),
        })])
    };
    let focused_pane = |outcome: &ClientShellInput| {
        outcome.actions.iter().find_map(|action| match action {
            ClientShellAction::Endpoint { request, .. } => match &request.method {
                crate::api::schema::Method::PaneFocus(target) => Some(target.pane_id.clone()),
                _ => None,
            },
            _ => None,
        })
    };

    // The blank row and the heading are not agents.
    assert_eq!(focused_pane(&click(&mut state, beta - 1)), None);
    assert_eq!(focused_pane(&click(&mut state, beta)), None);
    assert_eq!(
        focused_pane(&click(&mut state, beta + 1)).as_deref(),
        Some("pane_3")
    );
}

#[test]
fn moving_to_an_agent_out_of_view_reveals_its_space_heading() {
    let mut state = shell(grouped_snapshot(12), &Config::default());
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

/// Foreground color and boldness of the first cell of `needle` inside `area`.
fn text_style(frame: &FrameData, area: Rect, needle: &str) -> (Color, bool) {
    let (x, y) = cell_symbol_position(frame, area, needle);
    let buffer = frame.to_ratatui_buffer().expect("sidebar buffer");
    let cell = &buffer[(x, y)];
    (cell.fg, cell.modifier.contains(Modifier::BOLD))
}

fn sidebar_area(state: &ClientShellState) -> Rect {
    Rect::new(0, 0, state.hits.sidebar_divider.x, 30)
}

#[test]
fn sidebar_text_uses_two_brightness_levels() {
    let mut state = shell(named_snapshot(), &Config::default());
    let frame = state.compose(106, 30).expect("frame");
    let palette = state.config.palette.clone();
    let sidebar = sidebar_area(&state);
    let bright = (palette.text, true);
    let normal = (palette.subtext0, false);

    // Section headers, the focused space, and the focused agent are bright.
    assert_eq!(text_style(&frame, sidebar, "spaces"), bright);
    assert_eq!(text_style(&frame, sidebar, "agents"), bright);
    let spaces = state.hits.workspace_body;
    assert_eq!(text_style(&frame, spaces, "alpha"), bright);
    assert_eq!(
        text_style(&frame, state.hits.agent_body, "fix login"),
        bright
    );

    // Headings in the agents panel and every other row share the normal level.
    assert_eq!(text_style(&frame, spaces, "beta"), normal);
    assert_eq!(text_style(&frame, state.hits.agent_body, "alpha"), normal);
    assert_eq!(text_style(&frame, state.hits.agent_body, "beta"), normal);
    assert_eq!(text_style(&frame, state.hits.agent_body, "Unnamed"), normal);
    assert_eq!(
        text_style(&frame, state.hits.agent_body, "reviewer"),
        normal
    );
}

#[test]
fn space_colors_follow_the_brightness_levels() {
    let config: Config = toml::from_str(
        r##"
[ui.sidebar]
space_color_default = "#f0883e"

[[ui.sidebar.space_colors]]
cwd = '/work/prod-'
color = "#f85149"
"##,
    )
    .expect("space colors");
    let mut snapshot = named_snapshot();
    snapshot.workspaces[0].cwd = Some("/work/prod-api".into());
    let mut state = shell(snapshot, &config);
    let frame = state.compose(106, 30).expect("frame");
    let palette = state.config.palette.clone();
    let red = Color::Rgb(0xf8, 0x51, 0x49);
    let orange = Color::Rgb(0xf0, 0x88, 0x3e);
    let dimmed = |color| {
        super::super::render::sidebar::space_name_style(Some(color), false, &palette)
            .fg
            .expect("dimmed color")
    };
    assert_ne!(dimmed(red), red);
    assert_ne!(dimmed(orange), orange);

    // The focused space shows the full color; other names show the dimmed form.
    let spaces = state.hits.workspace_body;
    assert_eq!(text_style(&frame, spaces, "alpha"), (red, true));
    assert_eq!(text_style(&frame, spaces, "beta"), (dimmed(orange), false));
    let agents = state.hits.agent_body;
    assert_eq!(text_style(&frame, agents, "alpha"), (dimmed(red), false));
    assert_eq!(text_style(&frame, agents, "beta"), (dimmed(orange), false));
    // Agent rows keep the theme colors.
    assert_eq!(
        text_style(&frame, agents, "fix login"),
        (palette.text, true)
    );
}

#[test]
fn dimmed_space_colors_move_toward_the_background() {
    let mut palette = crate::app::state::Palette::catppuccin();
    palette.sidebar_bg = Color::Rgb(0, 0, 0);
    let style = super::super::render::sidebar::space_name_style(
        Some(Color::Rgb(200, 100, 0)),
        false,
        &palette,
    );
    assert_eq!(style.fg, Some(Color::Rgb(156, 78, 0)));
    assert!(!style.add_modifier.contains(Modifier::BOLD));

    // Named colors and unknown backgrounds fall back to the terminal's dim attribute.
    let style =
        super::super::render::sidebar::space_name_style(Some(Color::Green), false, &palette);
    assert_eq!(style.fg, Some(Color::Green));
    assert!(style.add_modifier.contains(Modifier::DIM));
    palette.sidebar_bg = Color::Reset;
    palette.panel_bg = Color::Reset;
    palette.surface_dim = Color::Reset;
    let style = super::super::render::sidebar::space_name_style(
        Some(Color::Rgb(200, 100, 0)),
        false,
        &palette,
    );
    assert_eq!(style.fg, Some(Color::Rgb(200, 100, 0)));
    assert!(style.add_modifier.contains(Modifier::DIM));
}

#[test]
fn space_colors_apply_after_a_config_reload() {
    let mut state = shell(grouped_snapshot(1), &Config::default());
    let frame = state.compose(106, 30).expect("frame");
    let spaces = state.hits.workspace_body;
    assert_eq!(
        text_style(&frame, spaces, "alpha"),
        (state.config.palette.text, true)
    );

    let next: Config = toml::from_str(
        "[ui.sidebar]\nspace_color_default = \"#123456\"\n\n[[ui.sidebar.space_colors]]\ncwd = '/never'\ncolor = 'not-a-color'\n",
    )
    .unwrap();
    // A reload reports the rules it skips, like `herdr config check`.
    let diagnostics = state.config.apply_live_config(&next, &[], &[]);
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert!(diagnostics[0].starts_with("ui.sidebar.space_colors[0].color"));
    let frame = state.compose(106, 30).expect("frame");
    assert_eq!(
        text_style(&frame, spaces, "alpha"),
        (Color::Rgb(0x12, 0x34, 0x56), true)
    );
}
