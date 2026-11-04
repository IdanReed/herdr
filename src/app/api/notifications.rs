use crate::api::schema::{
    NotificationAcceptParams, NotificationAction, NotificationAddParams, NotificationLogEntry,
    NotificationSelectParams, ResponseResult, DEFAULT_NOTIFICATION_ACTION_LABEL,
};
use crate::app::notification_log::{
    NewNotification, NotificationAcceptError, MAX_NOTIFICATION_ACTION_ARGV_BYTES,
    MAX_NOTIFICATION_ACTION_LABEL_CHARS, MAX_NOTIFICATION_BODY_CHARS,
    MAX_NOTIFICATION_SOURCE_CHARS, MAX_NOTIFICATION_TITLE_CHARS,
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

/// Names the accepted entry for its action.
pub(crate) const NOTIFICATION_ID_ENV_VAR: &str = "HERDR_NOTIFICATION_ID";
/// Names the entry's workspace for its action, when the entry has one.
pub(crate) const NOTIFICATION_WORKSPACE_ID_ENV_VAR: &str = "HERDR_NOTIFICATION_WORKSPACE_ID";

/// Keeps argv verbatim and stores the label the entry shows.
fn normalized_action(
    action: Option<NotificationAction>,
) -> Result<Option<NotificationAction>, &'static str> {
    let Some(action) = action else {
        return Ok(None);
    };
    if action
        .argv
        .first()
        .is_none_or(|program| program.trim().is_empty())
    {
        return Err("notification action needs a program in argv");
    }
    if action.argv.iter().any(|arg| arg.contains('\0')) {
        return Err("notification action argv contains a NUL character");
    }
    if action.argv.iter().map(String::len).sum::<usize>() > MAX_NOTIFICATION_ACTION_ARGV_BYTES {
        return Err("notification action argv is too long");
    }
    let label = match action.label.as_deref() {
        Some(label) => sanitized_notification_text(label, MAX_NOTIFICATION_ACTION_LABEL_CHARS)
            .ok_or("notification action label is empty")?,
        None => DEFAULT_NOTIFICATION_ACTION_LABEL.to_owned(),
    };
    Ok(Some(NotificationAction {
        argv: action.argv,
        label: Some(label),
    }))
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
        let action = match normalized_action(params.action) {
            Ok(action) => action,
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
                action,
            },
            current_unix_ms(),
        );
        encode_success(
            id,
            ResponseResult::NotificationAdded {
                notification,
                instance: self.state.notification_log.instance().to_owned(),
            },
        )
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
                instance: self.state.notification_log.instance().to_owned(),
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

    /// Runs a pending action the way shell custom commands run, then marks the
    /// entry accepted and read. A failed start leaves the action pending.
    pub(super) fn handle_notification_accept(
        &mut self,
        id: String,
        params: NotificationAcceptParams,
    ) -> String {
        let entry_id = params.id.trim();
        let entry = match self.state.notification_log.acceptable(entry_id) {
            Ok(entry) => entry.clone(),
            Err(error) => {
                let (code, message) = match error {
                    NotificationAcceptError::NotFound => (
                        "notification_not_found",
                        format!("notification {entry_id} not found"),
                    ),
                    NotificationAcceptError::NoAction => (
                        "notification_no_action",
                        format!("notification {entry_id} has no action"),
                    ),
                    NotificationAcceptError::AlreadyAccepted => (
                        "notification_already_accepted",
                        format!("notification {entry_id} was already accepted"),
                    ),
                };
                return encode_error(id, code, message);
            }
        };
        let Some(mut command) = self.notification_action_command(&entry) else {
            return encode_error(
                id,
                "notification_no_action",
                format!("notification {entry_id} has no action"),
            );
        };
        match command.spawn() {
            Ok(child) => self.detached_process_children.push(child),
            Err(err) => {
                return encode_error(
                    id,
                    "notification_action_failed",
                    format!("failed to run the action of notification {entry_id}: {err}"),
                )
            }
        }
        match self.state.notification_log.mark_accepted(&entry.id) {
            Some(notification) => {
                encode_success(id, ResponseResult::NotificationAccepted { notification })
            }
            None => encode_error(
                id,
                "notification_not_found",
                format!("notification {entry_id} not found"),
            ),
        }
    }

    /// The detached process for an entry's action, with the entry named in its environment.
    pub(crate) fn notification_action_command(
        &self,
        entry: &NotificationLogEntry,
    ) -> Option<std::process::Command> {
        let (program, args) = entry.action.as_ref()?.argv.split_first()?;
        let mut command = std::process::Command::new(program);
        command.args(args);
        crate::platform::configure_background_command(&mut command);
        self.prepare_detached_command(&mut command);
        command.env(NOTIFICATION_ID_ENV_VAR, &entry.id);
        if let Some(workspace_id) = entry.workspace_id.as_deref() {
            command.env(NOTIFICATION_WORKSPACE_ID_ENV_VAR, workspace_id);
        }
        Some(command)
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
            action: None,
        })
    }

    fn list(app: &mut crate::app::App) -> Vec<crate::api::schema::NotificationLogEntry> {
        match result(app, Method::NotificationList(Default::default())) {
            ResponseResult::NotificationList { notifications, .. } => notifications,
            other => panic!("unexpected result {other:?}"),
        }
    }

    #[test]
    fn add_returns_the_entry_with_a_canonical_workspace_id() {
        let mut app = app();
        let second = app.public_workspace_id(1);
        // A 1-based index is a workspace target that other commands accept too.
        let ResponseResult::NotificationAdded { notification, .. } = result(
            &mut app,
            Method::NotificationAdd(NotificationAddParams {
                title: "  finished\nbuild ".into(),
                body: Some("exit\t0".into()),
                workspace_id: Some("2".into()),
                source: Some(" jobs ".into()),
                action: None,
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

    fn add_with_action(
        title: &str,
        workspace_id: Option<&str>,
        argv: &[&str],
        label: Option<&str>,
    ) -> Method {
        Method::NotificationAdd(NotificationAddParams {
            title: title.into(),
            body: None,
            workspace_id: workspace_id.map(str::to_owned),
            source: None,
            action: Some(crate::api::schema::NotificationAction {
                argv: argv.iter().map(|arg| (*arg).to_owned()).collect(),
                label: label.map(str::to_owned),
            }),
        })
    }

    fn accept(id: &str) -> Method {
        Method::NotificationAccept(crate::api::schema::NotificationAcceptParams { id: id.into() })
    }

    /// A program every test machine has that exits at once.
    fn harmless_argv() -> Vec<&'static str> {
        if cfg!(windows) {
            vec!["cmd", "/d", "/c", "exit 0"]
        } else {
            vec!["true"]
        }
    }

    #[test]
    fn list_shows_action_fields_even_when_an_entry_has_no_action() {
        let mut app = app();
        result(&mut app, add("plain", None, None));
        let ResponseResult::NotificationAdded { notification, .. } = result(
            &mut app,
            add_with_action("deploy ready", None, &["deploy", "--now"], None),
        ) else {
            panic!("expected notification_added");
        };
        let action = notification.action.as_ref().expect("action");
        assert_eq!(action.argv, ["deploy", "--now"]);
        // A missing label is stored as the default, so clients always have one.
        assert_eq!(action.label.as_deref(), Some("accept"));
        assert_eq!(
            notification.action_state,
            Some(crate::api::schema::NotificationActionState::Pending)
        );

        let listed = call(&mut app, Method::NotificationList(Default::default()));
        let entries = listed["result"]["notifications"].as_array().unwrap();
        assert_eq!(
            entries[0]["action"]["argv"],
            serde_json::json!(["deploy", "--now"])
        );
        assert_eq!(entries[0]["action"]["label"], "accept");
        assert_eq!(entries[0]["action_state"], "pending");
        assert!(entries[1]["action"].is_null());
        assert!(entries[1]["action_state"].is_null());
        assert!(entries[1].as_object().unwrap().contains_key("action"));
        assert!(entries[1].as_object().unwrap().contains_key("action_state"));
    }

    #[test]
    fn add_rejects_actions_without_a_program_or_with_an_empty_label() {
        let mut app = app();
        let too_long =
            "x".repeat(crate::app::notification_log::MAX_NOTIFICATION_ACTION_ARGV_BYTES + 1);
        for method in [
            add_with_action("t", None, &[], None),
            add_with_action("t", None, &[" "], None),
            add_with_action("t", None, &["run", "a\0b"], None),
            add_with_action("t", None, &["run", too_long.as_str()], None),
            add_with_action("t", None, &["run"], Some(" \n ")),
        ] {
            let response = call(&mut app, method);
            assert_eq!(response["error"]["code"], "invalid_params", "{response}");
        }
        assert!(list(&mut app).is_empty());

        let ResponseResult::NotificationAdded { notification, .. } = result(
            &mut app,
            add_with_action("t", None, &["run"], Some("  open\tlog ")),
        ) else {
            panic!("expected notification_added");
        };
        assert_eq!(
            notification.action.unwrap().label.as_deref(),
            Some("open log")
        );
    }

    #[test]
    fn action_command_runs_argv_directly_and_names_the_entry() {
        let mut app = app();
        let second = app.public_workspace_id(1);
        let ResponseResult::NotificationAdded { notification, .. } = result(
            &mut app,
            add_with_action(
                "deploy",
                Some(&second),
                &["deploy", "--to", "two words"],
                None,
            ),
        ) else {
            panic!("expected notification_added");
        };
        let command = app
            .notification_action_command(&notification)
            .expect("action command");
        assert_eq!(command.get_program(), "deploy");
        assert_eq!(
            command.get_args().collect::<Vec<_>>(),
            ["--to", "two words"]
        );
        let env = command
            .get_envs()
            .filter_map(|(key, value)| {
                Some((key.to_str()?.to_owned(), value?.to_str()?.to_owned()))
            })
            .collect::<std::collections::HashMap<_, _>>();
        assert_eq!(
            env.get(super::NOTIFICATION_ID_ENV_VAR).map(String::as_str),
            Some(notification.id.as_str())
        );
        assert_eq!(
            env.get(super::NOTIFICATION_WORKSPACE_ID_ENV_VAR),
            Some(&second)
        );
        // Shell custom commands get these too.
        assert!(env.contains_key(crate::api::SOCKET_PATH_ENV_VAR));
        assert!(env.contains_key("HERDR_ACTIVE_WORKSPACE_ID"));

        let ResponseResult::NotificationAdded { notification, .. } =
            result(&mut app, add_with_action("global", None, &["run"], None))
        else {
            panic!("expected notification_added");
        };
        let command = app.notification_action_command(&notification).unwrap();
        assert!(!command
            .get_envs()
            .any(|(key, _)| key == super::NOTIFICATION_WORKSPACE_ID_ENV_VAR));
    }

    #[test]
    fn accept_runs_the_action_once_and_marks_the_entry_read() {
        let mut app = app();
        result(&mut app, add("plain", None, None));
        result(
            &mut app,
            add_with_action("go", None, &harmless_argv(), None),
        );
        let revision = app.state.notification_log.revision();

        assert_eq!(
            call(&mut app, accept("n9"))["error"]["code"],
            "notification_not_found"
        );
        assert_eq!(
            call(&mut app, accept("n1"))["error"]["code"],
            "notification_no_action"
        );
        assert_eq!(app.state.notification_log.revision(), revision);

        let children = app.detached_process_children.len();
        let ResponseResult::NotificationAccepted { notification } =
            result(&mut app, accept(" n2 "))
        else {
            panic!("expected notification_accepted");
        };
        assert_eq!(app.detached_process_children.len(), children + 1);
        assert_eq!(
            notification.action_state,
            Some(crate::api::schema::NotificationActionState::Accepted)
        );
        assert!(notification.read);
        assert_eq!(app.state.notification_log.revision(), revision + 1);
        assert_eq!(list(&mut app)[0], notification);

        assert_eq!(
            call(&mut app, accept("n2"))["error"]["code"],
            "notification_already_accepted"
        );
        assert_eq!(app.detached_process_children.len(), children + 1);
        for child in &mut app.detached_process_children {
            let _ = child.wait();
        }
    }

    #[test]
    fn a_failed_start_leaves_the_action_pending() {
        let mut app = app();
        result(
            &mut app,
            add_with_action(
                "go",
                None,
                &["herdr-test-program-that-does-not-exist"],
                None,
            ),
        );

        let response = call(&mut app, accept("n1"));
        assert_eq!(response["error"]["code"], "notification_action_failed");
        let entry = &list(&mut app)[0];
        assert!(entry.pending_action().is_some());
        assert!(!entry.read);
    }

    #[test]
    fn add_and_list_name_the_log_instance_until_the_server_restarts() {
        let mut app = app();
        let ResponseResult::NotificationAdded { instance, .. } =
            result(&mut app, add("a", None, None))
        else {
            panic!("expected notification_added");
        };
        assert!(!instance.is_empty());
        let listed = call(&mut app, Method::NotificationList(Default::default()));
        assert_eq!(listed["result"]["instance"], instance.as_str());

        // An empty log after a dismiss is the same instance.
        result(&mut app, Method::NotificationDismiss(Default::default()));
        let listed = call(&mut app, Method::NotificationList(Default::default()));
        assert!(listed["result"]["notifications"]
            .as_array()
            .unwrap()
            .is_empty());
        assert_eq!(listed["result"]["instance"], instance.as_str());

        // A restarted server starts a new log with a new instance.
        let mut restarted = super::tests::app();
        let listed = call(&mut restarted, Method::NotificationList(Default::default()));
        assert_ne!(listed["result"]["instance"], instance.as_str());
    }
}
