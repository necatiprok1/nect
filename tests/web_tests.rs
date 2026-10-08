//! Integration tests for the web primitives.
//!
//! These run the real binary, because the point of most of them is that the
//! built-ins are registered on the engine's shared value path and therefore
//! behave identically on the interpreter and the VM.

use std::io::Write;
use std::process::{Command, Stdio};

fn run(source: &str, args: &[&str]) -> (String, String) {
    let mut command = Command::new(env!("CARGO_BIN_EXE_nect"));
    command.arg("run");
    for arg in args {
        command.arg(arg);
    }
    command.arg("-");
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to start the nect binary");
    child
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(source.as_bytes())
        .expect("writes the program");
    let output = child.wait_with_output().expect("runs");
    (
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

fn stdout_of(source: &str) -> String {
    run(source, &[]).0
}

/// Asserts the program prints the same on the interpreter and on the VM.
#[track_caller]
fn assert_engines_agree(source: &str) {
    let vm = stdout_of(source);
    let interp = run(source, &["--interp"]).0;
    assert_eq!(
        vm, interp,
        "engines disagree for:\n{source}\nvm: {vm:?}\ninterp: {interp:?}"
    );
}

#[test]
fn a_route_matches_and_captures_its_parameters() {
    let source = r#"
let params = http_match_route("/users/:id/posts/:post", "/users/7/posts/9")
print(params["id"])
print(params["post"])
"#;
    assert_eq!(stdout_of(source), "7\n9\n");
    assert_engines_agree(source);
}

#[test]
fn an_unmatched_route_is_null() {
    let source = r#"
let params = http_match_route("/users/:id", "/teams/7")
print(params)
"#;
    assert_eq!(stdout_of(source), "null\n");
}

#[test]
fn a_trailing_wildcard_captures_the_rest_of_the_path() {
    let source = r#"
let params = http_match_route("/static/*", "/static/css/site.css")
print(params["*"])
"#;
    assert_eq!(stdout_of(source), "css/site.css\n");
}

#[test]
fn a_cookie_carries_its_attributes() {
    let source = r#"
let attrs = {"httpOnly": true, "secure": true, "sameSite": "Lax", "path": "/", "maxAge": 3600}
print(http_cookie("sid", "abc123", attrs))
"#;
    assert_eq!(
        stdout_of(source),
        "sid=abc123; Path=/; Max-Age=3600; HttpOnly; Secure; SameSite=Lax\n"
    );
    assert_engines_agree(source);
}

#[test]
fn a_cookie_value_cannot_forge_an_attribute() {
    // A raw semicolon would end the cookie and let the rest be read as an
    // attribute, which is how a session cookie turns into an HttpOnly one.
    let source = r#"
print(http_cookie("sid", "abc; HttpOnly"))
"#;
    assert_eq!(stdout_of(source), "sid=abc%3B%20HttpOnly\n");
}

#[test]
fn cookies_are_parsed_back_out_of_a_header() {
    let source = r#"
let jar = http_parse_cookies("a=1; sid=xyz; theme=dark")
print(jar["sid"])
print(jar["theme"])
print(has(jar, "missing"))
"#;
    assert_eq!(stdout_of(source), "xyz\ndark\nfalse\n");
    assert_engines_agree(source);
}

#[test]
fn a_cookie_round_trips_through_build_and_parse() {
    let source = r#"
// The built header is "sid=abc123"; parsing it must recover the value, which is
// what a server sees on the next request.
let built = http_cookie("sid", "abc123")
let jar = http_parse_cookies(built)
print(jar["sid"])
"#;
    assert_eq!(stdout_of(source), "abc123\n");
}

#[test]
fn an_unknown_cookie_option_is_refused() {
    // A typo in httpOnly would otherwise silently leave a session cookie
    // readable from JavaScript.
    let source = r#"
print(http_cookie("sid", "v", {"httpOnlyo": true}))
"#;
    let (_, stderr) = run(source, &[]);
    assert!(stderr.contains("unknown cookie option"), "got {stderr}");
}

#[test]
fn validation_reports_every_problem_at_once() {
    let source = r#"
let body = json_decode("{\"age\": \"old\"}")
let rules = {"name": "required", "age": "number"}
let check = http_validate(body, rules)
print(check["valid"])
print(check["errors"])
"#;
    assert_eq!(
        stdout_of(source),
        "false\n[\"'name' is required\", \"'age' should be a number, got string\"]\n"
    );
    assert_engines_agree(source);
}

#[test]
fn a_valid_body_reports_no_errors_at_all() {
    let source = r#"
let body = json_decode("{\"name\": \"Ada\"}")
let check = http_validate(body, {"name": "required"})
print(check["valid"])
print(has(check, "errors"))
"#;
    assert_eq!(stdout_of(source), "true\nfalse\n");
}

#[test]
fn a_null_field_counts_as_absent() {
    let source = r#"
let body = json_decode("{\"name\": null}")
let check = http_validate(body, {"name": "required"})
print(check["valid"])
"#;
    assert_eq!(stdout_of(source), "false\n");
}

#[test]
fn an_error_response_is_a_usable_shape() {
    let source = r#"
let err = http_error(404, "user_not_found", "no user with that id")
print(err["status"])
print(err["reason"])
print(err["headers"]["Content-Type"])
print(err["body"])
"#;
    assert_eq!(
        stdout_of(source),
        "404\nNot Found\napplication/json\n{\"error\":\"user_not_found\",\"message\":\"no user with that id\"}\n"
    );
    assert_engines_agree(source);
}

#[test]
fn an_error_message_defaults_to_the_reason_phrase() {
    let source = r#"
let err = http_error(503, "unavailable")
let body = json_decode(err["body"])
print(body["message"])
"#;
    assert_eq!(stdout_of(source), "Service Unavailable\n");
}

#[test]
fn a_status_outside_the_http_range_is_refused() {
    let source = r#"
print(http_error(42, "nope"))
"#;
    let (_, stderr) = run(source, &[]);
    assert!(stderr.contains("not an HTTP status code"), "got {stderr}");
}

#[test]
fn reason_phrases_cover_the_codes_a_server_uses() {
    let source = r#"
print(http_status_text(200))
print(http_status_text(201))
print(http_status_text(400))
print(http_status_text(401))
print(http_status_text(403))
print(http_status_text(405))
print(http_status_text(429))
print(http_status_text(500))
print(http_status_text(503))
"#;
    assert_eq!(
        stdout_of(source),
        "OK\nCreated\nBad Request\nUnauthorized\nForbidden\nMethod Not Allowed\nToo Many Requests\nInternal Server Error\nService Unavailable\n"
    );
    assert_engines_agree(source);
}

#[test]
fn arity_errors_are_reported_clearly() {
    for (source, expected) in [
        (
            "print(http_match_route(\"/a\"))",
            "requires a pattern and a path",
        ),
        ("print(http_parse_cookies())", "requires a Cookie header"),
        ("print(http_cookie(\"a\"))", "requires a name"),
        ("print(http_error(404))", "requires a status"),
        ("print(http_validate(1))", "requires a body"),
        ("print(http_status_text())", "requires a status code"),
    ] {
        let (_, stderr) = run(source, &[]);
        assert!(stderr.contains(expected), "for {source}: got {stderr}");
    }
}

#[test]
fn a_route_with_an_invalid_pattern_is_reported() {
    let source = r#"
print(http_match_route("/users/:", "/users/1"))
"#;
    let (_, stderr) = run(source, &[]);
    assert!(
        stderr.contains("not a valid parameter name"),
        "got {stderr}"
    );
}

#[test]
fn a_malformed_percent_escape_does_not_match() {
    let source = r#"
print(http_match_route("/f/:name", "/f/%zz"))
"#;
    assert_eq!(stdout_of(source), "null\n");
}

#[test]
fn a_captured_value_is_percent_decoded() {
    let source = r#"
print(http_match_route("/f/:name", "/f/my%20report.pdf")["name"])
"#;
    assert_eq!(stdout_of(source), "my report.pdf\n");
}

// `http_route` builds a router object, so this needs the server feature.
#[cfg(feature = "server")]
#[test]
fn a_route_built_with_http_route_can_be_matched() {
    // The route descriptor and the matcher have to agree, or a declared route
    // would silently never fire.
    let source = r#"
let route = http_route("GET", "/users/:id", "show_user")
print(route["path"])
print(http_match_route(route["path"], "/users/3")["id"])
"#;
    assert_eq!(stdout_of(source), "/users/:id\n3\n");
    assert_engines_agree(source);
}

#[test]
fn a_decoded_nul_does_not_become_a_parameter() {
    // `%00` decodes cleanly but truncates every C string downstream, so a value
    // that looks one way to Nect and shorter to anything else must be refused.
    let source = r#"
print(http_match_route("/f/:name", "/f/a%00b"))
"#;
    assert_eq!(stdout_of(source), "null\n");
}

#[test]
fn a_percent_encoded_slash_is_part_of_the_value_not_a_separator() {
    // `%2F` must not split one segment into two, or a path could smuggle an
    // extra segment past a route that matched on segment count.
    let source = r#"
print(http_match_route("/files/:name", "/files/a%2Fb")["name"])
print(http_match_route("/files/:name", "/files/a/b"))
"#;
    assert_eq!(stdout_of(source), "a/b\nnull\n");
}
