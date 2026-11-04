use crate::api::schema::{NotificationAddParams, NotificationSelectParams, ResponseResult};
use crate::app::notification_log::{
    NewNotification, MAX_NOTIFICATION_BODY_CHARS, MAX_NOTIFICATION_SOURCE_CHARS,
    MAX_NOTIFICATION_TITLE_CHARS,
};
use crate::app::App;

use super::responses::{encode_error, encode_success};
use super::sanitized_notification_text;

fn invalid_params(id: String, message: &str) -> String {
    encode_error(id, "invalid_params", message)
}

/// Sources match exactly after the same cleanup that `notification.add` applies.
fn normalized_source(source: Option<String>) -> Result<Option<String>, &'static str> {
    source
        .map(|source| {
            sanitized_notification_text(&source, MAX_NOTIFICATION_SOURCE_CHARS)
                .ok_or("notification source is empty")
        })
        .transpose()
}

fn normalized_select(
    params: NotificationSelectParams,
) -> Result<NotificationSelectParams, &'static str> {
    Ok(NotificationSelectParams {
        id: params.id.map(|id| id.trim().to_owned()),
        source: normalized_source(params.source)?,
    })
}

fn current_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or(0)
}

impl App {
    pub(super) fn handle_notification_add(
        &mut self,
        id: String,
        params: NotificationAddParams,
    ) -> String {
        let Some(title) = sanitized_notification_text(&params.title, MAX_NOTIFICATION_TITLE_CHARS)
        else {
            return invalid_params(id, "notification title is empty");
        };
        let body = params
            .body
            .as_deref()
            .and_then(|body| sanitized_notification_text(body, MAX_NOTIFICATION_BODY_CHARS));
        let source = match normalized_source(params.source) {
            Ok(source) => source,
            Err(message) => return invalid_params(id, message),
        };
        // Store the canonical id so the entry keeps pointing at this workspace
        // even after other workspaces are added, moved, or closed.
        let workspace_id = match params.workspace_id {
            Some(target) => match self.parse_workspace_id(&target) {
                Some(index) => Some(self.public_workspace_id(index)),
                None => {
                    return encode_error(
                        id,
                        "workspace_not_found",
                        format!("workspace {target} not found"),
                    )
                }
            },
            None => None,
        };
        let notification = self.state.notification_log.add(
            NewNotification {
                title,
                body,
                workspace_id,
                source,
            },
            current_unix_ms(),
        );
        encode_success(id, ResponseResult::NotificationAdded { notification })
    }

    pub(super) fn handle_notification_list(&self, id: String) -> String {
        encode_success(
            id,
            ResponseResult::NotificationList {
                notifications: self
                    .state
                    .notification_log
                    .newest_first()
                    .cloned()
                    .collect(),
            },
        )
    }

    pub(super) fn handle_notification_dismiss(
        &mut self,
        id: String,
        params: NotificationSelectParams,
    ) -> String {
        match normalized_select(params) {
            Ok(select) => encode_success(
                id,
                ResponseResult::NotificationDismissed {
                    removed: self.state.notification_log.dismiss(&select),
                },
            ),
            Err(message) => invalid_params(id, message),
        }
    }

    pub(super) fn handle_notification_mark_read(
        &mut self,
        id: String,
        params: NotificationSelectParams,
    ) -> String {
        match normalized_select(params) {
            Ok(select) => encode_success(
                id,
                ResponseResult::NotificationMarkedRead {
                    marked: self.state.notification_log.mark_read(&select),
                },
            ),
            Err(message) => invalid_params(id, message),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::api::schema::{
        Method, NotificationAddParams, NotificationSelectParams, Request, ResponseResult,
        SuccessResponse,
    };
    use crate::app::notification_log::MAX_NOTIFICATION_LOG_ENTRIES;

    fn app() -> crate::app::App {
        let (_api_tx, api_rx) = tokio::sync::mpsc::unbounded_channel();
        let mut app = crate::app::App::new(
            &crate::config::Config::default(),
            crate::app::AppPolicy::TEST,
            None,
            api_rx,
            crate::api::EventHub::default(),
        );
        app.state.workspaces = vec![
            crate::workspace::Workspace::test_new("first"),
            crate::workspace::Workspace::test_new("second"),
        ];
        app.state.active = Some(0);
        app.state.ensure_test_terminals();
        app
    }

    fn call(app: &mut crate::app::App, method: Method) -> serde_json::Value {
        serde_json::from_str(&app.handle_api_request(Request {
            id: "test".into(),
            method,
        }))
        .unwrap()
    }

    fn result(app: &mut crate::app::App, method: Method) -> ResponseResult {
        let response = app.handle_api_request(Request {
            id: "test".into(),
            method,
        });
        serde_json::from_str::<SuccessResponse>(&response)
            .unwrap_or_else(|_| panic!("expected success, got {response}"))
            .result
    }

    fn add(title: &str, workspace_id: Option<&str>, source: Option<&str>) -> Method {
        Method::NotificationAdd(NotificationAddParams {
            title: title.into(),
            body: None,
            workspace_id: workspace_id.map(str::to_owned),
            source: source.map(str::to_owned),
        })
    }

    fn list(app: &mut crate::app::App) -> Vec<crate::api::schema::NotificationLogEntry> {
        match result(app, Method::NotificationList(Default::default())) {
            ResponseResult::NotificationList { notifications } => notifications,
            other => panic!("unexpected result {other:?}"),
        }
    }

    #[test]
    fn add_returns_the_entry_with_a_canonical_workspace_id() {
        let mut app = app();
        let second = app.public_workspace_id(1);
        // A 1-based index is a workspace target that other commands accept too.
        let ResponseResult::NotificationAdded { notification } = result(
            &mut app,
            Method::NotificationAdd(NotificationAddParams {
                title: "  finished\nbuild ".into(),
                body: Some("exit\t0".into()),
                workspace_id: Some("2".into()),
                source: Some(" jobs ".into()),
            }),
        ) else {
            panic!("expected notification_added");
        };

        assert_eq!(notification.id, "n1");
        assert_eq!(notification.title, "finished build");
        assert_eq!(notification.body.as_deref(), Some("exit 0"));
        assert_eq!(notification.workspace_id.as_deref(), Some(second.as_str()));
        assert_eq!(notification.source.as_deref(), Some("jobs"));
        assert!(!notification.read);
        assert!(notification.created_unix_ms > 0);
    }

    #[test]
    fn add_rejects_empty_titles_and_unknown_workspaces() {
        let mut app = app();
        let response = call(&mut app, add(" \n ", None, None));
        assert_eq!(response["error"]["code"], "invalid_params");
        let response = call(&mut app, add("title", Some("w-missing"), None));
        assert_eq!(response["error"]["code"], "workspace_not_found");
        let response = call(&mut app, add("title", None, Some("\u{7}")));
        assert_eq!(response["error"]["code"], "invalid_params");
        assert!(list(&mut app).is_empty());
    }

    #[test]
    fn list_is_newest_first_and_bounded() {
        let mut app = app();
        for index in 0..MAX_NOTIFICATION_LOG_ENTRIES + 1 {
            result(&mut app, add(&format!("entry {index}"), None, None));
        }
        let entries = list(&mut app);

        assert_eq!(entries.len(), MAX_NOTIFICATION_LOG_ENTRIES);
        assert_eq!(
            entries[0].title,
            format!("entry {MAX_NOTIFICATION_LOG_ENTRIES}")
        );
        assert_eq!(entries.last().unwrap().title, "entry 1");
    }

    #[test]
    fn dismiss_and_mark_read_use_the_same_selectors() {
        let mut app = app();
        result(&mut app, add("a", None, Some("jobs")));
        result(&mut app, add("b", None, Some("jobs")));
        result(&mut app, add("c", None, Some("build")));

        let select = |id: Option<&str>, source: Option<&str>| NotificationSelectParams {
            id: id.map(str::to_owned),
            source: source.map(str::to_owned),
        };
        assert_eq!(
            result(
                &mut app,
                Method::NotificationMarkRead(select(None, Some(" jobs")))
            ),
            ResponseResult::NotificationMarkedRead { marked: 2 }
        );
        assert_eq!(
            list(&mut app)
                .iter()
                .map(|entry| entry.read)
                .collect::<Vec<_>>(),
            [false, true, true]
        );
        assert_eq!(
            result(
                &mut app,
                Method::NotificationDismiss(select(Some("n3"), None))
            ),
            ResponseResult::NotificationDismissed { removed: 1 }
        );
        // Unknown ids are not an error, so two clients may dismiss the same entry.
        assert_eq!(
            result(
                &mut app,
                Method::NotificationDismiss(select(Some("n3"), None))
            ),
            ResponseResult::NotificationDismissed { removed: 0 }
        );
        assert_eq!(
            result(&mut app, Method::NotificationDismiss(select(None, None))),
            ResponseResult::NotificationDismissed { removed: 2 }
        );
        assert!(list(&mut app).is_empty());
    }

    #[test]
    fn entries_outlive_their_workspace() {
        let mut app = app();
        let second = app.public_workspace_id(1);
        result(&mut app, add("done", Some(&second), None));
        result(
            &mut app,
            Method::WorkspaceClose(crate::api::schema::WorkspaceCloseParams {
                workspace_id: second.clone(),
                close_group: false,
            }),
        );

        assert!(app.parse_workspace_id(&second).is_none());
        let entries = list(&mut app);
        assert_eq!(entries[0].workspace_id.as_deref(), Some(second.as_str()));
        let ResponseResult::NotificationMarkedRead { marked } = result(
            &mut app,
            Method::NotificationMarkRead(NotificationSelectParams {
                id: Some(entries[0].id.clone()),
                source: None,
            }),
        ) else {
            panic!("expected notification_marked_read");
        };
        assert_eq!(marked, 1);
    }
}
