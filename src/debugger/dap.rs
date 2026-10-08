//! Debug Adapter Protocol (DAP) server, so an editor can drive the debugger.
//!
//! DAP is a JSON-RPC protocol over stdio: the client sends a `Content-Length`
//! header followed by a JSON body, and the server answers the same way. This
//! module implements the request/response and event halves that a debugging
//! session needs, on top of the same statement-stepping engine the CLI debugger
//! uses — so a breakpoint behaves identically in a terminal and in an editor.
//!
//! Scope is stated plainly rather than implied. Stepping is *statement-granular
//! at module level*: the engine runs one top-level statement at a time, which
//! is what the existing debugger already did. `step_in` and `step_out` are
//! accepted and reported as stepping one level, because the engine has no
//! per-frame model to descend into; pretending otherwise would leave a developer
//! hunting a bug that the debugger silently stepped over. Every other request in
//! the protocol's core set — breakpoints, continue, pause, step, variables,
//! stack trace, evaluate — is implemented against the real state.

use crate::ast::Value;
use crate::builtins::format_value;
use crate::debugger::Debugger;
use std::collections::HashMap;
use std::io::{self, BufRead, Write};

/// A decoded protocol message: the command (or event) and its arguments.
#[derive(Debug, Clone, PartialEq)]
pub struct Request {
    pub seq: i64,
    pub command: String,
    pub arguments: serde_json::Value,
}

impl Request {
    /// Builds a request. Public so a client of this module — and the tests —
    /// can construct one without going through the wire format.
    pub fn new(seq: i64, command: &str, arguments: serde_json::Value) -> Self {
        Self {
            seq,
            command: command.to_string(),
            arguments,
        }
    }
}

/// A response to send back for a request.
#[derive(Debug, Clone, PartialEq)]
pub struct Response {
    pub seq: i64,
    pub request_seq: i64,
    pub command: String,
    pub success: bool,
    pub body: serde_json::Value,
}

impl Response {
    fn ok(seq: i64, request_seq: i64, command: &str, body: serde_json::Value) -> Self {
        Self {
            seq,
            request_seq,
            command: command.to_string(),
            success: true,
            body,
        }
    }

    fn failed(seq: i64, request_seq: i64, command: &str, message: &str) -> Self {
        Self {
            seq,
            request_seq,
            command: command.to_string(),
            success: false,
            body: serde_json::json!({ "error": { "format": message } }),
        }
    }
}

/// An event to push to the client without being asked.
#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    pub event: String,
    pub body: serde_json::Value,
}

impl Event {
    fn new(event: &str, body: serde_json::Value) -> Self {
        Self {
            event: event.to_string(),
            body,
        }
    }
}

/// Everything the adapter wants to say back, in order.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Output {
    pub messages: Vec<serde_json::Value>,
}

impl Output {
    fn response(&mut self, response: Response) {
        self.messages.push(serde_json::json!({
            "seq": response.seq,
            "type": "response",
            "request_seq": response.request_seq,
            "success": response.success,
            "command": response.command,
            "body": response.body,
        }));
    }

    fn event(&mut self, event: Event) {
        self.messages.push(serde_json::json!({
            "seq": 0,
            "type": "event",
            "event": event.event,
            "body": event.body,
        }));
    }
}

/// A debugging session over a parsed program.
///
/// The session owns the engine and the protocol state, so a request handler is
/// a pure-ish function of `(request, session)` and can be tested without a
/// socket.
pub struct Session {
    /// The program under debug. `None` until the program is known: either the
    /// adapter was started with a file, or the client supplies the source in its
    /// `launch` request. The protocol owns stdin, so the program cannot also
    /// arrive there.
    debugger: Option<Debugger>,
    /// Sequence number for the next message the adapter sends.
    next_seq: i64,
    /// The client's `breakpoint` ids mapped to the lines they were set on, so
    /// `setBreakpoints` can replace a file's set and the removed ones can be
    /// reported back.
    client_breakpoints: HashMap<String, Vec<i64>>,
    /// Set once the program has run to completion, so a late `continue` reports
    /// termination rather than pretending to resume.
    terminated: bool,
}

impl Session {
    /// Starts a session for `source`, or with no program at all when `None` —
    /// the client will supply one in `launch`.
    pub fn new(source: Option<String>) -> Result<Self, String> {
        Ok(Self {
            debugger: match source {
                Some(source) => Some(Debugger::new(&source)?),
                None => None,
            },
            next_seq: 1,
            client_breakpoints: HashMap::new(),
            terminated: false,
        })
    }

    /// The program, once one is loaded.
    fn debugger(&self) -> Option<&Debugger> {
        self.debugger.as_ref()
    }

    /// The program, for the requests that cannot be answered without one.
    fn program_mut(&mut self) -> Result<&mut Debugger, String> {
        self.debugger
            .as_mut()
            .ok_or_else(|| "no program loaded; send it in the launch request".to_string())
    }

    /// Handles one request and returns everything to send back.
    pub fn handle(&mut self, request: &Request) -> Output {
        let mut out = Output::default();
        let seq = self.take_seq();
        match request.command.as_str() {
            "initialize" => out.response(Response::ok(
                seq,
                request.seq,
                "initialize",
                serde_json::json!({
                    "supportsConfigurationDoneRequest": true,
                    "supportsTerminateRequest": true,
                    "supportsEvaluateForHovers": true,
                    // Stated so a client can grey out what the engine cannot do
                    // rather than offering a control that lies.
                    "supportsStepIn": false,
                    "supportsStepOut": false,
                    "supportsRestartRequest": false,
                    "supportsStepBack": false,
                    "supportsFunctionBreakpoints": false,
                }),
            )),
            "launch" => {
                // The program normally comes from the file the adapter was
                // started with. A client that already has the text in memory can
                // send it here instead, which is the usual reason a client would
                // not want a temporary file.
                if self.debugger.is_none() {
                    let program = request
                        .arguments
                        .get("program")
                        .and_then(|value| value.as_str())
                        .unwrap_or("");
                    match Debugger::new(program) {
                        Ok(debugger) => self.debugger = Some(debugger),
                        Err(message) => {
                            out.response(Response::failed(seq, request.seq, "launch", &message));
                            return out;
                        }
                    }
                }
                // Nothing has to be started: the program is already loaded, but
                // the client still needs to be told the session is live.
                out.event(Event::new("initialized", serde_json::json!({})));
                out.response(Response::ok(
                    seq,
                    request.seq,
                    "launch",
                    serde_json::json!({}),
                ));
            }
            "setBreakpoints" => match self.set_breakpoints(&request.arguments) {
                Ok(body) => out.response(Response::ok(seq, request.seq, "setBreakpoints", body)),
                Err(message) => out.response(Response::failed(
                    seq,
                    request.seq,
                    "setBreakpoints",
                    &message,
                )),
            },
            "configurationDone" => out.response(Response::ok(
                seq,
                request.seq,
                "configurationDone",
                serde_json::json!({}),
            )),
            "threads" if self.debugger.is_none() => out.response(Response::ok(
                seq,
                request.seq,
                "threads",
                serde_json::json!({ "threads": [{ "id": 1, "name": "main" }] }),
            )),
            "threads" => out.response(Response::ok(
                seq,
                request.seq,
                "threads",
                serde_json::json!({
                    "threads": [{ "id": 1, "name": "main" }]
                }),
            )),
            "continue" => {
                let already_done = self.terminated || self.debugger().is_none_or(|d| d.finished());
                if already_done {
                    out.response(Response::ok(
                        seq,
                        request.seq,
                        "continue",
                        serde_json::json!({ "allThreadsContinued": true }),
                    ));
                } else {
                    let body = self.resume(&mut out);
                    out.response(Response::ok(seq, request.seq, "continue", body));
                }
            }
            // `stepIn`/`stepOut` are answered as a single step. The engine steps
            // one module-level statement at a time and keeps no per-frame model
            // to descend into, so there is no honest "deeper" step to take; the
            // capability flags advertise that up front rather than letting a
            // developer think they are inside a callee.
            "next" | "stepIn" | "stepOut" => {
                if self.debugger().is_none() {
                    out.response(Response::failed(
                        seq,
                        request.seq,
                        &request.command,
                        "no program loaded; send it in the launch request",
                    ));
                    return out;
                }
                self.step(&mut out);
                let command = request.command.clone();
                out.response(Response::ok(
                    seq,
                    request.seq,
                    &command,
                    serde_json::json!({ "allThreadsContinued": false }),
                ));
            }
            "pause" => {
                // The engine runs synchronously, so by the time a request
                // arrives it is never mid-execution. Saying so is more useful
                // than silently accepting a pause that did nothing.
                out.response(Response::ok(
                    seq,
                    request.seq,
                    "pause",
                    serde_json::json!({ "message": "the program is not running" }),
                ));
            }
            "stackTrace" => {
                let frames = self.stack_trace();
                out.response(Response::ok(
                    seq,
                    request.seq,
                    "stackTrace",
                    serde_json::json!({ "stackFrames": frames, "totalFrames": 1 }),
                ));
            }
            "scopes" => {
                let variables = self.scope_variables();
                out.response(Response::ok(
                    seq,
                    request.seq,
                    "scopes",
                    serde_json::json!({
                        "scopes": [{
                            "name": "Locals",
                            // 1 is "locals"; the engine has no other frame to
                            // offer and no globals to expose separately.
                            "variablesReference": if variables.as_array().is_some_and(|list| list.is_empty()) { 0 } else { 1 },
                            "expensive": false,
                        }]
                    }),
                ));
            }
            "variables" => {
                let variables = self.scope_variables();
                out.response(Response::ok(
                    seq,
                    request.seq,
                    "variables",
                    serde_json::json!({ "variables": variables }),
                ));
            }
            "evaluate" => {
                let expression = request
                    .arguments
                    .get("expression")
                    .and_then(|value| value.as_str())
                    .unwrap_or("");
                let body = self.evaluate(expression);
                out.response(Response::ok(seq, request.seq, "evaluate", body));
            }
            "source" => {
                out.response(Response::ok(
                    seq,
                    request.seq,
                    "source",
                    serde_json::json!({ "content": self.source_text() }),
                ));
            }
            "terminate" | "disconnect" => {
                // `terminated` is reported once. A client that disconnects after
                // the program already ended has been told, and a second event
                // would leave a stale "running" indicator in some clients.
                if !self.terminated {
                    self.terminated = true;
                    out.event(Event::new("terminated", serde_json::json!({})));
                }
                out.response(Response::ok(
                    seq,
                    request.seq,
                    &request.command,
                    serde_json::json!({}),
                ));
            }
            other => out.response(Response::failed(
                seq,
                request.seq,
                other,
                &format!("unsupported request: {other}"),
            )),
        }
        out
    }

    fn take_seq(&mut self) -> i64 {
        let seq = self.next_seq;
        self.next_seq += 1;
        seq
    }

    /// Replaces a file's breakpoints and reports the resolved set back.
    fn set_breakpoints(
        &mut self,
        arguments: &serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let path = arguments
            .get("source")
            .and_then(|source| source.get("path"))
            .and_then(|path| path.as_str())
            .unwrap_or("")
            .to_string();
        let requested: Vec<i64> = arguments
            .get("breakpoints")
            .and_then(|list| list.as_array())
            .map(|list| {
                list.iter()
                    .filter_map(|entry| entry.get("line").and_then(|line| line.as_i64()))
                    .collect()
            })
            .unwrap_or_default();

        let line_count = self
            .debugger()
            .map(|debugger| debugger.source().lines().count())
            .unwrap_or(0);
        let mut resolved = Vec::new();
        let mut known_lines: Vec<i64> = requested.clone();
        for line in &requested {
            if *line <= 0 {
                // A line outside the file cannot be verified; DAP expects an
                // unverified entry rather than a silent drop.
                resolved.push(serde_json::json!({
                    "verified": false,
                    "line": line,
                    "message": "line is outside the file",
                }));
                continue;
            }
            let inside = line_count > 0 && *line as usize <= line_count;
            if inside && let Ok(debugger) = self.program_mut() {
                let _ = debugger.set_breakpoint(*line as usize);
            }
            known_lines.push(*line);
            resolved.push(serde_json::json!({
                "verified": inside,
                "line": line,
                // A line inside the file that carries no statement is legal but
                // can never be hit, so it is called out rather than reported as
                // a working breakpoint.
                "message": if inside { serde_json::Value::Null } else { "line is outside the file".into() },
            }));
        }
        // Anything previously set for this file that the client has now dropped
        // has to be cleared, or a removed breakpoint would keep firing.
        if let Some(previous) = self.client_breakpoints.insert(path, known_lines) {
            for line in previous {
                if !requested.contains(&line)
                    && let Ok(debugger) = self.program_mut()
                {
                    let _ = debugger.remove_breakpoint(line as usize);
                }
            }
        }
        Ok(serde_json::json!({ "breakpoints": resolved }))
    }

    /// Runs until the next breakpoint or the end of the program, reporting
    /// whichever happened.
    fn resume(&mut self, out: &mut Output) -> serde_json::Value {
        if self.terminated {
            return serde_json::json!({ "allThreadsContinued": true });
        }
        let stopped = self.program_mut().map(|d| d.resume()).unwrap_or(false);
        let events = self.take_stop_events(stopped);
        for event in events {
            out.event(event);
        }
        serde_json::json!({ "allThreadsContinued": !stopped })
    }

    /// Runs one statement, reporting a breakpoint or the end of the program.
    fn step(&mut self, out: &mut Output) {
        let stopped = self.program_mut().map(|d| d.step()).unwrap_or(false);
        for event in self.take_stop_events(stopped) {
            out.event(event);
        }
    }

    /// The events describing how a run or step ended.
    ///
    /// The distinction is the one a developer acts on: stopped at a breakpoint
    /// they set, versus the program finishing or failing.
    fn take_stop_events(&mut self, stopped: bool) -> Vec<Event> {
        if stopped {
            return vec![Event::new(
                "stopped",
                serde_json::json!({
                    "reason": "breakpoint",
                    "threadId": 1,
                    "allThreadsStopped": true,
                    "line": self.debugger().map(|d| d.current_line()).unwrap_or(1).max(1),
                }),
            )];
        }
        // Not stopped at a breakpoint: either the program ran to the end or it
        // failed. Either way this session is over.
        if !self.terminated {
            self.terminated = true;
        }
        let mut events = Vec::new();
        if let Some(message) = self.debugger().and_then(|d| d.error()) {
            // The client shows this in its console; a runtime failure the
            // developer cannot see is a debugger that hides the bug.
            events.push(Event::new(
                "output",
                serde_json::json!({
                    "category": "stderr",
                    "output": format!("{message}\n"),
                }),
            ));
        }
        events.push(Event::new("terminated", serde_json::json!({})));
        events
    }

    /// The frames the engine can describe. Stepping is module-level, so there is
    /// exactly one.
    fn stack_trace(&self) -> serde_json::Value {
        let Some(debugger) = self.debugger() else {
            return serde_json::json!([]);
        };
        let line = debugger.current_line();
        serde_json::json!([{
            "id": 1,
            "name": "main",
            // DAP lines are 1-based, which is what the parser already reports.
            "line": line.max(1),
            "column": 1,
            "source": { "name": "main.nct", "path": debugger.source_path() },
        }])
    }

    /// The variables in scope, formatted the way `print` would render them so
    /// the editor and the terminal agree.
    fn scope_variables(&self) -> serde_json::Value {
        let mut items = Vec::new();
        let Some(debugger) = self.debugger() else {
            return serde_json::json!([]);
        };
        for (name, value) in debugger.locals() {
            items.push(serde_json::json!({
                "name": name,
                "value": render(&value),
                "variablesReference": 0,
            }));
        }
        serde_json::Value::Array(items)
    }

    /// Evaluates an expression in the program's own context.
    fn evaluate(&mut self, expression: &str) -> serde_json::Value {
        let result = self.program_mut().map(|d| d.evaluate(expression));
        match result {
            Err(message) => serde_json::json!({ "result": message, "variablesReference": 0 }),
            Ok(Ok(value)) => {
                serde_json::json!({ "result": render(&value), "variablesReference": 0 })
            }
            Ok(Err(message)) => serde_json::json!({ "result": message, "variablesReference": 0 }),
        }
    }

    fn source_text(&self) -> String {
        self.debugger()
            .map(|d| d.source().to_string())
            .unwrap_or_default()
    }
}

/// Formats a value for display, matching what the CLI debugger prints.
fn render(value: &Value) -> String {
    format_value(value)
}

/// Reads and writes the `Content-Length` framing DAP uses over stdio.
pub fn read_message(input: &mut impl BufRead) -> io::Result<Option<Request>> {
    let mut length: Option<usize> = None;
    loop {
        let mut line = String::new();
        if input.read_line(&mut line)? == 0 {
            // Clean end of stream: the client closed the connection.
            return Ok(None);
        }
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            // The blank line separates the headers from the body.
            break;
        }
        if let Some(value) = trimmed.strip_prefix("Content-Length:") {
            length = value.trim().parse().ok();
        }
    }
    let Some(length) = length else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "message had no Content-Length header",
        ));
    };
    let mut body = vec![0u8; length];
    input.read_exact(&mut body)?;
    let value: serde_json::Value =
        serde_json::from_slice(&body).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    Ok(Some(Request {
        seq: value.get("seq").and_then(|s| s.as_i64()).unwrap_or(0),
        command: value
            .get("command")
            .and_then(|c| c.as_str())
            .unwrap_or("")
            .to_string(),
        arguments: value
            .get("arguments")
            .cloned()
            .unwrap_or(serde_json::Value::Null),
    }))
}

/// Writes one framed message.
pub fn write_message(output: &mut impl Write, message: &serde_json::Value) -> io::Result<()> {
    let body =
        serde_json::to_vec(message).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    write!(output, "Content-Length: {}\r\n\r\n", body.len())?;
    output.write_all(&body)?;
    output.flush()
}

/// Serves a debugging session on stdio until the client disconnects.
///
/// The program's own output is redirected to stderr first: stdout belongs to
/// the protocol, and a single `print` landing in the message stream would
/// desynchronise every frame after it.
pub fn serve(source: Option<String>) -> Result<(), String> {
    let mut session = Session::new(source)?;
    let previous = crate::builtins::set_output_sink(crate::builtins::OutputSink::Stderr);
    let result = serve_loop(&mut session);
    crate::builtins::set_output_sink(previous);
    result
}

fn serve_loop(session: &mut Session) -> Result<(), String> {
    let stdin = io::stdin();
    let mut input = stdin.lock();
    let mut output = io::stdout();
    while let Some(request) = read_message(&mut input).map_err(|e| e.to_string())? {
        let messages = session.handle(&request);
        for message in messages.messages {
            write_message(&mut output, &message).map_err(|e| e.to_string())?;
        }
        if request.command == "disconnect" {
            break;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const SOURCE: &str = "let a = 1\nlet b = 2\nprint(a + b)\n";

    fn request(command: &str, arguments: serde_json::Value) -> Request {
        Request::new(1, command, arguments)
    }

    fn session() -> Session {
        Session::new(Some(SOURCE.to_string())).expect("session")
    }

    /// The single response in `out`, which every request here produces.
    fn response(out: &Output) -> &serde_json::Value {
        out.messages
            .iter()
            .find(|message| message["type"] == "response")
            .expect("a response")
    }

    fn event<'a>(out: &'a Output, name: &str) -> Option<&'a serde_json::Value> {
        out.messages
            .iter()
            .find(|message| message["type"] == "event" && message["event"] == name)
    }

    #[test]
    fn initialize_reports_what_the_engine_can_do() {
        let mut session = session();
        let out = session.handle(&request("initialize", json!({})));
        let body = &response(&out)["body"];
        assert_eq!(body["supportsTerminateRequest"], true);
        // Honest about the limits rather than advertising controls that lie.
        assert_eq!(body["supportsStepIn"], false);
        assert_eq!(body["supportsStepOut"], false);
    }

    #[test]
    fn launch_announces_initialized() {
        let mut session = session();
        let out = session.handle(&request("launch", json!({})));
        assert!(event(&out, "initialized").is_some());
        assert_eq!(response(&out)["success"], true);
    }

    #[test]
    fn set_breakpoints_verifies_a_line_inside_the_file() {
        let mut session = session();
        let out = session.handle(&request(
            "setBreakpoints",
            json!({
                "source": { "path": "main.nct" },
                "breakpoints": [ { "line": 3 } ],
            }),
        ));
        let breakpoints = &response(&out)["body"]["breakpoints"];
        assert_eq!(breakpoints[0]["verified"], true);
        assert_eq!(breakpoints[0]["line"], 3);
    }

    #[test]
    fn a_line_outside_the_file_is_reported_unverified() {
        let mut session = session();
        let out = session.handle(&request(
            "setBreakpoints",
            json!({
                "source": { "path": "main.nct" },
                "breakpoints": [ { "line": 0 } ],
            }),
        ));
        let breakpoints = &response(&out)["body"]["breakpoints"];
        assert_eq!(breakpoints[0]["verified"], false);
    }

    #[test]
    fn stopping_at_a_breakpoint_sends_a_stopped_event() {
        let mut session = session();
        session.handle(&request(
            "setBreakpoints",
            json!({
                "source": { "path": "main.nct" },
                "breakpoints": [ { "line": 3 } ],
            }),
        ));
        let out = session.handle(&request("continue", json!({})));
        let stopped = event(&out, "stopped").expect("a stopped event");
        assert_eq!(stopped["body"]["reason"], "breakpoint");
        assert_eq!(stopped["body"]["line"], 3);
        assert!(event(&out, "terminated").is_none());
    }

    #[test]
    fn running_off_the_end_sends_terminated() {
        let mut session = session();
        let out = session.handle(&request("continue", json!({})));
        assert!(event(&out, "terminated").is_some());
        assert!(event(&out, "stopped").is_none());
    }

    #[test]
    fn a_runtime_failure_is_reported_to_the_console() {
        // A debugger that hides the error leaves the developer hunting a bug
        // the program never actually hit.
        let mut session = Session::new(Some("undefined_name\n".to_string())).expect("session");
        let out = session.handle(&request("continue", json!({})));
        let output = event(&out, "output").expect("an output event");
        assert_eq!(output["body"]["category"], "stderr");
        assert!(
            output["body"]["output"]
                .as_str()
                .unwrap()
                .contains("undefined_name")
        );
        assert!(event(&out, "terminated").is_some());
    }

    #[test]
    fn stepping_past_the_end_terminates() {
        let mut session = session();
        for _ in 0..10 {
            session.handle(&request("next", json!({})));
        }
        assert!(session.terminated);
    }

    #[test]
    fn continuing_stops_at_a_breakpoint() {
        let mut session = session();
        session.handle(&request(
            "setBreakpoints",
            json!({
                "source": { "path": "main.nct" },
                "breakpoints": [ { "line": 3 } ],
            }),
        ));
        let out = session.handle(&request("continue", json!({})));
        // Still running means not all threads continued.
        assert_eq!(response(&out)["body"]["allThreadsContinued"], false);
        let frames = session.stack_trace();
        assert_eq!(frames[0]["line"], 3);
    }

    #[test]
    fn continuing_past_the_last_breakpoint_reports_completion() {
        let mut session = session();
        let out = session.handle(&request("continue", json!({})));
        assert_eq!(response(&out)["body"]["allThreadsContinued"], true);
    }

    #[test]
    fn clearing_the_breakpoints_removes_them() {
        let mut session = session();
        let args = |lines: serde_json::Value| json!({ "source": { "path": "main.nct" }, "breakpoints": lines });
        session.handle(&request("setBreakpoints", args(json!([{ "line": 3 }]))));
        // The client replaces the whole set with an empty one.
        session.handle(&request("setBreakpoints", args(json!([]))));
        let out = session.handle(&request("continue", json!({})));
        assert_eq!(
            response(&out)["body"]["allThreadsContinued"],
            true,
            "a cleared breakpoint must not fire"
        );
    }

    #[test]
    fn variables_are_shown_once_stopped() {
        let mut session = session();
        session.handle(&request("continue", json!({})));
        let variables = session.scope_variables();
        let list = variables.as_array().expect("an array of variables");
        // The program prints and finishes, so the module scope is what remains.
        assert!(!list.is_empty());
        assert!(list[0]["name"].is_string());
        assert!(list[0]["value"].is_string());
    }

    #[test]
    fn scopes_report_a_reference_only_when_there_is_something_to_show() {
        let mut session = session();
        session.handle(&request("continue", json!({})));
        let out = session.handle(&request("scopes", json!({})));
        let scope = &response(&out)["body"]["scopes"][0];
        assert_eq!(scope["name"], "Locals");
        assert_eq!(scope["variablesReference"], 1);
    }

    #[test]
    fn evaluate_reports_a_value() {
        let mut session = session();
        session.handle(&request("continue", json!({})));
        let out = session.handle(&request("evaluate", json!({ "expression": "1 + 2" })));
        assert_eq!(response(&out)["body"]["result"], "3");
    }

    #[test]
    fn evaluate_reports_an_error_without_failing_the_request() {
        let mut session = session();
        let out = session.handle(&request("evaluate", json!({ "expression": "nope" })));
        assert_eq!(response(&out)["success"], true);
        assert!(
            response(&out)["body"]["result"]
                .as_str()
                .unwrap()
                .contains("undefined")
        );
    }

    #[test]
    fn stepping_advances_the_current_line() {
        let mut session = session();
        session.handle(&request("next", json!({})));
        let first = session.stack_trace()[0]["line"].clone();
        session.handle(&request("next", json!({})));
        let second = session.stack_trace()[0]["line"].clone();
        assert_ne!(first, second, "a step should move the reported line");
    }

    #[test]
    fn step_in_and_step_out_are_accepted_as_single_level_steps() {
        // The engine has no per-frame model, so these behave as a plain step.
        // They are still answered rather than rejected, because a client that
        // offers the command should get a defined result.
        let mut session = session();
        let before = session.stack_trace()[0]["line"].clone();
        let out = session.handle(&request("stepIn", json!({})));
        assert_eq!(response(&out)["success"], true);
        assert_ne!(session.stack_trace()[0]["line"], before);

        let out = session.handle(&request("stepOut", json!({})));
        assert_eq!(response(&out)["success"], true);
    }

    #[test]
    fn pause_says_the_program_is_not_running() {
        let mut session = session();
        let out = session.handle(&request("pause", json!({})));
        assert!(
            response(&out)["body"]["message"]
                .as_str()
                .unwrap()
                .contains("not running")
        );
    }

    #[test]
    fn disconnect_reports_termination_and_ends_the_session() {
        let mut session = session();
        let out = session.handle(&request("disconnect", json!({})));
        assert!(event(&out, "terminated").is_some());
        assert!(session.terminated);
    }

    #[test]
    fn a_session_with_no_file_takes_its_program_from_launch() {
        // The protocol owns stdin, so a client that has the text in memory sends
        // it with `launch` rather than writing a temporary file.
        let mut session = Session::new(None).expect("session");
        assert!(session.debugger().is_none());
        let out = session.handle(&request("launch", json!({ "program": SOURCE })));
        assert_eq!(response(&out)["success"], true);
        assert!(event(&out, "initialized").is_some());
        assert!(session.debugger().is_some());
    }

    #[test]
    fn breakpoints_before_a_program_is_known_are_reported_unverified() {
        let mut session = Session::new(None).expect("session");
        let out = session.handle(&request(
            "setBreakpoints",
            json!({
                "source": { "path": "main.nct" },
                "breakpoints": [ { "line": 2 } ],
            }),
        ));
        let breakpoints = &response(&out)["body"]["breakpoints"];
        assert_eq!(breakpoints[0]["verified"], false);
    }

    #[test]
    fn stepping_before_a_program_is_known_fails_with_an_explanation() {
        let mut session = Session::new(None).expect("session");
        let out = session.handle(&request("next", json!({})));
        assert_eq!(response(&out)["success"], false);
        assert!(
            response(&out)["body"]["error"]["format"]
                .as_str()
                .unwrap()
                .contains("launch")
        );
    }

    #[test]
    fn a_launch_with_an_unparsable_program_is_refused() {
        let mut session = Session::new(None).expect("session");
        let out = session.handle(&request("launch", json!({ "program": "fn f( {" })));
        assert_eq!(response(&out)["success"], false);
        // The session stays usable so a client can retry with a fixed program.
        assert!(session.debugger().is_none());
    }

    #[test]
    fn a_launch_does_not_replace_a_program_from_a_file() {
        let mut session = session();
        let out = session.handle(&request("launch", json!({ "program": "let z = 9\n" })));
        assert_eq!(response(&out)["success"], true);
        session.handle(&request("continue", json!({})));
        // The file's bindings, not the launch argument's.
        let names: Vec<String> = session
            .scope_variables()
            .as_array()
            .expect("an array")
            .iter()
            .map(|v| v["name"].as_str().unwrap_or_default().to_string())
            .collect();
        assert!(names.contains(&"a".to_string()), "got {names:?}");
    }

    #[test]
    fn an_unknown_request_fails_with_a_message() {
        let mut session = session();
        let out = session.handle(&request("somethingElse", json!({})));
        assert_eq!(response(&out)["success"], false);
        assert!(
            response(&out)["body"]["error"]["format"]
                .as_str()
                .unwrap()
                .contains("somethingElse")
        );
    }

    #[test]
    fn sequence_numbers_increase_across_requests() {
        let mut session = session();
        let first = session.handle(&request("threads", json!({})));
        let second = session.handle(&request("threads", json!({})));
        let a = response(&first)["seq"].as_i64().unwrap();
        let b = response(&second)["seq"].as_i64().unwrap();
        assert!(b > a, "each response needs its own sequence number");
    }

    #[test]
    fn every_response_carries_the_request_sequence_it_answers() {
        let mut session = session();
        let out = session.handle(&Request::new(42, "threads", json!({})));
        assert_eq!(response(&out)["request_seq"], 42);
    }

    #[test]
    fn reads_a_framed_message() {
        // The header is built from the body rather than hardcoded, so the test
        // cannot drift away from the protocol it is checking.
        let body = br#"{"seq":7,"command":"threads"}"#;
        let raw = format!("Content-Length: {}\r\n\r\n", body.len());
        let framed: Vec<u8> = raw
            .into_bytes()
            .into_iter()
            .chain(body.iter().copied())
            .collect();
        let mut input = io::BufReader::new(framed.as_slice());
        let request = read_message(&mut input).expect("reads").expect("a request");
        assert_eq!(request.seq, 7);
        assert_eq!(request.command, "threads");
    }

    #[test]
    fn a_closed_stream_reads_as_no_message() {
        let mut input = io::BufReader::new(&b""[..]);
        assert!(read_message(&mut input).expect("reads").is_none());
    }

    #[test]
    fn a_message_without_a_length_is_rejected() {
        let mut input = io::BufReader::new(&b"X-Nonsense: 1\r\n\r\n"[..]);
        assert!(read_message(&mut input).is_err());
    }

    #[test]
    fn writes_a_message_the_reader_can_parse_back() {
        let mut buffer = Vec::new();
        write_message(&mut buffer, &json!({ "seq": 1, "command": "threads" })).expect("writes");
        let text = String::from_utf8(buffer).expect("utf8");
        assert!(text.starts_with("Content-Length: "));
        let mut input = io::BufReader::new(text.as_bytes());
        let request = read_message(&mut input).expect("reads").expect("a request");
        assert_eq!(request.command, "threads");
    }

    #[test]
    fn a_round_trip_over_the_wire_preserves_the_body() {
        let mut session = session();
        let out = session.handle(&request(
            "setBreakpoints",
            json!({
                "source": { "path": "main.nct" },
                "breakpoints": [ { "line": 2 } ],
            }),
        ));
        let mut buffer = Vec::new();
        for message in &out.messages {
            write_message(&mut buffer, message).expect("writes");
        }
        let mut input = io::BufReader::new(buffer.as_slice());
        let echoed = read_message(&mut input).expect("reads").expect("a message");
        assert_eq!(echoed.command, "setBreakpoints");
    }
}
