//! Pane, tab, spaces, agents, and notifications modes.
//!
//! Each mode stays open so movement keys can repeat: h/j/k/l or the arrows
//! move focus and the shifted keys move the focused item. Creating an item
//! returns to the terminal so the new item can take input right away.
//! Renaming or closing keeps the mode open, so `x` can close several items.

use super::*;
use crate::config::terminal_key_matches_combo;
use crate::input::{KeybindAction, KeybindMatch, TerminalKey};
use crossterm::event::KeyModifiers;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Direction {
    Left,
    Down,
    Up,
    Right,
}

impl Direction {
    fn toward_end(self) -> bool {
        matches!(self, Self::Down | Self::Right)
    }
}

impl ScopeMode {
    const ALL: [Self; 5] = [
        Self::Pane,
        Self::Tab,
        Self::Workspace,
        Self::Agent,
        Self::Notification,
    ];

    pub(super) fn entered_by(action: KeybindAction) -> Option<Self> {
        match action {
            KeybindAction::EnterPaneMode => Some(Self::Pane),
            KeybindAction::EnterTabMode => Some(Self::Tab),
            KeybindAction::EnterWorkspaceMode => Some(Self::Workspace),
            KeybindAction::EnterAgentMode => Some(Self::Agent),
            KeybindAction::EnterNotificationMode => Some(Self::Notification),
            _ => None,
        }
    }

    fn bindings(self, keybinds: &crate::config::Keybinds) -> &crate::config::ActionKeybinds {
        match self {
            Self::Pane => &keybinds.pane_mode,
            Self::Tab => &keybinds.tab_mode,
            Self::Workspace => &keybinds.workspace_mode,
            Self::Agent => &keybinds.agent_mode,
            Self::Notification => &keybinds.notification_mode,
        }
    }

    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Pane => " PANE ",
            Self::Tab => " TAB ",
            Self::Workspace => " SPACES ",
            Self::Agent => " AGENTS ",
            Self::Notification => " NOTIFICATIONS ",
        }
    }

    pub(super) fn color(self, palette: &Palette) -> ratatui::style::Color {
        match self {
            Self::Pane => palette.accent,
            Self::Tab => palette.blue,
            Self::Workspace => palette.green,
            Self::Agent => palette.peach,
            Self::Notification => palette.yellow,
        }
    }

    /// Key and description pairs shown in the mode bar.
    pub(super) fn hints(self) -> &'static [(&'static str, &'static str)] {
        match self {
            Self::Pane => &[
                ("hjkl ←↓↑→", "focus"),
                ("HJKL", "move"),
                ("d", "split down"),
                ("r", "split right"),
                ("x", "close"),
                ("esc", "done"),
            ],
            Self::Tab => &[
                ("h/l ←/→", "focus"),
                ("H/L", "move"),
                ("r", "rename"),
                ("esc", "done"),
            ],
            Self::Workspace => &[
                ("j/k ↑/↓", "focus"),
                ("J/K", "move"),
                ("n", "new"),
                ("r", "rename"),
                ("x", "close"),
                ("esc", "done"),
            ],
            Self::Agent => &[("j/k ↑/↓", "focus"), ("esc", "done")],
            Self::Notification => &[
                ("j/k ↑/↓", "open"),
                ("a", "accept"),
                ("x", "dismiss"),
                ("esc", "done"),
            ],
        }
    }

    /// The action that moves focus, or `None` for notifications, which have no focus.
    fn focus_action(self, direction: Direction) -> Option<KeybindAction> {
        let toward_end = direction.toward_end();
        Some(match self {
            Self::Pane => match direction {
                Direction::Left => KeybindAction::FocusPaneLeft,
                Direction::Down => KeybindAction::FocusPaneDown,
                Direction::Up => KeybindAction::FocusPaneUp,
                Direction::Right => KeybindAction::FocusPaneRight,
            },
            Self::Tab if toward_end => KeybindAction::NextTab,
            Self::Tab => KeybindAction::PreviousTab,
            Self::Workspace if toward_end => KeybindAction::NextWorkspace,
            Self::Workspace => KeybindAction::PreviousWorkspace,
            Self::Agent if toward_end => KeybindAction::NextAgent,
            Self::Agent => KeybindAction::PreviousAgent,
            Self::Notification => return None,
        })
    }

    /// The action for an item key, and whether it returns to the terminal.
    /// Creating returns so the new item takes input. Renaming and closing reuse
    /// the normal actions, with their dialogs, and keep the mode open.
    fn item_action(self, key: &TerminalKey) -> Option<(KeybindAction, bool)> {
        let plain = |character| plain_key(key, character);
        match self {
            Self::Pane if plain('d') => Some((KeybindAction::SplitHorizontal, true)),
            Self::Pane if plain('r') => Some((KeybindAction::SplitVertical, true)),
            Self::Pane if plain('x') => Some((KeybindAction::ClosePane, false)),
            Self::Tab if plain('r') => Some((KeybindAction::RenameTab, false)),
            Self::Workspace if plain('n') => Some((KeybindAction::NewWorkspace, true)),
            Self::Workspace if plain('r') => Some((KeybindAction::RenameWorkspace, false)),
            Self::Workspace if plain('x') => Some((KeybindAction::CloseWorkspace, false)),
            _ => None,
        }
    }
}

fn plain_key(key: &TerminalKey, character: char) -> bool {
    terminal_key_matches_combo(key, (KeyCode::Char(character), KeyModifiers::empty()))
}

/// h/j/k/l or an arrow, and whether Shift was held to move the focused item.
fn movement(key: &TerminalKey) -> Option<(Direction, bool)> {
    [
        ('h', KeyCode::Left, Direction::Left),
        ('j', KeyCode::Down, Direction::Down),
        ('k', KeyCode::Up, Direction::Up),
        ('l', KeyCode::Right, Direction::Right),
    ]
    .into_iter()
    .find_map(|(letter, arrow, direction)| {
        [(KeyModifiers::empty(), false), (KeyModifiers::SHIFT, true)]
            .into_iter()
            .find(|(modifiers, _)| {
                terminal_key_matches_combo(key, (KeyCode::Char(letter), *modifiers))
                    || terminal_key_matches_combo(key, (arrow, *modifiers))
            })
            .map(|(_, shifted)| (direction, shifted))
    })
}

impl ClientShellState {
    pub(super) fn route_scope_key(
        &mut self,
        scope: ScopeMode,
        key: &TerminalKey,
        outcome: &mut ClientShellInput,
    ) {
        let (code, modifiers) = crate::config::normalize_key_combo((key.code, key.modifiers));
        if (matches!(code, KeyCode::Esc | KeyCode::Enter) && modifiers.is_empty())
            || super::input::is_ctrl_bracket_key(key)
        {
            self.exit_scope_mode(outcome);
            return;
        }
        if let Some(next) = self.scope_mode_key(key, true) {
            self.switch_scope_mode(scope, next, outcome);
            return;
        }
        if self.config.keybinds.matches_prefix(key) {
            self.mode = ClientShellMode::Prefix;
            outcome.repaint = true;
            return;
        }

        if let Some((direction, shifted)) = movement(key) {
            if scope == ScopeMode::Notification {
                // The log is a vertical list whose order the user does not arrange.
                if !shifted && matches!(direction, Direction::Up | Direction::Down) {
                    self.open_adjacent_notification(direction.toward_end(), outcome);
                }
                outcome.repaint = true;
                return;
            }
            if let Some(action) = scope.focus_action(direction).filter(|_| !shifted) {
                self.record_binding(KeybindMatch::Action(action), outcome);
                outcome.repaint = true;
                return;
            }
            match scope {
                ScopeMode::Pane => {
                    let action = match direction {
                        Direction::Left => KeybindAction::SwapPaneLeft,
                        Direction::Down => KeybindAction::SwapPaneDown,
                        Direction::Up => KeybindAction::SwapPaneUp,
                        Direction::Right => KeybindAction::SwapPaneRight,
                    };
                    self.record_binding(KeybindMatch::Action(action), outcome);
                }
                ScopeMode::Tab => {
                    let action = if direction.toward_end() {
                        KeybindAction::MoveTabNext
                    } else {
                        KeybindAction::MoveTabPrevious
                    };
                    self.record_binding(KeybindMatch::Action(action), outcome);
                }
                ScopeMode::Workspace => {
                    self.move_focused_workspace(direction.toward_end(), outcome);
                }
                // Agents follow the panel order, which the user does not arrange.
                ScopeMode::Agent | ScopeMode::Notification => {}
            }
            outcome.repaint = true;
            return;
        }

        if let Some((action, exits)) = scope.item_action(key) {
            if exits {
                self.mode = self.copy_or_terminal_mode();
            }
            self.record_binding(KeybindMatch::Action(action), outcome);
            outcome.repaint = true;
            return;
        }
        if scope == ScopeMode::Notification && plain_key(key, 'x') {
            self.dismiss_selected_notification(outcome);
            return;
        }
        if scope == ScopeMode::Notification && plain_key(key, 'a') {
            self.accept_selected_notification(outcome);
            return;
        }
        // A mode bound under the prefix closes or switches on its bare key. Item
        // keys are checked first so they keep working if the two overlap.
        if let Some(next) = self.scope_mode_key(key, false) {
            self.switch_scope_mode(scope, next, outcome);
        }
    }

    /// Notifications mode needs the log section, which is off by default.
    fn scope_mode_available(&self, scope: ScopeMode) -> bool {
        scope != ScopeMode::Notification || self.config.notifications.enabled
    }

    /// Opens a mode. Notifications mode starts with no entry highlighted.
    pub(super) fn enter_scope_mode(&mut self, scope: ScopeMode, outcome: &mut ClientShellInput) {
        if !self.scope_mode_available(scope) {
            return;
        }
        if scope == ScopeMode::Notification {
            self.selected_notification_id = None;
        }
        self.mode = ClientShellMode::Scope(scope);
        outcome.repaint = true;
    }

    fn scope_mode_key(&self, key: &TerminalKey, direct: bool) -> Option<ScopeMode> {
        let keybinds = &self.config.keybinds.keybinds;
        ScopeMode::ALL.into_iter().find(|mode| {
            let bindings = mode.bindings(keybinds);
            if direct {
                bindings.matches_direct_key(key)
            } else {
                bindings.matches_prefix_key(key)
            }
        })
    }

    fn switch_scope_mode(
        &mut self,
        current: ScopeMode,
        next: ScopeMode,
        outcome: &mut ClientShellInput,
    ) {
        if next == current {
            self.exit_scope_mode(outcome);
        } else {
            self.enter_scope_mode(next, outcome);
        }
    }

    fn exit_scope_mode(&mut self, outcome: &mut ClientShellInput) {
        self.mode = self.copy_or_terminal_mode();
        outcome.repaint = true;
    }

    /// Moves the focused workspace one place in the sidebar, wrapping at the
    /// ends like tab moves. A worktree group moves as one block.
    fn move_focused_workspace(&mut self, toward_end: bool, outcome: &mut ClientShellInput) {
        let Some(snapshot) = self.snapshot.as_deref() else {
            return;
        };
        let Some(workspace_id) = snapshot.focused_workspace_id.clone() else {
            return;
        };
        let roots = snapshot
            .workspaces
            .iter()
            .filter(|workspace| {
                !workspace
                    .worktree
                    .as_ref()
                    .is_some_and(|worktree| worktree.is_linked_worktree)
            })
            .map(|workspace| workspace.workspace_id.clone())
            .collect::<Vec<_>>();
        let Some(position) = roots.iter().position(|id| *id == workspace_id) else {
            return;
        };
        if roots.len() < 2 {
            return;
        }
        let before = if toward_end {
            if position + 1 == roots.len() {
                roots.first()
            } else {
                roots.get(position + 2)
            }
        } else if position == 0 {
            None
        } else {
            roots.get(position - 1)
        };
        if let Some(method) = self.workspace_move_method(&workspace_id, before.map(String::as_str))
        {
            self.reveal_workspace(&workspace_id);
            self.push_endpoint_method(method, outcome);
        }
    }
}
