//! Notification log: short entries that scripts post and clients list.
//!
//! `notification.show` raises a transient toast. The log is separate: entries
//! stay until they are dismissed or pushed out by newer ones. An entry may
//! carry an action, a command that runs once when the user accepts the entry.

use serde::{Deserialize, Serialize};

use crate::cli::projection::{cli_params, optional_text, Field, Input, ParseError};

/// Label an action gets when `notification.add` names none.
pub const DEFAULT_NOTIFICATION_ACTION_LABEL: &str = "accept";

fn title() -> Input<String> {
    Input::positional("title", 0, |slot, value| {
        *slot = value.to_owned();
        Ok(())
    })
    .value_name("TITLE")
    .verbatim()
}

fn action_label() -> Input<Option<NotificationAction>> {
    Input::value(
        "action-label",
        "TEXT",
        |slot: &mut Option<NotificationAction>, value| {
            slot.get_or_insert_with(NotificationAction::default).label = Some(value.to_owned());
            Ok(())
        },
    )
    .order(3)
    .help("Label shown with the action [default: accept]")
}

fn action_argv() -> Input<Option<NotificationAction>> {
    Input::positional(
        "action_argv",
        1,
        |slot: &mut Option<NotificationAction>, value| {
            slot.get_or_insert_with(NotificationAction::default)
                .argv
                .push(value.to_owned());
            Ok(())
        },
    )
    .value_name("ARGV")
    .configure(|arg| arg.required(false).num_args(0..).last(true))
    .help("Command the entry runs when it is accepted, without a shell")
}

fn validate_action(action: &Option<NotificationAction>) -> Result<(), ParseError> {
    match action {
        Some(action) if action.argv.is_empty() => Err(ParseError::Usage(
            "--action-label needs a command after --".into(),
        )),
        _ => Ok(()),
    }
}

cli_params! {
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
    pub struct NotificationAddParams => NotificationAddProjection {
        pub title: String => Field::new(String::new()).input(title()),
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub body: Option<String> => Field::new(None).input(optional_text("body", "TEXT").order(0)),
        /// Workspace the entry opens. Accepts the same ids as `workspace.get`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub workspace_id: Option<String> => Field::new(None)
            .input(super::tabs::workspace_option("WORKSPACE_ID").order(1)),
        /// Free-text tag naming the poster, used to dismiss its entries together.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub source: Option<String> => Field::new(None).input(optional_text("source", "SOURCE").order(2)),
        /// Command the entry runs once when the user accepts it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub action: Option<NotificationAction> => Field::new(None)
            .input(action_label())
            .input(action_argv())
            .validate(validate_action),
    }
}

/// A command attached to a log entry. `notification.accept` runs it once.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct NotificationAction {
    /// Program and arguments. The program runs directly, without a shell.
    pub argv: Vec<String>,
    /// Short label shown with the entry. Defaults to `accept`; listed entries always carry it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

impl NotificationAction {
    pub fn label(&self) -> &str {
        self.label
            .as_deref()
            .unwrap_or(DEFAULT_NOTIFICATION_ACTION_LABEL)
    }
}

/// Whether an entry's action still waits for the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum NotificationActionState {
    Pending,
    Accepted,
}

cli_params! {
    /// Selects log entries by id, by source, or both. With neither, it selects every entry.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema, Default)]
    pub struct NotificationSelectParams => NotificationSelectProjection {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub id: Option<String> => Field::new(None).input(optional_text("id", "ID").order(0)),
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub source: Option<String> => Field::new(None).input(optional_text("source", "SOURCE").order(1)),
    }
}

cli_params! {
    /// Runs the action of one log entry, then marks the entry accepted and read.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
    pub struct NotificationAcceptParams => NotificationAcceptProjection {
        pub id: String => Field::new(String::new()).input(
            Input::positional("id", 0, |slot, value| {
                *slot = value.to_owned();
                Ok(())
            })
            .value_name("ID"),
        ),
    }
}

/// One notification log entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct NotificationLogEntry {
    /// Server-assigned id, unique within one server run.
    pub id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    /// Canonical id of the linked workspace. The workspace may have closed since.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    pub created_unix_ms: u64,
    #[serde(default)]
    pub read: bool,
    /// Command the entry runs when accepted, or null.
    #[serde(default)]
    pub action: Option<NotificationAction>,
    /// `pending` until the action runs, then `accepted`. Null when there is no action.
    #[serde(default)]
    pub action_state: Option<NotificationActionState>,
}

impl NotificationLogEntry {
    /// The action, while it still waits to run.
    pub fn pending_action(&self) -> Option<&NotificationAction> {
        self.action
            .as_ref()
            .filter(|_| self.action_state == Some(NotificationActionState::Pending))
    }
}
