use clap::Command;

use crate::api::schema::common::NotificationShowProjection;
use crate::api::schema::notifications::{NotificationAddProjection, NotificationSelectProjection};
use crate::api::schema::{EmptyParams, Method, Request};

use super::super::projection::{self, cli_projection, Leaf, ParseError};

cli_projection! {
    pub(crate) NotificationListProjection for EmptyParams {}
}

const LEAVES: [Leaf; 5] = [
    Leaf {
        name: "show",
        about: "Show a notification",
        id: "cli:notification:show",
        arguments: NotificationShowProjection::arguments,
        method: |matches| {
            NotificationShowProjection::from_matches(matches).map(Method::NotificationShow)
        },
    },
    Leaf {
        name: "add",
        about: "Add an entry to the notification log",
        id: "cli:notification:add",
        arguments: NotificationAddProjection::arguments,
        method: |matches| {
            NotificationAddProjection::from_matches(matches).map(Method::NotificationAdd)
        },
    },
    Leaf {
        name: "list",
        about: "List notification log entries, newest first",
        id: "cli:notification:list",
        arguments: NotificationListProjection::arguments,
        method: |matches| {
            NotificationListProjection::from_matches(matches).map(Method::NotificationList)
        },
    },
    Leaf {
        name: "dismiss",
        about: "Remove log entries by id or source, or all entries",
        id: "cli:notification:dismiss",
        arguments: NotificationSelectProjection::arguments,
        method: |matches| {
            NotificationSelectProjection::from_matches(matches).map(Method::NotificationDismiss)
        },
    },
    Leaf {
        name: "mark-read",
        about: "Mark log entries read by id or source, or all entries",
        id: "cli:notification:mark-read",
        arguments: NotificationSelectProjection::arguments,
        method: |matches| {
            NotificationSelectProjection::from_matches(matches).map(Method::NotificationMarkRead)
        },
    },
];

pub(crate) fn command() -> Command {
    Command::new("notification")
        .about("Show Herdr notifications and manage the notification log")
        .subcommand_required(true)
        .subcommands(LEAVES.iter().map(Leaf::command))
}

pub(crate) fn request(args: &[String]) -> Result<Request, ParseError> {
    // A bare `help` title historically asks for usage instead of showing a notification.
    // The title is a verbatim slot, so route through the help subcommand.
    let help_args;
    let args = match args {
        [show, title, ..] if show == "show" && title == "help" => {
            help_args = ["help".to_owned(), show.clone()];
            &help_args[..]
        }
        _ => args,
    };
    projection::group_request(command(), &LEAVES, args)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Request, ParseError> {
        request(&args.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>())
    }

    #[test]
    fn notification_projection_preserves_request_literals_and_last_wins() {
        for (args, expected) in [
            (
                vec!["show", "build failed"],
                serde_json::json!({"id":"cli:notification:show", "method":"notification.show", "params":{"title":"build failed"}}),
            ),
            (
                vec![
                    "show",
                    "--literal-title",
                    "--body",
                    "first",
                    "--body",
                    "--literal-body",
                    "--position",
                    "bottom-left",
                    "--position",
                    "top-right",
                    "--sound",
                    "done",
                    "--sound",
                    "request",
                ],
                serde_json::json!({"id":"cli:notification:show", "method":"notification.show", "params":{"title":"--literal-title", "body":"--literal-body", "position":"top-right", "sound":"request"}}),
            ),
            (
                vec!["show", "title", "--sound=none"],
                serde_json::json!({"id":"cli:notification:show", "method":"notification.show", "params":{"title":"title"}}),
            ),
        ] {
            assert_eq!(
                serde_json::to_value(parse(&args).unwrap()).unwrap(),
                expected
            );
        }
    }

    #[test]
    fn notification_projection_preserves_all_cli_enum_values() {
        for position in ["top-left", "top-right", "bottom-left", "bottom-right"] {
            assert_eq!(
                serde_json::to_value(parse(&["show", "title", "--position", position]).unwrap())
                    .unwrap()["params"]["position"],
                position
            );
        }
        for sound in ["done", "request"] {
            assert_eq!(
                serde_json::to_value(parse(&["show", "title", "--sound", sound]).unwrap()).unwrap()
                    ["params"]["sound"],
                sound
            );
        }
        command().debug_assert();
    }

    #[test]
    fn notification_projection_clap_validation_and_help_before_transport() {
        for args in [
            vec!["show"],
            vec!["show", "title", "extra"],
            vec!["show", "title", "--body"],
            vec!["show", "title", "--position", "top_right"],
            vec!["show", "title", "--sound", "DONE"],
        ] {
            assert!(matches!(parse(&args), Err(ParseError::Clap(error)) if error.exit_code() == 2));
        }
        assert!(
            matches!(parse(&["show", "title", "--help"]), Err(ParseError::Clap(error)) if error.exit_code() == 0)
        );
        assert!(
            matches!(parse(&["show", "help"]), Err(ParseError::Clap(error)) if error.exit_code() == 0)
        );
        assert_eq!(
            serde_json::to_value(parse(&["show", "title", "--sound=request"]).unwrap()).unwrap(),
            serde_json::json!({"id":"cli:notification:show", "method":"notification.show", "params":{"title":"title", "sound":"request"}})
        );
    }

    #[test]
    fn notification_log_commands_build_their_requests() {
        for (args, expected) in [
            (
                vec!["add", "--literal-title"],
                serde_json::json!({"id":"cli:notification:add", "method":"notification.add", "params":{"title":"--literal-title"}}),
            ),
            (
                vec![
                    "add",
                    "finished build",
                    "--body",
                    "exit 0",
                    "--workspace",
                    "w2",
                    "--source",
                    "jobs",
                ],
                serde_json::json!({"id":"cli:notification:add", "method":"notification.add", "params":{"title":"finished build", "body":"exit 0", "workspace_id":"w2", "source":"jobs"}}),
            ),
            (
                vec!["list"],
                serde_json::json!({"id":"cli:notification:list", "method":"notification.list", "params":{}}),
            ),
            (
                vec!["dismiss"],
                serde_json::json!({"id":"cli:notification:dismiss", "method":"notification.dismiss", "params":{}}),
            ),
            (
                vec!["dismiss", "--id", "n3", "--source", "jobs"],
                serde_json::json!({"id":"cli:notification:dismiss", "method":"notification.dismiss", "params":{"id":"n3", "source":"jobs"}}),
            ),
            (
                vec!["mark-read", "--source", "jobs"],
                serde_json::json!({"id":"cli:notification:mark-read", "method":"notification.mark_read", "params":{"source":"jobs"}}),
            ),
        ] {
            assert_eq!(
                serde_json::to_value(parse(&args).unwrap()).unwrap(),
                expected,
                "{args:?}"
            );
        }
        for args in [
            vec!["add"],
            vec!["add", "title", "extra"],
            vec!["list", "extra"],
            vec!["dismiss", "n3"],
            vec!["mark-read", "--id"],
        ] {
            assert!(
                matches!(parse(&args), Err(ParseError::Clap(error)) if error.exit_code() == 2),
                "{args:?}"
            );
        }
    }
}
