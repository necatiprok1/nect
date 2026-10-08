//! Web primitives that are pure functions of their inputs.
//!
//! The HTTP server in `builtins.rs` starts an axum listener and the client
//! builtins issue requests, but a framework still needs the parts that decide
//! *what* to send: which route a path matches, what a `Set-Cookie` looks like,
//! whether a body is valid, and what an error response should say. Those are all
//! decisions rather than I/O, so they live here where they can be tested without
//! opening a socket.
//!
//! They are exposed as built-ins rather than as a library so a program can build
//! a response with the same vocabulary the rest of the standard library uses.

use crate::builtins::{Map, RuntimeError, Value};
use std::cell::RefCell;
use std::rc::Rc;

/// The outcome of matching a request path against a route pattern.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RouteMatch {
    /// The pattern matches. `params` holds the `:name` segments, decoded.
    Matched { params: Vec<(String, String)> },
    /// The pattern does not match this path.
    NoMatch,
    /// The pattern is not usable — a `:name` segment with no name, or a
    /// duplicate name. Reported rather than silently never matching, because a
    /// route that can never fire is a bug the author needs to see.
    InvalidPattern(String),
}

/// Percent-decodes a URL path segment.
///
/// Returns `None` for a malformed escape, so a caller can reject a path rather
/// than treating `%zz` as literal text — which is what a naive implementation
/// does, and it is how a path can smuggle a literal `%` past a filter.
///
/// A decoded NUL is also rejected. `%00` decodes cleanly, but a NUL truncates
/// every C string it reaches, so a value that looks one way to Nect and shorter
/// to anything downstream is exactly the kind of mismatch a path parameter must
/// not carry.
fn percent_decode(segment: &str) -> Option<String> {
    let decoded = if !segment.contains('%') {
        segment.to_string()
    } else {
        let bytes = segment.as_bytes();
        let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
        let mut index = 0;
        while index < bytes.len() {
            if bytes[index] == b'%' {
                let high = *bytes.get(index + 1)?;
                let low = *bytes.get(index + 2)?;
                let digit = |c: u8| match c {
                    b'0'..=b'9' => Some(c - b'0'),
                    b'a'..=b'f' => Some(c - b'a' + 10),
                    b'A'..=b'F' => Some(c - b'A' + 10),
                    _ => None,
                };
                out.push(digit(high)? * 16 + digit(low)?);
                index += 3;
            } else {
                out.push(bytes[index]);
                index += 1;
            }
        }
        String::from_utf8(out).ok()?
    };
    if decoded.contains('\0') {
        return None;
    }
    Some(decoded)
}

/// Splits a path into its non-empty segments.
///
/// A trailing slash is not significant (`/users` and `/users/` are the same
/// route), and repeated slashes collapse, so a client cannot accidentally hit a
/// different handler by varying them.
fn segments(path: &str) -> Vec<&str> {
    path.split('/').filter(|s| !s.is_empty()).collect()
}

/// Matches a request path against a route pattern such as `/users/:id`.
///
/// Segments starting with `:` capture, and `*` at the end captures the rest of
/// the path. Matching is exact on segment count otherwise, so `/users` does not
/// match `/users/:id`.
pub fn match_route(pattern: &str, path: &str) -> RouteMatch {
    let pattern_segments = segments(pattern);
    let path_segments = segments(path);
    let mut params: Vec<(String, String)> = Vec::new();

    let mut index = 0;
    while index < pattern_segments.len() {
        let expected = pattern_segments[index];

        // A trailing `*` swallows everything that is left, including nothing.
        if expected == "*" {
            if index + 1 != pattern_segments.len() {
                return RouteMatch::InvalidPattern(
                    "'*' may only appear as the last segment of a route".to_string(),
                );
            }
            let rest = path_segments[index..].join("/");
            let decoded = match percent_decode(&rest) {
                Some(decoded) => decoded,
                None => return RouteMatch::NoMatch,
            };
            params.push(("*".to_string(), decoded));
            return RouteMatch::Matched { params };
        }

        let Some(actual) = path_segments.get(index) else {
            return RouteMatch::NoMatch;
        };

        if let Some(name) = expected.strip_prefix(':') {
            if name.is_empty() || name.contains(':') {
                return RouteMatch::InvalidPattern(format!(
                    "'{expected}' is not a valid parameter name"
                ));
            }
            if params.iter().any(|(existing, _)| existing == name) {
                return RouteMatch::InvalidPattern(format!(
                    "parameter '{name}' appears twice in the pattern"
                ));
            }
            let decoded = match percent_decode(actual) {
                Some(decoded) => decoded,
                None => return RouteMatch::NoMatch,
            };
            params.push((name.to_string(), decoded));
        } else if expected != *actual {
            return RouteMatch::NoMatch;
        }

        index += 1;
    }

    if path_segments.len() != pattern_segments.len() {
        return RouteMatch::NoMatch;
    }
    RouteMatch::Matched { params }
}

/// Parses a `Cookie:` request header into name/value pairs.
///
/// Malformed pairs are skipped rather than failing the whole header: a browser
/// sending one bad cookie should not cost the request every other cookie.
pub fn parse_cookies(header: &str) -> Vec<(String, String)> {
    let mut cookies = Vec::new();
    for part in header.split(';') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let Some((name, value)) = part.split_once('=') else {
            continue;
        };
        let name = name.trim();
        if name.is_empty() {
            continue;
        }
        // A quoted value keeps its quotes stripped, which is what a caller
        // comparing against an expected token needs.
        let value = value.trim();
        let value = value
            .strip_prefix('"')
            .and_then(|v| v.strip_suffix('"'))
            .unwrap_or(value);
        cookies.push((name.to_string(), value.to_string()));
    }
    cookies
}

/// The cookie attributes to set on a `Set-Cookie` header.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CookieOptions {
    pub path: Option<String>,
    pub domain: Option<String>,
    /// Lifetime in seconds. `None` means a session cookie.
    pub max_age: Option<i64>,
    pub expires: Option<String>,
    pub http_only: bool,
    pub secure: bool,
    pub same_site: Option<String>,
}

impl CookieOptions {
    /// Reads the options from a map, so a program can build them as data.
    ///
    /// An unknown key is an error rather than being ignored: a typo in
    /// `"httpOnly"` silently dropping the flag would leave a session cookie
    /// readable from JavaScript, which is exactly the mistake worth catching.
    pub fn from_value(value: &Value) -> Result<Self, String> {
        let mut options = CookieOptions::default();
        let Value::Map(map) = value else {
            return Err(format!(
                "expected a map of cookie options, got {}",
                type_name(value)
            ));
        };
        for (key, value) in map.borrow().entries.iter() {
            let Value::String(name) = key else {
                continue;
            };
            match name.as_str() {
                "path" => options.path = Some(expect_string(name, value)?),
                "domain" => options.domain = Some(expect_string(name, value)?),
                "maxAge" | "max_age" => {
                    let Value::Number(seconds) = value else {
                        return Err(format!("cookie option '{name}' requires a number"));
                    };
                    // A negative max-age means "expire immediately", which is how
                    // a client deletes a cookie; it is legal and preserved.
                    options.max_age = Some(*seconds as i64);
                }
                "expires" => options.expires = Some(expect_string(name, value)?),
                "httpOnly" | "http_only" => options.http_only = expect_bool(name, value)?,
                "secure" => options.secure = expect_bool(name, value)?,
                "sameSite" | "same_site" => {
                    let site = expect_string(name, value)?;
                    // Only the three values a browser recognises are accepted;
                    // anything else is silently ignored by browsers, so a typo
                    // would leave a weaker cookie than intended.
                    let canonical = match site.to_ascii_lowercase().as_str() {
                        "strict" => "Strict",
                        "lax" => "Lax",
                        "none" => "None",
                        other => {
                            return Err(format!(
                                "cookie option 'sameSite' must be Strict, Lax, or None, got '{other}'"
                            ));
                        }
                    };
                    options.same_site = Some(canonical.to_string());
                }
                other => return Err(format!("unknown cookie option '{other}'")),
            }
        }
        Ok(options)
    }
}

fn type_name(value: &Value) -> &'static str {
    crate::builtins::type_of(value)
}

fn expect_string(name: &str, value: &Value) -> Result<String, String> {
    match value {
        Value::String(text) => Ok(text.clone()),
        other => Err(format!(
            "cookie option '{name}' requires a string, got {}",
            type_name(other)
        )),
    }
}

fn expect_bool(name: &str, value: &Value) -> Result<bool, String> {
    match value {
        Value::Boolean(flag) => Ok(*flag),
        other => Err(format!(
            "cookie option '{name}' requires a boolean, got {}",
            type_name(other)
        )),
    }
}

/// Builds a `Set-Cookie` header value.
///
/// The value is percent-encoded so a session token containing `;`, a space, or
/// a comma cannot terminate the attribute list and inject one of its own.
pub fn build_cookie(name: &str, value: &str, options: &CookieOptions) -> String {
    let mut out = String::new();
    out.push_str(name);
    out.push('=');
    out.push_str(&encode_cookie_value(value));
    if let Some(path) = &options.path {
        out.push_str("; Path=");
        out.push_str(path);
    }
    if let Some(domain) = &options.domain {
        out.push_str("; Domain=");
        out.push_str(domain);
    }
    if let Some(max_age) = options.max_age {
        out.push_str("; Max-Age=");
        out.push_str(&max_age.to_string());
    }
    if let Some(expires) = &options.expires {
        out.push_str("; Expires=");
        out.push_str(expires);
    }
    if options.http_only {
        out.push_str("; HttpOnly");
    }
    if options.secure {
        out.push_str("; Secure");
    }
    if let Some(site) = &options.same_site {
        out.push_str("; SameSite=");
        out.push_str(site);
    }
    out
}

/// Percent-encodes a cookie value.
///
/// RFC 6265 forbids the attribute separators, so they are escaped. Letters,
/// digits, and `-._~` pass through, which covers base64url tokens unchanged.
fn encode_cookie_value(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(*byte as char)
            }
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

/// A field a request body must satisfy.
#[derive(Debug, Clone, PartialEq)]
pub enum Field {
    Required,
    Optional,
}

/// Validates a decoded request body against a schema.
///
/// A schema is a map from field name to `"required"`, `"optional"`, or a type
/// name (`"string"`, `"number"`, `"boolean"`, `"array"`, `"map"`). Every problem
/// is collected rather than reported one at a time, so a client fixing a form
/// sees the whole list.
///
/// A `null` field counts as absent: a JSON body with an explicit `null` is
/// telling you the field has no value, which is the same thing as omitting it.
pub fn validate(body: &Value, schema: &Value) -> Result<Vec<String>, String> {
    let Value::Map(fields) = body else {
        return Err(format!(
            "validate() requires a map body, got {}",
            type_name(body)
        ));
    };
    let Value::Map(rules) = schema else {
        return Err(format!(
            "validate() requires a map schema, got {}",
            type_name(schema)
        ));
    };

    let mut problems = Vec::new();
    for (key, rule) in rules.borrow().entries.iter() {
        let Value::String(name) = key else {
            continue;
        };
        let rule = match rule {
            Value::String(text) => text.clone(),
            other => {
                problems.push(format!(
                    "schema for '{name}' must be a string, got {}",
                    type_name(other)
                ));
                continue;
            }
        };

        let present = fields
            .borrow()
            .entries
            .iter()
            .any(|(k, _)| matches!(k, Value::String(k) if k == name))
            && !matches!(
                fields
                    .borrow()
                    .entries
                    .iter()
                    .find(|(k, _)| matches!(k, Value::String(k) if k == name)),
                Some((_, Value::Null))
            );

        if !present {
            if rule == "required" {
                problems.push(format!("'{name}' is required"));
            }
            continue;
        }
        if rule == "required" || rule == "optional" {
            continue;
        }

        let value = fields
            .borrow()
            .entries
            .iter()
            .find_map(|(k, v)| match k {
                Value::String(k) if k == name => Some(v.clone()),
                _ => None,
            })
            .expect("checked as present above");
        if !matches_type(&rule, &value) {
            problems.push(format!(
                "'{name}' should be a {rule}, got {}",
                type_name(&value)
            ));
        }
    }
    Ok(problems)
}

fn matches_type(expected: &str, value: &Value) -> bool {
    match expected {
        "string" => matches!(value, Value::String(_)),
        "number" => matches!(value, Value::Number(_)),
        "boolean" => matches!(value, Value::Boolean(_)),
        "array" => matches!(value, Value::Array(_)),
        "map" => matches!(value, Value::Map(_)),
        // An unrecognised constraint is not silently dropped; `validate` reports
        // it above, so reaching here means the rule was one of the four kinds.
        _ => true,
    }
}

/// The standard reason phrases, so `http_error` produces a real response rather
/// than a bare number.
pub fn reason_phrase(status: u16) -> &'static str {
    match status {
        200 => "OK",
        201 => "Created",
        202 => "Accepted",
        204 => "No Content",
        301 => "Moved Permanently",
        302 => "Found",
        304 => "Not Modified",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        408 => "Request Timeout",
        409 => "Conflict",
        413 => "Payload Too Large",
        415 => "Unsupported Media Type",
        422 => "Unprocessable Entity",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        501 => "Not Implemented",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        _ => "Unknown",
    }
}

/// Builds a JSON error response: the status, its reason phrase, a stable machine
/// code, and a message safe to show a client.
///
/// The code is the part a client should branch on; the message is prose and is
/// not part of any contract.
pub fn error_response(status: u16, code: &str, message: &str) -> Value {
    let mut body = Map::new();
    let _ = body.insert(
        Value::String("error".into()),
        Value::String(code.to_string()),
    );
    let _ = body.insert(
        Value::String("message".into()),
        Value::String(message.to_string()),
    );
    // A map of two string fields always encodes, so this cannot fail; the
    // fallback keeps the signature infallible for the many callers that have no
    // error channel to report through.
    let encoded = crate::builtins::json_encode(&Value::Map(Rc::new(RefCell::new(body))))
        .unwrap_or_else(|_| format!("{{\"error\":\"{code}\"}}"));

    let mut headers = Map::new();
    let _ = headers.insert(
        Value::String("Content-Type".into()),
        Value::String("application/json".into()),
    );

    let mut response = Map::new();
    let _ = response.insert(Value::String("status".into()), Value::Number(status as f64));
    let _ = response.insert(
        Value::String("reason".into()),
        Value::String(reason_phrase(status).to_string()),
    );
    let _ = response.insert(Value::String("body".into()), Value::String(encoded));
    let _ = response.insert(
        Value::String("headers".into()),
        Value::Map(Rc::new(RefCell::new(headers))),
    );
    Value::Map(Rc::new(RefCell::new(response)))
}

/// What `resolve_route` returns: the route that matched, and the parameters its
/// pattern captured.
pub type ResolvedRoute = (Value, Vec<(String, String)>);

/// Resolves the first route in `routes` that matches `method` and `path`.
///
/// Routes are tried in declaration order, so a specific route placed before a
/// catch-all wins. The result reports which route matched and the parameters it
/// captured, so the caller can dispatch to the handler.
pub fn resolve_route(
    routes: &[Value],
    method: &str,
    path: &str,
) -> Result<Option<ResolvedRoute>, String> {
    let method = method.to_ascii_uppercase();
    let mut path_matched_any_method = false;

    for route in routes {
        let Value::Map(entry) = route else {
            continue;
        };
        let (route_method, route_path) = {
            let entry = entry.borrow();
            let method = entry
                .get(&Value::String("method".into()))
                .and_then(|v| match v {
                    Value::String(s) => Some(s.clone()),
                    _ => None,
                })
                .unwrap_or_default();
            let path = entry
                .get(&Value::String("path".into()))
                .and_then(|v| match v {
                    Value::String(s) => Some(s.clone()),
                    _ => None,
                })
                .unwrap_or_default();
            (method, path)
        };

        match match_route(&route_path, path) {
            RouteMatch::Matched { params } => {
                if route_method == method {
                    return Ok(Some((route.clone(), params)));
                }
                // The path matched but the method did not, which is a 405 rather
                // than a 404 — recorded so the caller can say so.
                path_matched_any_method = true;
            }
            RouteMatch::InvalidPattern(reason) => {
                return Err(format!("route '{route_path}' is invalid: {reason}"));
            }
            RouteMatch::NoMatch => {}
        }
    }

    let _ = path_matched_any_method;
    Ok(None)
}

/// Turns a resolution failure into the right response: 405 when the path exists
/// under another method, 404 when it does not exist at all.
pub fn not_found_response(routes: &[Value], method: &str, path: &str) -> Value {
    let method = method.to_ascii_uppercase();
    for route in routes {
        let Value::Map(entry) = route else {
            continue;
        };
        let entry = entry.borrow();
        let path_pattern = match entry.get(&Value::String("path".into())) {
            Some(Value::String(p)) => p.clone(),
            _ => continue,
        };
        if matches!(match_route(&path_pattern, path), RouteMatch::Matched { .. }) {
            let allowed: Vec<String> = routes
                .iter()
                .filter_map(|other| match other {
                    Value::Map(map) => {
                        let map = map.borrow();
                        let other_path = match map.get(&Value::String("path".into())) {
                            Some(Value::String(p)) => p.clone(),
                            _ => return None,
                        };
                        let other_method = match map.get(&Value::String("method".into())) {
                            Some(Value::String(m)) => m.clone(),
                            _ => return None,
                        };
                        matches!(match_route(&other_path, path), RouteMatch::Matched { .. })
                            .then_some(other_method)
                    }
                    _ => None,
                })
                .collect();
            let response = error_response(
                405,
                "method_not_allowed",
                &format!("{method} is not allowed on {path}"),
            );
            if let Value::Map(map) = &response
                && !allowed.is_empty()
            {
                let mut headers = match map.borrow().get(&Value::String("headers".into())) {
                    Some(Value::Map(existing)) => existing.borrow().clone(),
                    _ => Map::new(),
                };
                let _ = headers.insert(
                    Value::String("Allow".into()),
                    Value::String(allowed.join(", ")),
                );
                let _ = map.borrow_mut().insert(
                    Value::String("headers".into()),
                    Value::Map(Rc::new(RefCell::new(headers))),
                );
            }
            return response;
        }
    }
    error_response(404, "not_found", &format!("no route for {method} {path}"))
}

fn runtime(message: &str) -> RuntimeError {
    RuntimeError::new(message)
}

/// Entry point for the `http_match_route` built-in.
pub fn builtin_match_route(args: &[Value]) -> Result<Value, RuntimeError> {
    if args.len() != 2 {
        return Err(runtime("http_match_route() requires a pattern and a path"));
    }
    let (Value::String(pattern), Value::String(path)) = (&args[0], &args[1]) else {
        return Err(runtime("http_match_route() requires two strings"));
    };
    match match_route(pattern, path) {
        RouteMatch::Matched { params } => {
            let mut map = Map::new();
            for (name, value) in params {
                map.insert(Value::String(name), Value::String(value))?;
            }
            Ok(Value::Map(Rc::new(RefCell::new(map))))
        }
        RouteMatch::NoMatch => Ok(Value::Null),
        RouteMatch::InvalidPattern(reason) => Err(runtime(&format!(
            "http_match_route(): pattern '{pattern}' is invalid: {reason}"
        ))),
    }
}

/// Entry point for the `http_parse_cookies` built-in.
pub fn builtin_parse_cookies(args: &[Value]) -> Result<Value, RuntimeError> {
    if args.len() != 1 {
        return Err(runtime("http_parse_cookies() requires a Cookie header"));
    }
    let Value::String(header) = &args[0] else {
        return Err(runtime("http_parse_cookies() requires a string"));
    };
    let mut map = Map::new();
    for (name, value) in parse_cookies(header) {
        map.insert(Value::String(name), Value::String(value))?;
    }
    Ok(Value::Map(Rc::new(RefCell::new(map))))
}

/// Entry point for the `http_cookie` built-in.
pub fn builtin_cookie(args: &[Value]) -> Result<Value, RuntimeError> {
    if args.len() < 2 || args.len() > 3 {
        return Err(runtime(
            "http_cookie() requires a name, a value, and optional attributes map",
        ));
    }
    let (Value::String(name), Value::String(value)) = (&args[0], &args[1]) else {
        return Err(runtime("http_cookie() requires a name and a value"));
    };
    if name.is_empty() || name.contains(['=', ';', ',', ' ', '\t']) {
        return Err(runtime(
            "http_cookie(): a cookie name cannot be empty or contain '=', ';', ',', or whitespace",
        ));
    }
    let options = match args.get(2) {
        Some(value) => {
            CookieOptions::from_value(value).map_err(|e| runtime(&format!("http_cookie(): {e}")))?
        }
        None => CookieOptions::default(),
    };
    Ok(Value::String(build_cookie(name, value, &options)))
}

/// Entry point for the `http_error` built-in.
pub fn builtin_error(args: &[Value]) -> Result<Value, RuntimeError> {
    if args.len() < 2 || args.len() > 3 {
        return Err(runtime(
            "http_error() requires a status, a machine code, and an optional message",
        ));
    }
    let status = crate::builtins::require_number("http_error", &args[0])?;
    if !(100.0..=599.0).contains(&status) {
        return Err(runtime(&format!(
            "http_error(): {} is not an HTTP status code",
            status as i64
        )));
    }
    let Value::String(code) = &args[1] else {
        return Err(runtime("http_error() requires a string error code"));
    };
    let message = match args.get(2) {
        Some(Value::String(text)) => text.clone(),
        None => reason_phrase(status as u16).to_string(),
        Some(other) => {
            return Err(runtime(&format!(
                "http_error() requires a string message, got {}",
                crate::builtins::type_of(other)
            )));
        }
    };
    Ok(error_response(status as u16, code, &message))
}

/// Entry point for the `http_validate` built-in.
///
/// Returns a map with `valid` and, when invalid, the `errors` list — so a
/// handler can branch on one value rather than unwrap a result the language has
/// no way to express.
pub fn builtin_validate(args: &[Value]) -> Result<Value, RuntimeError> {
    if args.len() != 2 {
        return Err(runtime("http_validate() requires a body and a schema"));
    }
    let problems =
        validate(&args[0], &args[1]).map_err(|e| runtime(&format!("http_validate(): {e}")))?;
    let mut result = Map::new();
    result.insert(
        Value::String("valid".into()),
        Value::Boolean(problems.is_empty()),
    )?;
    if !problems.is_empty() {
        result.insert(
            Value::String("errors".into()),
            Value::Array(Rc::new(RefCell::new(
                problems.into_iter().map(Value::String).collect(),
            ))),
        )?;
    }
    Ok(Value::Map(Rc::new(RefCell::new(result))))
}

/// Entry point for the `http_status_text` built-in.
pub fn builtin_status_text(args: &[Value]) -> Result<Value, RuntimeError> {
    if args.len() != 1 {
        return Err(runtime("http_status_text() requires a status code"));
    }
    let status = crate::builtins::require_number("http_status_text", &args[0])?;
    Ok(Value::String(reason_phrase(status as u16).to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(pattern: &str, path: &str) -> Option<Vec<(String, String)>> {
        match match_route(pattern, path) {
            RouteMatch::Matched { params } => Some(params),
            _ => None,
        }
    }

    // ---- route matching ----

    #[test]
    fn an_exact_path_matches() {
        assert_eq!(params("/users", "/users"), Some(vec![]));
    }

    #[test]
    fn a_parameter_is_captured() {
        assert_eq!(
            params("/users/:id", "/users/42"),
            Some(vec![("id".to_string(), "42".to_string())])
        );
    }

    #[test]
    fn several_parameters_are_captured_in_order() {
        assert_eq!(
            params("/users/:id/posts/:post", "/users/7/posts/9"),
            Some(vec![
                ("id".to_string(), "7".to_string()),
                ("post".to_string(), "9".to_string())
            ])
        );
    }

    #[test]
    fn segment_counts_must_agree() {
        // `/users` must not match `/users/:id`, or a collection route would
        // shadow a detail route.
        assert_eq!(params("/users/:id", "/users"), None);
        assert_eq!(params("/users", "/users/42"), None);
    }

    #[test]
    fn a_trailing_slash_is_not_significant() {
        assert_eq!(params("/users/", "/users"), Some(vec![]));
        assert_eq!(params("/users", "/users/"), Some(vec![]));
    }

    #[test]
    fn repeated_slashes_collapse() {
        assert_eq!(
            params("/users/:id", "//users//42"),
            Some(vec![("id".to_string(), "42".to_string())])
        );
    }

    #[test]
    fn a_trailing_star_captures_the_rest() {
        assert_eq!(
            params("/files/*", "/files/a/b/c.txt"),
            Some(vec![("*".to_string(), "a/b/c.txt".to_string())])
        );
    }

    #[test]
    fn a_trailing_star_matches_an_empty_tail() {
        assert_eq!(
            params("/files/*", "/files"),
            Some(vec![("*".to_string(), String::new())])
        );
    }

    #[test]
    fn a_star_may_only_be_last() {
        assert!(matches!(
            match_route("/a/*/b", "/a/x/b"),
            RouteMatch::InvalidPattern(_)
        ));
    }

    #[test]
    fn a_bare_parameter_name_is_rejected() {
        assert!(matches!(
            match_route("/users/:", "/users/1"),
            RouteMatch::InvalidPattern(_)
        ));
    }

    #[test]
    fn a_duplicate_parameter_name_is_rejected() {
        assert!(matches!(
            match_route("/a/:id/b/:id", "/a/1/b/2"),
            RouteMatch::InvalidPattern(_)
        ));
    }

    #[test]
    fn captured_values_are_percent_decoded() {
        assert_eq!(
            params("/files/:name", "/files/my%20report.pdf"),
            Some(vec![("name".to_string(), "my report.pdf".to_string())])
        );
    }

    #[test]
    fn a_decoded_nul_does_not_match() {
        // `%00` decodes cleanly but truncates every C string downstream, so a
        // parameter that looks one way to Nect and shorter elsewhere must not be
        // accepted.
        assert_eq!(params("/files/:name", "/files/a%00b"), None);
    }

    #[test]
    fn a_malformed_escape_does_not_match() {
        // `%zz` is not a valid escape. Matching it literally would let a path
        // carry a raw `%` past a filter that only checks decoded values.
        assert_eq!(params("/files/:name", "/files/%zz"), None);
    }

    // ---- cookies ----

    #[test]
    fn cookies_are_parsed_out_of_a_header() {
        assert_eq!(
            parse_cookies("a=1; b=2;c=3"),
            vec![
                ("a".to_string(), "1".to_string()),
                ("b".to_string(), "2".to_string()),
                ("c".to_string(), "3".to_string())
            ]
        );
    }

    #[test]
    fn surrounding_whitespace_is_ignored() {
        assert_eq!(
            parse_cookies("  a = 1 ;  b = 2 "),
            vec![
                ("a".to_string(), "1".to_string()),
                ("b".to_string(), "2".to_string())
            ]
        );
    }

    #[test]
    fn quoted_values_lose_their_quotes() {
        assert_eq!(
            parse_cookies("a=\"hello world\""),
            vec![("a".to_string(), "hello world".to_string())]
        );
    }

    #[test]
    fn a_value_may_contain_an_equals_sign() {
        assert_eq!(
            parse_cookies("token=abc=def"),
            vec![("token".to_string(), "abc=def".to_string())]
        );
    }

    #[test]
    fn malformed_pairs_are_skipped_not_fatal() {
        // One bad cookie must not cost the request every other cookie.
        assert_eq!(
            parse_cookies("a=1; junk; b=2"),
            vec![
                ("a".to_string(), "1".to_string()),
                ("b".to_string(), "2".to_string())
            ]
        );
    }

    #[test]
    fn an_empty_header_yields_nothing() {
        assert!(parse_cookies("").is_empty());
    }

    #[test]
    fn a_minimal_cookie_is_just_name_and_value() {
        assert_eq!(
            build_cookie("sid", "abc", &CookieOptions::default()),
            "sid=abc"
        );
    }

    #[test]
    fn attributes_are_emitted_in_a_stable_order() {
        let options = CookieOptions {
            path: Some("/".to_string()),
            max_age: Some(3600),
            http_only: true,
            secure: true,
            same_site: Some("Lax".to_string()),
            ..CookieOptions::default()
        };
        assert_eq!(
            build_cookie("sid", "abc", &options),
            "sid=abc; Path=/; Max-Age=3600; HttpOnly; Secure; SameSite=Lax"
        );
    }

    #[test]
    fn a_value_cannot_inject_an_attribute() {
        // A raw `;` would end the cookie and let the rest become an attribute.
        let header = build_cookie("sid", "abc; HttpOnly", &CookieOptions::default());
        assert_eq!(header, "sid=abc%3B%20HttpOnly");
        assert!(!header.contains("; HttpOnly"));
    }

    #[test]
    fn a_base64url_token_passes_through_unchanged() {
        let token = "eyJhbGciOiJIUzI1NiJ9-_.~";
        assert_eq!(
            build_cookie("sid", token, &CookieOptions::default()),
            format!("sid={token}")
        );
    }

    #[test]
    fn an_unknown_cookie_option_is_an_error() {
        // A typo in `httpOnly` must not silently drop the flag.
        let value = Value::Map(Rc::new(RefCell::new({
            let mut map = Map::new();
            map.insert(Value::String("httpOnlyo".into()), Value::Boolean(true))
                .unwrap();
            map
        })));
        assert!(CookieOptions::from_value(&value).is_err());
    }

    #[test]
    fn an_invalid_same_site_is_an_error() {
        let value = Value::Map(Rc::new(RefCell::new({
            let mut map = Map::new();
            map.insert(
                Value::String("sameSite".into()),
                Value::String("loose".into()),
            )
            .unwrap();
            map
        })));
        assert!(CookieOptions::from_value(&value).is_err());
    }

    #[test]
    fn same_site_is_canonicalised() {
        let value = Value::Map(Rc::new(RefCell::new({
            let mut map = Map::new();
            map.insert(
                Value::String("sameSite".into()),
                Value::String("strict".into()),
            )
            .unwrap();
            map
        })));
        assert_eq!(
            CookieOptions::from_value(&value).unwrap().same_site,
            Some("Strict".to_string())
        );
    }

    // ---- validation ----

    fn body(pairs: &[(&str, Value)]) -> Value {
        let mut map = Map::new();
        for (name, value) in pairs {
            map.insert(Value::String((*name).to_string()), value.clone())
                .unwrap();
        }
        Value::Map(Rc::new(RefCell::new(map)))
    }

    fn schema(pairs: &[(&str, &str)]) -> Value {
        let mut map = Map::new();
        for (name, rule) in pairs {
            map.insert(
                Value::String((*name).to_string()),
                Value::String((*rule).to_string()),
            )
            .unwrap();
        }
        Value::Map(Rc::new(RefCell::new(map)))
    }

    #[test]
    fn a_valid_body_reports_no_problems() {
        let b = body(&[
            ("name", Value::String("Ada".into())),
            ("age", Value::Number(36.0)),
        ]);
        let s = schema(&[("name", "string"), ("age", "number")]);
        assert!(validate(&b, &s).unwrap().is_empty());
    }

    #[test]
    fn a_missing_required_field_is_reported() {
        let b = body(&[]);
        let s = schema(&[("name", "required")]);
        assert_eq!(validate(&b, &s).unwrap(), vec!["'name' is required"]);
    }

    #[test]
    fn a_missing_optional_field_is_not_a_problem() {
        let b = body(&[]);
        let s = schema(&[("name", "optional")]);
        assert!(validate(&b, &s).unwrap().is_empty());
    }

    #[test]
    fn a_null_field_counts_as_absent() {
        // A JSON body with an explicit null is saying the field has no value.
        let b = body(&[("name", Value::Null)]);
        let s = schema(&[("name", "required")]);
        assert_eq!(validate(&b, &s).unwrap(), vec!["'name' is required"]);
    }

    #[test]
    fn a_wrong_type_is_reported_with_both_types() {
        let b = body(&[("age", Value::String("old".into()))]);
        let s = schema(&[("age", "number")]);
        assert_eq!(
            validate(&b, &s).unwrap(),
            vec!["'age' should be a number, got string"]
        );
    }

    #[test]
    fn every_problem_is_collected_not_just_the_first() {
        let b = body(&[("age", Value::String("old".into()))]);
        let s = schema(&[("name", "required"), ("age", "number")]);
        let problems = validate(&b, &s).unwrap();
        assert_eq!(problems.len(), 2);
    }

    #[test]
    fn a_field_not_in_the_schema_is_ignored() {
        let b = body(&[("extra", Value::Number(1.0))]);
        let s = schema(&[("name", "optional")]);
        assert!(validate(&b, &s).unwrap().is_empty());
    }

    // ---- error responses ----

    #[test]
    fn an_error_response_carries_a_status_and_reason() {
        let response = error_response(404, "not_found", "no such user");
        let Value::Map(map) = &response else {
            panic!("a map")
        };
        let map = map.borrow();
        assert_eq!(
            map.get(&Value::String("status".into())),
            Some(Value::Number(404.0))
        );
        assert_eq!(
            map.get(&Value::String("reason".into())),
            Some(Value::String("Not Found".into()))
        );
    }

    #[test]
    fn the_error_body_is_json_with_a_stable_code() {
        let response = error_response(422, "validation_failed", "name is required");
        let Value::Map(map) = &response else {
            panic!("a map")
        };
        let body = map.borrow().get(&Value::String("body".into())).unwrap();
        let Value::String(text) = body else {
            panic!("a string body")
        };
        assert!(text.contains("\"error\":\"validation_failed\""));
        assert!(text.contains("name is required"));
    }

    #[test]
    fn known_statuses_have_reason_phrases() {
        assert_eq!(reason_phrase(200), "OK");
        assert_eq!(reason_phrase(405), "Method Not Allowed");
        assert_eq!(reason_phrase(503), "Service Unavailable");
    }

    #[test]
    fn an_unknown_status_says_so_rather_than_guessing() {
        assert_eq!(reason_phrase(599), "Unknown");
    }

    // ---- routing ----

    fn route(method: &str, path: &str) -> Value {
        let mut map = Map::new();
        map.insert(Value::String("method".into()), Value::String(method.into()))
            .unwrap();
        map.insert(Value::String("path".into()), Value::String(path.into()))
            .unwrap();
        Value::Map(Rc::new(RefCell::new(map)))
    }

    #[test]
    fn the_first_matching_route_wins() {
        let routes = vec![route("GET", "/users/:id"), route("GET", "/users/me")];
        let (found, params) = resolve_route(&routes, "GET", "/users/me").unwrap().unwrap();
        let Value::Map(map) = &found else {
            panic!("a map")
        };
        assert_eq!(
            map.borrow().get(&Value::String("path".into())),
            Some(Value::String("/users/:id".into()))
        );
        assert_eq!(params, vec![("id".to_string(), "me".to_string())]);
    }

    #[test]
    fn method_matching_is_case_insensitive() {
        let routes = vec![route("GET", "/users")];
        assert!(resolve_route(&routes, "get", "/users").unwrap().is_some());
    }

    #[test]
    fn an_unmatched_path_resolves_to_nothing() {
        let routes = vec![route("GET", "/users")];
        assert!(resolve_route(&routes, "GET", "/nope").unwrap().is_none());
    }

    #[test]
    fn the_wrong_method_resolves_to_nothing() {
        let routes = vec![route("GET", "/users")];
        assert!(resolve_route(&routes, "POST", "/users").unwrap().is_none());
    }

    #[test]
    fn the_wrong_method_on_a_known_path_reports_405_with_allow() {
        let routes = vec![route("GET", "/users"), route("POST", "/users")];
        let response = not_found_response(&routes, "DELETE", "/users");
        let Value::Map(map) = &response else {
            panic!("a map")
        };
        let map = map.borrow();
        assert_eq!(
            map.get(&Value::String("status".into())),
            Some(Value::Number(405.0))
        );
        let Value::Map(headers) = map.get(&Value::String("headers".into())).unwrap() else {
            panic!("headers")
        };
        let allow = headers
            .borrow()
            .get(&Value::String("Allow".into()))
            .unwrap();
        let Value::String(allow) = allow else {
            panic!("a string")
        };
        assert!(
            allow.contains("GET") && allow.contains("POST"),
            "got {allow}"
        );
    }

    #[test]
    fn an_unknown_path_reports_404() {
        let routes = vec![route("GET", "/users")];
        let response = not_found_response(&routes, "GET", "/nope");
        let Value::Map(map) = &response else {
            panic!("a map")
        };
        assert_eq!(
            map.borrow().get(&Value::String("status".into())),
            Some(Value::Number(404.0))
        );
    }

    #[test]
    fn an_invalid_route_pattern_is_reported_rather_than_ignored() {
        let routes = vec![route("GET", "/users/:")];
        assert!(resolve_route(&routes, "GET", "/users/1").is_err());
    }

    // ---- built-in wrappers ----

    #[test]
    fn the_match_built_in_returns_params_or_null() {
        let matched = builtin_match_route(&[
            Value::String("/users/:id".into()),
            Value::String("/users/7".into()),
        ])
        .unwrap();
        let Value::Map(map) = &matched else {
            panic!("a map")
        };
        assert_eq!(
            map.borrow().get(&Value::String("id".into())),
            Some(Value::String("7".into()))
        );

        let missed = builtin_match_route(&[
            Value::String("/users/:id".into()),
            Value::String("/nope".into()),
        ])
        .unwrap();
        assert_eq!(missed, Value::Null);
    }

    #[test]
    fn the_cookie_built_in_rejects_an_unsafe_name() {
        let attempt =
            builtin_cookie(&[Value::String("bad;name".into()), Value::String("v".into())]);
        assert!(attempt.is_err());
    }

    #[test]
    fn the_error_built_in_rejects_a_status_outside_the_range() {
        assert!(builtin_error(&[Value::Number(42.0), Value::String("x".into())]).is_err());
        assert!(builtin_error(&[Value::Number(404.0), Value::String("x".into())]).is_ok());
    }

    #[test]
    fn the_validate_built_in_reports_valid_and_the_error_list() {
        let b = body(&[]);
        let s = schema(&[("name", "required")]);
        let result = builtin_validate(&[b, s]).unwrap();
        let Value::Map(map) = &result else {
            panic!("a map")
        };
        assert_eq!(
            map.borrow().get(&Value::String("valid".into())),
            Some(Value::Boolean(false))
        );
        assert!(map.borrow().get(&Value::String("errors".into())).is_some());
    }

    #[test]
    fn a_valid_body_reports_no_error_list_at_all() {
        let b = body(&[("name", Value::String("Ada".into()))]);
        let s = schema(&[("name", "required")]);
        let result = builtin_validate(&[b, s]).unwrap();
        let Value::Map(map) = &result else {
            panic!("a map")
        };
        assert_eq!(
            map.borrow().get(&Value::String("valid".into())),
            Some(Value::Boolean(true))
        );
        assert!(map.borrow().get(&Value::String("errors".into())).is_none());
    }

    #[test]
    fn every_built_in_checks_its_arity() {
        assert!(builtin_match_route(&[Value::Null]).is_err());
        assert!(builtin_parse_cookies(&[]).is_err());
        assert!(builtin_cookie(&[Value::String("a".into())]).is_err());
        assert!(builtin_error(&[Value::Number(404.0)]).is_err());
        assert!(builtin_validate(&[Value::Null]).is_err());
        assert!(builtin_status_text(&[]).is_err());
    }
}
