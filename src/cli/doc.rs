//! `nect doc` — generates an API reference for a Nect project.
//!
//! The generator walks the project's `.nct` files, parses each one, and reports
//! the surface a reader needs in order to use the code: the functions it
//! defines (with their parameter lists), which built-ins each file relies on,
//! and where each file's declarations live.
//!
//! It deliberately does not invent prose. Everything it prints is derived from
//! the AST, so the output cannot drift away from the code the way a
//! hand-maintained reference does. `--check` exists so CI can fail when the
//! committed reference no longer matches what the source says.

use crate::ast::{Expr, Stmt};
use crate::builtins::names as builtin_names;
use crate::lexer::Lexer;
use crate::parser::Parser;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// One function as the reference reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocFunction {
    pub name: String,
    pub params: Vec<String>,
    /// `true` for `async fn`, which the reference labels.
    pub is_async: bool,
}

/// Everything the generator found in one file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DocFile {
    /// Path as it should appear in the output (relative to the project root).
    pub path: String,
    pub functions: Vec<DocFunction>,
    /// Module-level `let` names, which are part of a module's surface.
    pub globals: Vec<String>,
    /// Built-in functions the file calls, sorted.
    pub builtins_used: Vec<String>,
    /// Non-fatal problems: a file that failed to parse is reported rather than
    /// silently omitted, so `--check` notices a broken build.
    pub problems: Vec<String>,
}

impl DocFile {
    /// Whether the file declared anything at all.
    pub fn is_empty(&self) -> bool {
        self.functions.is_empty() && self.globals.is_empty()
    }
}

/// How `nect doc` was invoked.
#[derive(Default)]
pub struct DocOptions {
    /// Where to write the reference. `None` prints to stdout.
    pub out_dir: Option<PathBuf>,
    /// Compare against the committed reference instead of writing it.
    pub check: bool,
}

/// The outcome of a run, so callers and tests can inspect it without parsing
/// the rendered Markdown back.
pub struct DocReport {
    pub files: Vec<DocFile>,
    /// The rendered Markdown, identical to what is written or compared.
    pub markdown: String,
    /// Set when `--check` found the committed reference stale.
    pub stale: Option<String>,
}

/// Collects every `.nct` file under `root`, sorted for a stable document.
///
/// `root` may be a single file, in which case the walk is skipped entirely.
pub fn collect_sources(root: &Path) -> Result<Vec<PathBuf>, String> {
    if root.is_file() {
        return Ok(vec![root.to_path_buf()]);
    }
    if !root.is_dir() {
        return Err(format!("no such file or directory: {}", root.display()));
    }
    let mut found = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries =
            std::fs::read_dir(&dir).map_err(|e| format!("cannot read {}: {}", dir.display(), e))?;
        for entry in entries {
            let entry =
                entry.map_err(|e| format!("cannot read an entry in {}: {}", dir.display(), e))?;
            let path = entry.path();
            let name = entry.file_name();
            let name = name.to_string_lossy();
            // Skip hidden directories and the build/package output dirs, which
            // hold generated code that is not part of the source surface.
            if name.starts_with('.') || name == "target" || name == "node_modules" {
                continue;
            }
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "nct") {
                found.push(path);
            }
        }
    }
    found.sort();
    Ok(found)
}

/// Parses one source file and extracts its documented surface.
pub fn analyze_file(path: &Path, display: &str) -> DocFile {
    let mut file = DocFile {
        path: display.to_string(),
        ..DocFile::default()
    };
    let source = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) => {
            file.problems
                .push(format!("cannot read {}: {}", display, e));
            return file;
        }
    };
    let tokens = match Lexer::new(&source).tokenize() {
        Ok(tokens) => tokens,
        Err(e) => {
            file.problems
                .push(format!("{}:{}:{}: {}", display, e.line, e.col, e.message));
            return file;
        }
    };
    let stmts = match Parser::new(tokens).parse() {
        Ok(stmts) => stmts,
        Err(e) => {
            file.problems
                .push(format!("{}:{}:{}: {}", display, e.line, e.col, e.message));
            return file;
        }
    };
    collect_statements(&stmts, &mut file);
    file.builtins_used.sort();
    file.globals.sort();
    file
}

/// Walks statements at module level. Declarations nested in a function body
/// are that function's business, not the module's, so the walk does not descend
/// into function bodies.
fn collect_statements(stmts: &[Stmt], file: &mut DocFile) {
    let mut builtins: BTreeSet<String> = BTreeSet::new();
    let mut globals: BTreeSet<String> = BTreeSet::new();
    let mut functions: Vec<DocFunction> = Vec::new();

    for stmt in stmts {
        match stmt {
            Stmt::Function { name, params, body } => {
                functions.push(DocFunction {
                    name: name.clone(),
                    params: params.clone(),
                    is_async: false,
                });
                collect_builtins_in(body, &mut builtins);
            }
            Stmt::AsyncFunction { name, params, body } => {
                functions.push(DocFunction {
                    name: name.clone(),
                    params: params.clone(),
                    is_async: true,
                });
                collect_builtins_in(body, &mut builtins);
            }
            Stmt::Let { name, value } => {
                globals.insert(name.clone());
                collect_builtins_in_expr(value, &mut builtins);
            }
            Stmt::Expression(expr) => collect_builtins_in_expr(expr, &mut builtins),
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                collect_builtins_in_expr(condition, &mut builtins);
                collect_builtins_in(then_branch, &mut builtins);
                collect_builtins_in(else_branch, &mut builtins);
            }
            Stmt::While { condition, body } => {
                collect_builtins_in_expr(condition, &mut builtins);
                collect_builtins_in(body, &mut builtins);
            }
            Stmt::For { iterable, body, .. } => {
                collect_builtins_in_expr(iterable, &mut builtins);
                collect_builtins_in(body, &mut builtins);
            }
            Stmt::Return(Some(expr)) => collect_builtins_in_expr(expr, &mut builtins),
            _ => {}
        }
    }

    file.functions = functions;
    file.globals = globals.into_iter().collect();
    file.builtins_used = builtins.into_iter().collect();
}

fn collect_builtins_in(stmts: &[Stmt], out: &mut BTreeSet<String>) {
    for stmt in stmts {
        match stmt {
            Stmt::Function { body, .. } | Stmt::AsyncFunction { body, .. } => {
                collect_builtins_in(body, out)
            }
            Stmt::Block(body) => collect_builtins_in(body, out),
            Stmt::Let { value, .. } => collect_builtins_in_expr(value, out),
            Stmt::Expression(expr) => collect_builtins_in_expr(expr, out),
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                collect_builtins_in_expr(condition, out);
                collect_builtins_in(then_branch, out);
                collect_builtins_in(else_branch, out);
            }
            Stmt::While { condition, body } => {
                collect_builtins_in_expr(condition, out);
                collect_builtins_in(body, out);
            }
            Stmt::For { iterable, body, .. } => {
                collect_builtins_in_expr(iterable, out);
                collect_builtins_in(body, out);
            }
            Stmt::Return(Some(expr)) => collect_builtins_in_expr(expr, out),
            _ => {}
        }
    }
}

fn collect_builtins_in_expr(expr: &Expr, out: &mut BTreeSet<String>) {
    match expr {
        Expr::Call { callee, args, .. } => {
            if let Expr::Variable(name) = &**callee
                && builtin_names().contains(&name.as_str())
            {
                out.insert(name.clone());
            }
            collect_builtins_in_expr(callee, out);
            for arg in args {
                collect_builtins_in_expr(arg, out);
            }
        }
        Expr::Binary { left, right, .. } => {
            collect_builtins_in_expr(left, out);
            collect_builtins_in_expr(right, out);
        }
        Expr::Unary { operand, .. } => collect_builtins_in_expr(operand, out),
        Expr::Array(elements) => {
            for element in elements {
                collect_builtins_in_expr(element, out);
            }
        }
        Expr::Map(entries) => {
            for (key, value) in entries {
                collect_builtins_in_expr(key, out);
                collect_builtins_in_expr(value, out);
            }
        }
        Expr::GetIndex { array, index } => {
            collect_builtins_in_expr(array, out);
            collect_builtins_in_expr(index, out);
        }
        Expr::SetIndex {
            array,
            index,
            value,
            ..
        } => {
            collect_builtins_in_expr(array, out);
            collect_builtins_in_expr(index, out);
            collect_builtins_in_expr(value, out);
        }
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            collect_builtins_in_expr(condition, out);
            collect_builtins_in_expr(then_expr, out);
            collect_builtins_in_expr(else_expr, out);
        }
        Expr::Assign { value, .. } => collect_builtins_in_expr(value, out),
        Expr::Grouping(inner) | Expr::Await(inner) | Expr::Spawn(inner) => {
            collect_builtins_in_expr(inner, out)
        }
        _ => {}
    }
}

/// Renders the Markdown reference for the analyzed files.
///
/// The output is deterministic: files and names are sorted, so regenerating an
/// unchanged project produces byte-identical Markdown and `--check` is stable.
pub fn render(files: &[DocFile], project_name: &str) -> String {
    let mut out = String::new();
    out.push_str(&format!("# {project_name} — API reference\n\n"));
    out.push_str(
        "Generated by `nect doc` from the project's own source. Do not edit by hand;\n\
         run `nect doc` to refresh it.\n\n",
    );

    // A table of contents first, so a long reference is navigable.
    let documented: Vec<&DocFile> = files.iter().filter(|f| !f.is_empty()).collect();
    if !documented.is_empty() {
        out.push_str("## Contents\n\n");
        for file in &documented {
            out.push_str(&format!("- [`{}`](#{})\n", file.path, anchor(&file.path)));
        }
        out.push('\n');
    }

    for file in files {
        if !file.problems.is_empty() {
            out.push_str(&format!("## {}\n\n", file.path));
            out.push_str("Could not be analyzed:\n\n");
            for problem in &file.problems {
                out.push_str(&format!("- {problem}\n"));
            }
            out.push('\n');
            continue;
        }
        if file.is_empty() {
            continue;
        }
        out.push_str(&format!("## {}\n\n", file.path));

        if !file.functions.is_empty() {
            out.push_str("### Functions\n\n");
            for function in &file.functions {
                let params = if function.params.is_empty() {
                    String::new()
                } else {
                    function.params.join(", ")
                };
                let keyword = if function.is_async { "async fn" } else { "fn" };
                out.push_str(&format!("- `{keyword} {}({params})`\n", function.name));
            }
            out.push('\n');
        }

        if !file.globals.is_empty() {
            out.push_str("### Module-level values\n\n");
            for name in &file.globals {
                out.push_str(&format!("- `{name}`\n"));
            }
            out.push('\n');
        }

        if !file.builtins_used.is_empty() {
            out.push_str("### Built-ins used\n\n");
            out.push_str(&format!(
                "{}\n\n",
                file.builtins_used
                    .iter()
                    .map(|b| format!("`{b}`"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
    }

    out
}

/// GitHub-style heading anchor: lowercase, non-alphanumerics dropped, spaces to
/// dashes, so the table of contents links resolve.
fn anchor(path: &str) -> String {
    let mut out = String::new();
    for ch in path.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if ch == '.' || ch == '/' || ch == '_' || ch == '-' {
            out.push('-');
        }
    }
    out
}

/// Runs the generator over `root`.
pub fn generate(root: &Path, options: &DocOptions) -> Result<DocReport, String> {
    // The walk and the reported base have to agree on a spelling of `root`, or
    // `strip_prefix` fails and every path in the document comes out absolute (or
    // prefixed with `./`). Resolving once, up front, removes that whole class of
    // mismatch — including the `.` case, where `.` has no file name of its own.
    let base = if root.is_dir() {
        root.canonicalize()
            .map_err(|e| format!("cannot read {}: {}", root.display(), e))?
    } else {
        // `Path::new("main.nct").parent()` is `Some("")`, not `Some(".")`, and an
        // empty path cannot be canonicalized — so a bare filename is resolved
        // against the working directory explicitly.
        let parent = root.parent().filter(|p| !p.as_os_str().is_empty());
        match parent {
            Some(parent) => parent
                .canonicalize()
                .unwrap_or_else(|_| parent.to_path_buf()),
            None => std::env::current_dir().map_err(|e| e.to_string())?,
        }
    };
    let sources = collect_sources(&base)?;
    if sources.is_empty() {
        return Err(format!("no .nct files found under {}", root.display()));
    }
    // Paths are reported relative to the scanned root so the document does not
    // embed whatever absolute path the developer happened to use.
    let mut files = Vec::with_capacity(sources.len());
    for source in &sources {
        let display = source
            .strip_prefix(&base)
            .unwrap_or(source)
            .to_string_lossy()
            .replace('\\', "/");
        files.push(analyze_file(source, &display));
    }
    let project_name = base
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| "nect project".to_string());
    let markdown = render(&files, &project_name);

    let mut report = DocReport {
        files,
        markdown,
        stale: None,
    };

    if let Some(out_dir) = &options.out_dir {
        std::fs::create_dir_all(out_dir)
            .map_err(|e| format!("cannot create {}: {}", out_dir.display(), e))?;
        let target = out_dir.join("API.md");
        if options.check {
            let existing = std::fs::read_to_string(&target).unwrap_or_default();
            if existing != report.markdown {
                report.stale = Some(format!(
                    "{} is out of date; run `nect doc` to refresh it",
                    target.display()
                ));
            }
        } else {
            std::fs::write(&target, &report.markdown)
                .map_err(|e| format!("cannot write {}: {}", target.display(), e))?;
        }
    }

    Ok(report)
}

/// Groups files by their top-level directory, used by the summary line.
pub fn summarize(files: &[DocFile]) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for file in files {
        let key = match file.path.split_once('/') {
            Some((dir, _)) => dir.to_string(),
            None => ".".to_string(),
        };
        *counts.entry(key).or_insert(0) += 1;
    }
    counts
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(source: &str) -> Vec<Stmt> {
        let tokens = Lexer::new(source).tokenize().expect("lexes");
        Parser::new(tokens).parse().expect("parses")
    }

    fn analyze(source: &str) -> DocFile {
        let stmts = parse(source);
        let mut file = DocFile {
            path: "main.nct".to_string(),
            ..DocFile::default()
        };
        collect_statements(&stmts, &mut file);
        file.builtins_used.sort();
        file.globals.sort();
        file
    }

    #[test]
    fn reports_functions_with_their_parameters() {
        let file = analyze("fn add(a, b) {\n    return a + b\n}\nfn noop() {\n}\n");
        assert_eq!(
            file.functions,
            vec![
                DocFunction {
                    name: "add".to_string(),
                    params: vec!["a".to_string(), "b".to_string()],
                    is_async: false,
                },
                DocFunction {
                    name: "noop".to_string(),
                    params: vec![],
                    is_async: false,
                },
            ]
        );
    }

    #[test]
    fn labels_async_functions() {
        let file = analyze("async fn fetch(url) {\n    return url\n}\n");
        assert!(file.functions[0].is_async);
    }

    #[test]
    fn lists_module_level_values_but_not_locals() {
        let file = analyze("let limit = 10\nfn f() {\n    let hidden = 1\n    return hidden\n}\n");
        assert_eq!(file.globals, vec!["limit".to_string()]);
    }

    #[test]
    fn collects_builtins_from_nested_bodies() {
        let file =
            analyze("fn f(xs) {\n    for x in xs {\n        print(len(sort(xs)))\n    }\n}\n");
        assert_eq!(file.builtins_used, vec!["len", "print", "sort"]);
    }

    #[test]
    fn ignores_calls_to_the_programs_own_functions() {
        let file = analyze("fn helper() {\n}\nfn main() {\n    return helper()\n}\n");
        assert!(file.builtins_used.is_empty());
    }

    #[test]
    fn render_is_stable_across_runs() {
        let file = analyze("fn f() {\n    print(1)\n}\n");
        let first = render(std::slice::from_ref(&file), "demo");
        let second = render(std::slice::from_ref(&file), "demo");
        assert_eq!(first, second);
    }

    #[test]
    fn render_links_every_heading_it_writes() {
        let file = analyze("fn f() {\n}\n");
        let markdown = render(std::slice::from_ref(&file), "demo");
        assert!(markdown.contains("- [`main.nct`](#main-nct)"));
        assert!(markdown.contains("## main.nct"));
    }

    #[test]
    fn a_broken_file_is_reported_not_dropped() {
        let dir = std::env::temp_dir().join("nect-doc-broken");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("bad.nct");
        std::fs::write(&path, "fn f( {\n").unwrap();
        let file = analyze_file(&path, "bad.nct");
        assert_eq!(file.problems.len(), 1);
        assert!(file.problems[0].contains("bad.nct"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_file_with_no_declarations_is_empty() {
        let file = analyze("print(1)\n");
        assert!(file.is_empty());
    }

    #[test]
    fn the_document_is_titled_after_the_project_directory() {
        // `.` has no file name of its own, so running `nect doc` with no
        // argument — the normal case — has to resolve it or the document is
        // titled with a placeholder.
        let dir = std::env::temp_dir().join("nect-doc-title");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        std::fs::write(dir.join("main.nct"), "fn f() {\n}\n").expect("writes");

        let from_dir = generate(&dir, &DocOptions::default()).expect("generates");
        assert!(
            from_dir.markdown.starts_with("# nect-doc-title"),
            "got: {}",
            from_dir.markdown.lines().next().unwrap_or("")
        );

        // And with the directory named as `.` from inside it.
        let previous = std::env::current_dir().expect("cwd");
        std::env::set_current_dir(&dir).expect("enters the directory");
        let from_dot = generate(Path::new("."), &DocOptions::default()).expect("generates");
        std::env::set_current_dir(previous).expect("restores the directory");

        assert!(
            from_dot.markdown.starts_with("# nect-doc-title"),
            "got: {}",
            from_dot.markdown.lines().next().unwrap_or("")
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn analyze_a_missing_file_reports_rather_than_panics() {
        let file = analyze_file(Path::new("/nonexistent/x.nct"), "x.nct");
        assert_eq!(file.problems.len(), 1);
        assert!(file.problems[0].contains("x.nct"));
    }

    #[test]
    fn collecting_a_missing_root_is_an_error() {
        assert!(collect_sources(Path::new("/nonexistent/directory")).is_err());
    }

    #[test]
    fn collecting_a_single_file_yields_that_file() {
        let dir = std::env::temp_dir().join("nect-doc-single");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        let file = dir.join("only.nct");
        std::fs::write(&file, "print(1)\n").expect("writes");
        let found = collect_sources(&file).expect("collects");
        assert_eq!(found, vec![file]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn hidden_and_build_directories_are_skipped() {
        // Generated code in target/ is not part of the source surface, and a
        // hidden directory is not either.
        let dir = std::env::temp_dir().join("nect-doc-skip");
        let _ = std::fs::remove_dir_all(&dir);
        for sub in ["", "target", ".hidden", "node_modules", "src"] {
            std::fs::create_dir_all(dir.join(sub)).expect("creates a directory");
            std::fs::write(dir.join(sub).join("f.nct"), "print(1)\n").expect("writes");
        }
        let found = collect_sources(&dir).expect("collects");
        let names: Vec<String> = found
            .iter()
            .map(|p| p.strip_prefix(&dir).unwrap().to_string_lossy().to_string())
            .collect();
        assert!(names.contains(&"f.nct".to_string()), "got {names:?}");
        // Paths are normalised to `/` so the document is identical on every
        // platform.
        assert!(names.contains(&"src/f.nct".to_string()), "got {names:?}");
        assert!(
            !names.iter().any(|n| n.starts_with("target")),
            "got {names:?}"
        );
        assert!(
            !names.iter().any(|n| n.starts_with(".hidden")),
            "got {names:?}"
        );
        assert!(
            !names.iter().any(|n| n.starts_with("node_modules")),
            "got {names:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn summarizing_an_empty_report_is_empty() {
        assert!(summarize(&[]).is_empty());
    }

    #[test]
    fn summarize_groups_by_top_level_directory() {
        let files = vec![
            DocFile {
                path: "src/a.nct".to_string(),
                ..DocFile::default()
            },
            DocFile {
                path: "src/b.nct".to_string(),
                ..DocFile::default()
            },
            DocFile {
                path: "top.nct".to_string(),
                ..DocFile::default()
            },
        ];
        let counts = summarize(&files);
        assert_eq!(counts.get("src"), Some(&2));
        assert_eq!(counts.get("."), Some(&1));
    }
}
