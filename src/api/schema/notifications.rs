//! Notification log: short entries that scripts post and clients list.
//!
//! `notification.show` raises a transient toast. The log is separate: entries
//! stay until they are dismissed or pushed out by newer ones.

use serde::{Deserialize, Serialize};

use crate::cli::projection::{cli_params, optional_text, Field, Input};

fn title() -> Input<String> {
    Input::positional("title", 0, |slot, value| {
        *slot = value.to_owned();
        Ok(())
    })
    .value_name("TITLE")
    .verbatim()
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
    }
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
}
