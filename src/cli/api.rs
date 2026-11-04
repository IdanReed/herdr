const API_SCHEMA_JSON: &str = include_str!("../../docs/next/api/herdr-api.schema.json");

use crate::api::schema::{EmptyParams, Method, Request};

pub(super) fn run_api_command(args: &[String]) -> std::io::Result<i32> {
    let Some(subcommand) = args.first().map(String::as_str) else {
        print_api_help();
        return Ok(2);
    };

    match subcommand {
        "call" => api_call(&args[1..]),
        "schema" => api_schema(&args[1..]),
        "snapshot" => api_snapshot(&args[1..]),
        "help" | "--help" | "-h" => {
            print_api_help();
            Ok(0)
        }
        _ => {
            print_api_help();
            Ok(2)
        }
    }
}

fn api_schema(args: &[String]) -> std::io::Result<i32> {
    match args {
        [] => {
            print!("{}", schema_summary_text()?);
        }
        [flag] if flag == "--json" => {
            print!("{API_SCHEMA_JSON}");
        }
        [flag, path] if flag == "--output" => {
            write_schema_file(std::path::Path::new(path))?;
            println!("wrote API schema to {path}");
        }
        [flag] if flag == "--output" => {
            eprintln!("missing value for --output");
            return Ok(2);
        }
        [flag] if matches!(flag.as_str(), "help" | "--help" | "-h") => {
            print_api_schema_help();
        }
        [other] if other.starts_with('-') => {
            eprintln!("unknown option: {other}");
            return Ok(2);
        }
        _ => {
            print_api_schema_help();
            return Ok(2);
        }
    }
    Ok(0)
}

fn api_snapshot(args: &[String]) -> std::io::Result<i32> {
    if !args.is_empty() {
        eprintln!("usage: herdr api snapshot");
        return Ok(2);
    }

    super::print_response(&super::send_request(&Request {
        id: "cli:api:snapshot".into(),
        method: Method::SessionSnapshot(EmptyParams::default()),
    })?)
}

const API_CALL_REQUEST_ID: &str = "cli:api:call";
const API_CALL_USAGE: &str = "usage: herdr api call <METHOD> [PARAMS_JSON | -]";

fn api_call(args: &[String]) -> std::io::Result<i32> {
    if matches!(
        args.first().map(String::as_str),
        Some("help" | "--help" | "-h")
    ) {
        print_api_call_help();
        return Ok(0);
    }
    let request = match parse_api_call(args, || {
        let mut params = String::new();
        std::io::Read::read_to_string(&mut std::io::stdin(), &mut params)?;
        Ok(params)
    }) {
        Ok(request) => request,
        Err(ApiCallError::Usage(message)) => {
            eprintln!("{message}");
            return Ok(2);
        }
        Err(ApiCallError::Io(error)) => return Err(error),
    };

    super::print_response(&super::send_request(&request)?)
}

#[derive(Debug)]
enum ApiCallError {
    Usage(String),
    Io(std::io::Error),
}

/// Builds a typed request so unknown methods and malformed params fail before connecting.
fn parse_api_call(
    args: &[String],
    read_stdin: impl FnOnce() -> std::io::Result<String>,
) -> Result<Request, ApiCallError> {
    let (method, params) = match args {
        [method] => (method, None),
        [method, params] => (method, Some(params.as_str())),
        _ => return Err(ApiCallError::Usage(API_CALL_USAGE.into())),
    };
    if !Method::OPERATIONS
        .iter()
        .any(|operation| operation.name == method)
    {
        return Err(ApiCallError::Usage(format!(
            "unknown API method: {method}; run `herdr api schema --json` to list methods"
        )));
    }

    let params = match params {
        None => serde_json::Value::Object(serde_json::Map::new()),
        Some(source) => {
            let text = if source == "-" {
                read_stdin().map_err(ApiCallError::Io)?
            } else {
                source.to_owned()
            };
            let value: serde_json::Value = serde_json::from_str(&text).map_err(|error| {
                ApiCallError::Usage(format!("invalid params JSON for {method}: {error}"))
            })?;
            if !value.is_object() {
                return Err(ApiCallError::Usage(format!(
                    "params for {method} must be a JSON object"
                )));
            }
            value
        }
    };

    serde_json::from_value(serde_json::json!({
        "id": API_CALL_REQUEST_ID,
        "method": method,
        "params": params,
    }))
    .map_err(|error| ApiCallError::Usage(format!("invalid params for {method}: {error}")))
}

fn write_schema_file(path: &std::path::Path) -> std::io::Result<()> {
    std::fs::write(path, API_SCHEMA_JSON)
}

fn schema_summary_text() -> std::io::Result<String> {
    let value: serde_json::Value = serde_json::from_str(API_SCHEMA_JSON)?;
    let protocol = value
        .get("protocol")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| std::io::Error::other("API schema is missing protocol"))?;
    let schema_version = value
        .get("schema_version")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| std::io::Error::other("API schema is missing schema_version"))?;
    let mut schemas: Vec<&str> = value
        .get("schemas")
        .and_then(serde_json::Value::as_object)
        .ok_or_else(|| std::io::Error::other("API schema is missing schemas"))?
        .keys()
        .map(String::as_str)
        .collect();
    schemas.sort();

    Ok(format!(
        "Herdr API schema\nprotocol: {}\nschema_version: {}\nschemas: {}\n\nUse `herdr api schema --json` to print the full schema.\nUse `herdr api schema --output PATH` to write it to a file.\n",
        protocol,
        schema_version,
        schemas.join(", ")
    ))
}

fn print_api_help() {
    eprintln!("herdr api commands:");
    eprintln!("  herdr api call <METHOD> [PARAMS_JSON | -]");
    eprintln!("  herdr api snapshot");
    eprintln!("  herdr api schema [--json | --output PATH]");
}

fn print_api_call_help() {
    eprintln!("{API_CALL_USAGE}");
    eprintln!();
    eprintln!("Sends one socket API request and prints the JSON response.");
    eprintln!("PARAMS_JSON must be a JSON object; omit it for methods without params.");
    eprintln!("Pass - to read PARAMS_JSON from stdin.");
}

fn print_api_schema_help() {
    eprintln!("usage: herdr api schema [--json | --output PATH]");
}

#[cfg(test)]
mod tests {
    use super::{parse_api_call, ApiCallError, API_CALL_REQUEST_ID};
    use crate::api::schema::{Method, WorkspaceTarget};

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    fn no_stdin() -> std::io::Result<String> {
        panic!("stdin must not be read")
    }

    fn usage_message(result: Result<crate::api::schema::Request, ApiCallError>) -> String {
        match result {
            Err(ApiCallError::Usage(message)) => message,
            other => panic!("expected usage error, got {other:?}"),
        }
    }

    #[test]
    fn api_call_builds_a_typed_request_from_inline_params() {
        let request = parse_api_call(
            &args(&["workspace.focus", r#"{"workspace_id":"w2"}"#]),
            no_stdin,
        )
        .unwrap();

        assert_eq!(request.id, API_CALL_REQUEST_ID);
        assert_eq!(
            request.method,
            Method::WorkspaceFocus(WorkspaceTarget {
                workspace_id: "w2".into()
            })
        );
    }

    #[test]
    fn api_call_defaults_to_empty_params() {
        let request = parse_api_call(&args(&["workspace.list"]), no_stdin).unwrap();
        assert_eq!(request.method.operation().name, "workspace.list");
    }

    #[test]
    fn api_call_reads_params_from_stdin_for_dash() {
        let request = parse_api_call(&args(&["workspace.focus", "-"]), || {
            Ok(r#"{"workspace_id":"w7"}"#.to_owned())
        })
        .unwrap();
        assert_eq!(
            request.method,
            Method::WorkspaceFocus(WorkspaceTarget {
                workspace_id: "w7".into()
            })
        );
    }

    #[test]
    fn api_call_rejects_unknown_methods_before_reading_params() {
        let message = usage_message(parse_api_call(&args(&["workspace.nope", "-"]), no_stdin));
        assert!(message.contains("unknown API method: workspace.nope"));
    }

    #[test]
    fn api_call_rejects_malformed_or_non_object_params() {
        let message = usage_message(parse_api_call(
            &args(&["workspace.focus", "{not json"]),
            no_stdin,
        ));
        assert!(message.contains("invalid params JSON for workspace.focus"));

        let message = usage_message(parse_api_call(&args(&["workspace.focus", "[]"]), no_stdin));
        assert!(message.contains("must be a JSON object"));

        let message = usage_message(parse_api_call(&args(&["workspace.focus", "{}"]), no_stdin));
        assert!(message.contains("invalid params for workspace.focus"));
    }

    #[test]
    fn api_call_requires_a_method_and_at_most_one_params_value() {
        for input in [&[][..], &["workspace.list", "{}", "extra"]] {
            let message = usage_message(parse_api_call(&args(input), no_stdin));
            assert!(message.starts_with("usage: herdr api call"));
        }
    }

    #[test]
    fn schema_summary_text_stays_human_sized() {
        let text = super::schema_summary_text().unwrap();
        assert!(text.contains("Herdr API schema"));
        assert!(text.contains("Use `herdr api schema --json`"));
        assert!(text.len() < 400);
    }
}
