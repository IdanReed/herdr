//! Bounded log of notification entries posted through the API.
//!
//! The log is session state held by the server. Clients receive it as a
//! projection and change it only through API methods.

use std::collections::VecDeque;

use crate::api::schema::{NotificationLogEntry, NotificationSelectParams};

/// Oldest entries are dropped once the log holds this many.
pub(crate) const MAX_NOTIFICATION_LOG_ENTRIES: usize = 100;
pub(crate) const MAX_NOTIFICATION_TITLE_CHARS: usize = 80;
pub(crate) const MAX_NOTIFICATION_BODY_CHARS: usize = 240;
pub(crate) const MAX_NOTIFICATION_SOURCE_CHARS: usize = 80;

/// Fields of a new entry after validation. The log assigns id and time.
pub(crate) struct NewNotification {
    pub(crate) title: String,
    pub(crate) body: Option<String>,
    pub(crate) workspace_id: Option<String>,
    pub(crate) source: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct NotificationLog {
    /// Oldest first, so appending and dropping the oldest are both O(1).
    entries: VecDeque<NotificationLogEntry>,
    next_id: u64,
    /// Changes whenever an entry is added, removed, or marked read.
    revision: u64,
}

impl NotificationLog {
    pub(crate) fn revision(&self) -> u64 {
        self.revision
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }

    /// Entries newest first.
    pub(crate) fn newest_first(&self) -> impl Iterator<Item = &NotificationLogEntry> {
        self.entries.iter().rev()
    }

    pub(crate) fn add(
        &mut self,
        notification: NewNotification,
        created_unix_ms: u64,
    ) -> NotificationLogEntry {
        self.next_id = self.next_id.saturating_add(1);
        let entry = NotificationLogEntry {
            id: format!("n{}", self.next_id),
            title: notification.title,
            body: notification.body,
            workspace_id: notification.workspace_id,
            source: notification.source,
            created_unix_ms,
            read: false,
        };
        if self.entries.len() >= MAX_NOTIFICATION_LOG_ENTRIES {
            self.entries.pop_front();
        }
        self.entries.push_back(entry.clone());
        self.revision = self.revision.saturating_add(1);
        entry
    }

    /// Removes the selected entries and returns how many were removed.
    pub(crate) fn dismiss(&mut self, select: &NotificationSelectParams) -> usize {
        let before = self.entries.len();
        self.entries.retain(|entry| !selects(select, entry));
        let removed = before - self.entries.len();
        if removed > 0 {
            self.revision = self.revision.saturating_add(1);
        }
        removed
    }

    /// Marks the selected unread entries read and returns how many changed.
    pub(crate) fn mark_read(&mut self, select: &NotificationSelectParams) -> usize {
        let mut marked = 0;
        for entry in self
            .entries
            .iter_mut()
            .filter(|entry| !entry.read && selects(select, entry))
        {
            entry.read = true;
            marked += 1;
        }
        if marked > 0 {
            self.revision = self.revision.saturating_add(1);
        }
        marked
    }
}

fn selects(select: &NotificationSelectParams, entry: &NotificationLogEntry) -> bool {
    select.id.as_ref().is_none_or(|id| *id == entry.id)
        && select
            .source
            .as_ref()
            .is_none_or(|source| entry.source.as_ref() == Some(source))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(title: &str, source: Option<&str>) -> NewNotification {
        NewNotification {
            title: title.into(),
            body: None,
            workspace_id: None,
            source: source.map(str::to_owned),
        }
    }

    fn select(id: Option<&str>, source: Option<&str>) -> NotificationSelectParams {
        NotificationSelectParams {
            id: id.map(str::to_owned),
            source: source.map(str::to_owned),
        }
    }

    fn titles(log: &NotificationLog) -> Vec<&str> {
        log.newest_first()
            .map(|entry| entry.title.as_str())
            .collect()
    }

    #[test]
    fn add_assigns_unique_ids_and_lists_newest_first() {
        let mut log = NotificationLog::default();
        let first = log.add(note("first", None), 10);
        let second = log.add(note("second", None), 20);

        assert_eq!(first.id, "n1");
        assert_eq!(second.id, "n2");
        assert!(!second.read);
        assert_eq!(second.created_unix_ms, 20);
        assert_eq!(titles(&log), ["second", "first"]);
        assert_eq!(log.revision(), 2);
    }

    #[test]
    fn log_keeps_the_newest_entries_up_to_the_bound() {
        let mut log = NotificationLog::default();
        for index in 0..MAX_NOTIFICATION_LOG_ENTRIES + 5 {
            log.add(note(&format!("entry {index}"), None), index as u64);
        }

        assert_eq!(log.len(), MAX_NOTIFICATION_LOG_ENTRIES);
        let newest = log.newest_first().next().unwrap();
        assert_eq!(
            newest.title,
            format!("entry {}", MAX_NOTIFICATION_LOG_ENTRIES + 4)
        );
        assert_eq!(log.newest_first().last().unwrap().title, "entry 5");
        // Ids are never reused after old entries drop out.
        assert_eq!(newest.id, format!("n{}", MAX_NOTIFICATION_LOG_ENTRIES + 5));
    }

    #[test]
    fn dismiss_selects_by_id_source_both_or_all() {
        let mut log = NotificationLog::default();
        log.add(note("a", Some("jobs")), 1);
        log.add(note("b", Some("jobs")), 2);
        log.add(note("c", Some("build")), 3);
        log.add(note("d", None), 4);

        assert_eq!(log.dismiss(&select(Some("n1"), Some("build"))), 0);
        assert_eq!(log.dismiss(&select(Some("n3"), None)), 1);
        assert_eq!(titles(&log), ["d", "b", "a"]);
        assert_eq!(log.dismiss(&select(None, Some("jobs"))), 2);
        assert_eq!(titles(&log), ["d"]);
        assert_eq!(log.dismiss(&select(None, None)), 1);
        assert_eq!(log.len(), 0);
    }

    #[test]
    fn revision_changes_only_when_entries_change() {
        let mut log = NotificationLog::default();
        log.add(note("a", Some("jobs")), 1);
        let revision = log.revision();

        assert_eq!(log.dismiss(&select(Some("missing"), None)), 0);
        assert_eq!(log.mark_read(&select(None, Some("other"))), 0);
        assert_eq!(log.revision(), revision);

        assert_eq!(log.mark_read(&select(Some("n1"), None)), 1);
        assert!(log.newest_first().next().unwrap().read);
        assert_eq!(log.revision(), revision + 1);
        // Marking an entry that is already read is not a change.
        assert_eq!(log.mark_read(&select(None, None)), 0);
        assert_eq!(log.revision(), revision + 1);
    }
}
