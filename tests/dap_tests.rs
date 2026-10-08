//! End-to-end tests for the Debug Adapter Protocol server.
//!
//! These drive the real `nect dap` binary over a pipe, speaking the protocol as
//! an editor would. A unit test can prove the message shapes are right; only
//! this can prove the framing survives a real process boundary and that the
//! program's own output never lands in the transport.

use serde_json::{Value, json};
use std::io::{Read, Write};
use std::process::{Command, Stdio};

/// Frames one message the way DAP specifies: a `Content-Length` header, a
/// blank line, then the body.
fn frame(body: &Value) -> Vec<u8> {
    let encoded = serde_json::to_vec(body).expect("serializes");
    let mut out = format!("Content-Length: {}\r\n\r\n", encoded.len()).into_bytes();
    out.extend_from_slice(&encoded);
    out
}

/// Splits a stream of framed messages back into values.
fn parse_frames(stream: &[u8]) -> Vec<Value> {
    let mut messages = Vec::new();
    let mut index = 0usize;
    while index < stream.len() {
        let Some(header_end) = find(stream, index, b"\r\n\r\n") else {
            break;
        };
        let header = String::from_utf8_lossy(&stream[index..header_end]).to_string();
        let length = header
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.trim()
                    .eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().ok())?
            })
            .expect("every frame declares its length");
        let body_start = header_end + 4;
        let body_end = body_start + length;
        messages.push(serde_json::from_slice(&stream[body_start..body_end]).expect("a JSON body"));
        index = body_end;
    }
    messages
}

fn find(haystack: &[u8], from: usize, needle: &[u8]) -> Option<usize> {
    haystack[from..]
        .windows(needle.len())
        .position(|window| window == needle)
        .map(|offset| offset + from)
}

/// Runs `nect dap` and returns `(protocol messages, stderr)`.
///
/// The program is delivered in the `launch` request rather than as a file,
/// which is what a client with the text already in hand does — and it keeps the
/// test from writing a temporary file for every case.
fn converse(source: &str, requests: &[Value]) -> (Vec<Value>, String) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_nect"))
        .arg("dap")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to start the nect binary");

    // `launch` comes first and carries the program; the caller's requests follow
    // in the order they wrote them.
    let mut input = Vec::new();
    input.extend_from_slice(&frame(&json!({
        "seq": 0,
        "command": "launch",
        "arguments": { "program": source },
    })));
    for request in requests {
        input.extend_from_slice(&frame(request));
    }
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(&input)
        .expect("writes the request stream");
    // The child only sees EOF once stdin is closed, which is its cue to stop.
    drop(child.stdin.take());

    let mut stdout = Vec::new();
    child
        .stdout
        .take()
        .expect("stdout")
        .read_to_end(&mut stdout)
        .expect("reads the response stream");
    let mut stderr = String::new();
    child
        .stderr
        .take()
        .expect("stderr")
        .read_to_string(&mut stderr)
        .expect("reads stderr");
    child.wait().expect("the adapter exits");

    (parse_frames(&stdout), stderr)
}

fn response_body(messages: &[Value], command: &str) -> Value {
    messages
        .iter()
        .find(|m| m["type"] == "response" && m["command"] == command)
        .unwrap_or_else(|| panic!("no response for `{command}` in {messages:?}"))
        .clone()
}

fn events_named<'a>(messages: &'a [Value], name: &str) -> Vec<&'a Value> {
    messages
        .iter()
        .filter(|m| m["type"] == "event" && m["event"] == name)
        .collect()
}

const PROGRAM: &str = "let a = 1\nlet b = 2\nprint(a + b)\n";

#[test]
fn initialize_reports_the_capabilities_the_engine_has() {
    let (messages, _) = converse(
        PROGRAM,
        &[
            json!({ "seq": 1, "command": "initialize", "arguments": {} }),
            json!({ "seq": 2, "command": "disconnect", "arguments": {} }),
        ],
    );
    let body = response_body(&messages, "initialize");
    assert_eq!(body["success"], true);
    assert_eq!(body["body"]["supportsTerminateRequest"], true);
}

#[test]
fn every_response_is_a_well_formed_protocol_message() {
    let (messages, _) = converse(
        PROGRAM,
        &[
            json!({ "seq": 1, "command": "initialize", "arguments": {} }),
            json!({ "seq": 2, "command": "threads", "arguments": {} }),
            json!({ "seq": 3, "command": "disconnect", "arguments": {} }),
        ],
    );
    for message in &messages {
        assert!(message["type"].is_string(), "every message declares a type");
        assert!(message["seq"].is_i64(), "every message declares a sequence");
        if message["type"] == "response" {
            assert!(message["request_seq"].is_i64());
            assert!(message["success"].is_boolean());
        }
    }
}

#[test]
fn a_breakpoint_stops_execution_with_variables_visible() {
    let (messages, _) = converse(
        PROGRAM,
        &[
            json!({ "seq": 1, "command": "initialize", "arguments": {} }),
            json!({ "seq": 2, "command": "launch", "arguments": {} }),
            json!({
                "seq": 3,
                "command": "setBreakpoints",
                "arguments": {
                    "source": { "path": "main.nct" },
                    "breakpoints": [ { "line": 3 } ],
                },
            }),
            json!({ "seq": 4, "command": "continue", "arguments": {} }),
            json!({ "seq": 5, "command": "stackTrace", "arguments": { "threadId": 1 } }),
            json!({ "seq": 6, "command": "scopes", "arguments": { "frameId": 1 } }),
            json!({ "seq": 7, "command": "variables", "arguments": { "variablesReference": 1 } }),
            json!({ "seq": 8, "command": "disconnect", "arguments": {} }),
        ],
    );

    let stopped = events_named(&messages, "stopped");
    assert_eq!(stopped.len(), 1, "the breakpoint should be hit once");
    assert_eq!(stopped[0]["body"]["reason"], "breakpoint");
    assert_eq!(stopped[0]["body"]["line"], 3);

    let frames = response_body(&messages, "stackTrace");
    assert_eq!(frames["body"]["stackFrames"][0]["line"], 3);

    // Stopped *before* `print(a + b)` runs, so both bindings exist.
    let variables = response_body(&messages, "variables");
    let names: Vec<&str> = variables["body"]["variables"]
        .as_array()
        .expect("an array")
        .iter()
        .map(|v| v["name"].as_str().expect("a name"))
        .collect();
    assert!(names.contains(&"a"), "got {names:?}");
    assert!(names.contains(&"b"), "got {names:?}");
}

#[test]
fn a_variable_bound_after_the_breakpoint_is_not_yet_visible() {
    // Stopping on line 2 means `let b = 2` has not run, so reading `b` has to
    // fail. If it succeeded, the debugger would be showing a state the program
    // is not actually in.
    let (messages, _) = converse(
        PROGRAM,
        &[
            json!({ "seq": 1, "command": "launch", "arguments": {} }),
            json!({
                "seq": 2,
                "command": "setBreakpoints",
                "arguments": {
                    "source": { "path": "main.nct" },
                    "breakpoints": [ { "line": 2 } ],
                },
            }),
            json!({ "seq": 3, "command": "continue", "arguments": {} }),
            json!({ "seq": 4, "command": "evaluate", "arguments": { "expression": "b" } }),
            json!({ "seq": 5, "command": "disconnect", "arguments": {} }),
        ],
    );
    let result = response_body(&messages, "evaluate");
    assert_eq!(result["success"], true);
    assert!(
        result["body"]["result"]
            .as_str()
            .expect("a result")
            .contains("undefined"),
        "got {:?}",
        result["body"]["result"]
    );
}

#[test]
fn the_programs_own_output_never_enters_the_protocol_stream() {
    // The single most damaging thing a debug adapter can do is let the
    // debuggee's stdout desynchronise the message framing, so this is checked
    // against a program that prints.
    let (messages, stderr) = converse(
        PROGRAM,
        &[
            json!({ "seq": 1, "command": "continue", "arguments": {} }),
            json!({ "seq": 2, "command": "disconnect", "arguments": {} }),
        ],
    );
    // Every message still parses, which is the real assertion.
    assert!(messages.iter().all(|m| m["type"].is_string()));
    // And the program's output went somewhere the client can still show.
    assert!(
        stderr.contains('3'),
        "the program's output should reach stderr, got {stderr:?}"
    );
}

#[test]
fn running_to_completion_reports_termination() {
    let (messages, _) = converse(
        PROGRAM,
        &[
            json!({ "seq": 1, "command": "continue", "arguments": {} }),
            json!({ "seq": 2, "command": "disconnect", "arguments": {} }),
        ],
    );
    assert_eq!(events_named(&messages, "terminated").len(), 1);
    assert!(events_named(&messages, "stopped").is_empty());
}

#[test]
fn a_runtime_failure_is_delivered_to_the_client_console() {
    let (messages, _) = converse(
        "print(missing_name)\n",
        &[
            json!({ "seq": 1, "command": "continue", "arguments": {} }),
            json!({ "seq": 2, "command": "disconnect", "arguments": {} }),
        ],
    );
    let output = events_named(&messages, "output");
    assert_eq!(output.len(), 1, "the failure should be reported");
    assert_eq!(output[0]["body"]["category"], "stderr");
    assert!(
        output[0]["body"]["output"]
            .as_str()
            .expect("text")
            .contains("missing_name")
    );
    assert_eq!(events_named(&messages, "terminated").len(), 1);
}

#[test]
fn stepping_advances_one_statement_at_a_time() {
    let (messages, _) = converse(
        PROGRAM,
        &[
            json!({ "seq": 1, "command": "next", "arguments": {} }),
            json!({ "seq": 2, "command": "stackTrace", "arguments": { "threadId": 1 } }),
            json!({ "seq": 3, "command": "next", "arguments": {} }),
            json!({ "seq": 4, "command": "stackTrace", "arguments": { "threadId": 1 } }),
            json!({ "seq": 5, "command": "disconnect", "arguments": {} }),
        ],
    );
    let stacks: Vec<&Value> = messages
        .iter()
        .filter(|m| m["type"] == "response" && m["command"] == "stackTrace")
        .collect();
    assert_eq!(stacks.len(), 2);
    let first = stacks[0]["body"]["stackFrames"][0]["line"].clone();
    let second = stacks[1]["body"]["stackFrames"][0]["line"].clone();
    assert_ne!(first, second, "each `next` should move one statement on");
}

#[test]
fn clearing_the_breakpoints_stops_them_firing() {
    let (messages, _) = converse(
        PROGRAM,
        &[
            json!({
                "seq": 1,
                "command": "setBreakpoints",
                "arguments": {
                    "source": { "path": "main.nct" },
                    "breakpoints": [ { "line": 2 } ],
                },
            }),
            // The client replaces the whole set with nothing.
            json!({
                "seq": 2,
                "command": "setBreakpoints",
                "arguments": { "source": { "path": "main.nct" }, "breakpoints": [] },
            }),
            json!({ "seq": 3, "command": "continue", "arguments": {} }),
            json!({ "seq": 4, "command": "disconnect", "arguments": {} }),
        ],
    );
    assert!(
        events_named(&messages, "stopped").is_empty(),
        "a cleared breakpoint must not fire"
    );
    assert_eq!(events_named(&messages, "terminated").len(), 1);
}

#[test]
fn a_breakpoint_on_a_line_with_no_statement_never_fires() {
    // Verified or not, a line with nothing on it cannot be hit; the program
    // should simply run to the end rather than stopping at the wrong place.
    let (messages, _) = converse(
        "let a = 1\n\n\nlet b = 2\n",
        &[
            json!({
                "seq": 1,
                "command": "setBreakpoints",
                "arguments": {
                    "source": { "path": "main.nct" },
                    "breakpoints": [ { "line": 2 } ],
                },
            }),
            json!({ "seq": 2, "command": "continue", "arguments": {} }),
            json!({ "seq": 3, "command": "disconnect", "arguments": {} }),
        ],
    );
    assert!(events_named(&messages, "stopped").is_empty());
}

#[test]
fn the_source_can_be_read_back() {
    let (messages, _) = converse(
        PROGRAM,
        &[
            json!({ "seq": 1, "command": "source", "arguments": { "sourceReference": 0 } }),
            json!({ "seq": 2, "command": "disconnect", "arguments": {} }),
        ],
    );
    let body = response_body(&messages, "source");
    assert_eq!(body["body"]["content"], PROGRAM);
}

#[test]
fn an_unknown_request_is_refused_without_ending_the_session() {
    let (messages, _) = converse(
        PROGRAM,
        &[
            json!({ "seq": 1, "command": "notARealRequest", "arguments": {} }),
            json!({ "seq": 2, "command": "threads", "arguments": {} }),
            json!({ "seq": 3, "command": "disconnect", "arguments": {} }),
        ],
    );
    let refused = response_body(&messages, "notARealRequest");
    assert_eq!(refused["success"], false);
    // The session carries on afterwards.
    assert_eq!(response_body(&messages, "threads")["success"], true);
}
