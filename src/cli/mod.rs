use crate::lexer::Lexer;
use crate::parser::Parser;
use std::fs;
use std::io::{self, Read};

#[derive(Debug, Clone)]
pub struct SourceMap {
    lines: Vec<String>,
}

impl SourceMap {
    pub fn new(source: &str) -> Self {
        Self {
            lines: source.lines().map(|l| l.to_string()).collect(),
        }
    }

    pub fn format(&self, line: usize, col: usize, msg: &str) -> String {
        let mut result = String::new();
        if line > 0 && line <= self.lines.len() {
            let src_line = self.lines[line - 1].clone();
            let line_num_width = line.to_string().len();
            let gutter = format!("{} | ", line);
            result.push_str(&format!("{}{}\n", gutter, src_line));
            let pointer_spaces: String = " ".repeat(col.saturating_sub(1));
            result.push_str(&format!(
                "{} | {}{}\n",
                " ".repeat(line_num_width),
                pointer_spaces,
                "^"
            ));
        }
        result.push_str(&format!("error: {}", msg));
        result
    }
}

/// Which implementation executes a program.
///
/// `Vm` (the bytecode VM, with the JIT when available) is the default.
/// `Interpreter` is the original tree-walking reference implementation, kept so
/// the two can be compared program-for-program (see `tests/differential_tests.rs`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Engine {
    Vm,
    Interpreter,
}

fn read_source(path: &str) -> Result<String, String> {
    if path == "-" {
        let mut buf = String::new();
        io::stdin()
            .read_to_string(&mut buf)
            .map_err(|e| format!("cannot read from stdin: {}", e))?;
        expand_imports(&buf, None)
    } else {
        let raw = fs::read_to_string(path)
            .map_err(|e| format!("cannot read file '{}': {}", path, e))?;
        let base = std::path::Path::new(path)
            .parent()
            .map(|p| p.to_path_buf());
        expand_imports(&raw, base.as_deref())
    }
}

/// The standard library ships inside the binary: `import "std/..."` resolves
/// here, so scripts need no installation layout on disk.
const STDLIB: &[(&str, &str)] = &[("std/ui.nct", include_str!("../../std/ui.nct"))];

/// Expands `import "..."` lines by splicing the imported module's source into
/// the program, before lexing — the engines never see an import statement.
///
/// * `import "std/ui.nct"` resolves from [`STDLIB`] (embedded, always found).
/// * Other paths resolve relative to the importing file (or the working
///   directory for stdin), and modules are spliced exactly once, so shared
///   imports and import cycles are both safe.
/// * An `import` must be alone on its line; anything else passes through.
fn expand_imports(source: &str, base_dir: Option<&std::path::Path>) -> Result<String, String> {
    let mut seen = std::collections::HashSet::new();
    expand_imports_inner(source, base_dir, &mut seen)
}

fn expand_imports_inner(
    source: &str,
    base_dir: Option<&std::path::Path>,
    seen: &mut std::collections::HashSet<std::path::PathBuf>,
) -> Result<String, String> {
    let mut out = String::with_capacity(source.len());
    for line in source.lines() {
        let trimmed = line.trim();
        let module_path = trimmed
            .strip_prefix("import ")
            .map(|rest| rest.trim())
            .filter(|rest| rest.starts_with('"') && rest.ends_with('"') && rest.len() >= 3)
            .map(|rest| &rest[1..rest.len() - 1]);
        match module_path {
            Some(path) => {
                if let Some((_, text)) = STDLIB.iter().find(|(name, _)| *name == path) {
                    out.push_str(text);
                    out.push('\n');
                } else {
                    let candidate = match base_dir {
                        Some(base) => base.join(path),
                        None => std::path::PathBuf::from(path),
                    };
                    let canonical = candidate.canonicalize().map_err(|e| {
                        format!("cannot import '{}': {}", path, e)
                    })?;
                    if seen.insert(canonical.clone()) {
                        let text = fs::read_to_string(&canonical)
                            .map_err(|e| format!("cannot import '{}': {}", path, e))?;
                        out.push_str(&expand_imports_inner(
                            &text,
                            canonical.parent(),
                            seen,
                        )?);
                        out.push('\n');
                    }
                    // Already spliced once: a shared import or a cycle — skip.
                }
            }
            None => {
                out.push_str(line);
                out.push('\n');
            }
        }
    }
    Ok(out)
}

/// Runs through the default engine. Library-level entry: like the CLI, it
/// expands `import` lines (resolving `std/` modules from the embedded
/// library, file paths from the working directory).
pub fn run_source(source: &str) -> Result<(), RunError> {
    let expanded = expand_imports(source, None).map_err(RunError::simple)?;
    run_source_with_engine(&expanded, Engine::Vm)
}

pub fn run_source_with_engine(source: &str, engine: Engine) -> Result<(), RunError> {
    let sm = SourceMap::new(source);

    let lexer = Lexer::new(source);
    let tokens = lexer
        .tokenize()
        .map_err(|e| RunError::with_context(&sm, e.line, e.col, &e.message))?;

    let parser = Parser::new(tokens);
    let stmts = parser
        .parse()
        .map_err(|e| RunError::with_context(&sm, e.line, e.col, &e.message))?;

    match engine {
        Engine::Vm => {
            let program = crate::vm::Compiler::compile(&stmts)
                .map_err(|e| RunError::simple(e.message.clone()))?;
            let mut vm = crate::vm::VM::new(program);
            vm.run().map_err(|e| RunError::simple(e.message))
        }
        Engine::Interpreter => {
            let mut interpreter = crate::interpreter::Interpreter::new();
            interpreter
                .run(&stmts)
                .map_err(|e| RunError::simple(e.message))
        }
    }
}

pub fn run_source_with_context(source: &str) -> Result<(), String> {
    run_source(source).map_err(|e| e.to_string())
}

pub fn run_file(path: &str) -> Result<(), String> {
    run_file_with_engine(path, Engine::Vm)
}

pub fn run_file_with_engine(path: &str, engine: Engine) -> Result<(), String> {
    let source = read_source(path)?;
    run_source_with_engine(&source, engine).map_err(|e| e.to_string())
}

pub fn run_stdin_with_engine(engine: Engine) -> Result<(), String> {
    let source = read_source("-")?;
    run_source_with_engine(&source, engine).map_err(|e| e.to_string())
}

pub fn check_file(path: &str) -> Result<(), String> {
    let source = read_source(path)?;
    let sm = SourceMap::new(&source);

    let lexer = Lexer::new(&source);
    let tokens = lexer.tokenize().map_err(|e| sm.format(e.line, e.col, &e.message))?;

    let parser = Parser::new(tokens);
    parser.parse().map_err(|e| sm.format(e.line, e.col, &e.message))?;

    println!("checked {} - no errors found", path);
    Ok(())
}

/// Compiles a source file and renders its bytecode, annotated with the type
/// inference results that decide native compilation.
pub fn disassemble_file(path: &str) -> Result<String, String> {
    let source = read_source(path)?;
    let sm = SourceMap::new(&source);

    let lexer = Lexer::new(&source);
    let tokens = lexer.tokenize().map_err(|e| sm.format(e.line, e.col, &e.message))?;

    let parser = Parser::new(tokens);
    let stmts = parser.parse().map_err(|e| sm.format(e.line, e.col, &e.message))?;

    let program = crate::vm::Compiler::compile(&stmts).map_err(|e| format!("error: {}", e.message))?;
    let analysis = crate::jit::analyze(&program);
    let mut out = program.disassemble();
    out.push_str("\n=== inferred types / native compilation ===\n");
    out.push_str(&analysis.describe(&program));
    Ok(out)
}

/// Compiles a source file all the way to C, without running a C compiler.
///
/// Used by `nect build --emit-c` and by the test suite, which checks the
/// generated program against the VM's output.
pub fn compile_to_c(path: &str) -> Result<String, String> {
    let source = read_source(path)?;
    compile_source_to_c(&source)
}

pub fn compile_source_to_c(source: &str) -> Result<String, String> {
    let sm = SourceMap::new(source);
    let tokens = Lexer::new(source)
        .tokenize()
        .map_err(|e| sm.format(e.line, e.col, &e.message))?;
    let stmts = Parser::new(tokens)
        .parse()
        .map_err(|e| sm.format(e.line, e.col, &e.message))?;
    let program =
        crate::vm::Compiler::compile(&stmts).map_err(|e| format!("error: {}", e.message))?;
    crate::aot::emit_c(&program)
}

/// Options for `nect build`.
#[derive(Debug, Default, Clone)]
pub struct BuildOptions {
    /// Where to write the executable. Defaults to the source's file stem.
    pub output: Option<String>,
    /// The C compiler to invoke. Defaults to `$CC`, then `cc`.
    pub cc: Option<String>,
    /// Target triple for cross-compilation (e.g., x86_64-unknown-linux-gnu, aarch64-apple-darwin).
    /// Passed to Clang as --target. For GCC, use a cross-compiler binary via --cc.
    pub target: Option<String>,
    /// Sysroot for cross-compilation (path to target system root with headers/libs).
    pub sysroot: Option<String>,
    /// Keep the generated C next to the executable.
    pub keep_c: bool,
}

/// Compiles a source file into a standalone native executable.
///
/// Returns the path that was written.
pub fn build_file(path: &str, options: &BuildOptions) -> Result<String, String> {
    let c_source = compile_to_c(path)?;

    let stem = std::path::Path::new(path)
        .file_stem()
        .map(|stem| stem.to_string_lossy().to_string())
        .unwrap_or_else(|| "a.out".to_string());
    let mut output = options.output.clone().unwrap_or_else(|| stem.clone());

    // Determine target OS for platform-specific handling
    let target_os = detect_target_os(&options.target);

    // Add .exe extension on Windows
    if target_os == "windows" && !output.ends_with(".exe") {
        output.push_str(".exe");
    }

    let c_path = if options.keep_c {
        std::path::PathBuf::from(format!("{}.c", output))
    } else {
        std::env::temp_dir().join(format!("nect-{}-{}.c", std::process::id(), stem))
    };
    fs::write(&c_path, &c_source)
        .map_err(|e| format!("cannot write '{}': {}", c_path.display(), e))?;

    let cc = options
        .cc
        .clone()
        .or_else(|| std::env::var("CC").ok())
        .unwrap_or_else(|| "cc".to_string());

    let mut cmd = std::process::Command::new(&cc);
    cmd.arg("-O2")
        .arg("-ffp-contract=off")
        .arg("-o")
        .arg(&output)
        .arg(&c_path);

    // Platform-specific linker flags
    if target_os != "windows" {
        // On macOS, math functions are in libSystem (linked by default)
        // On Linux/Unix, we need -lm
        if target_os != "macos" {
            cmd.arg("-lm");
        }
    }

    // Add target flag for cross-compilation (Clang-style)
    if let Some(ref target) = options.target {
        cmd.arg(format!("--target={}", target));
    }
    // Add sysroot flag for cross-compilation
    if let Some(ref sysroot) = options.sysroot {
        cmd.arg(format!("--sysroot={}", sysroot));
    }

    let run = cmd.output();

    let result = match run {
        Ok(result) => result,
        Err(e) => {
            if !options.keep_c {
                let _ = fs::remove_file(&c_path);
            }
            return Err(format!(
                "cannot run the C compiler '{}': {} (set CC or pass --cc)",
                cc, e
            ));
        }
    };

    if !result.status.success() {
        if !options.keep_c {
            let _ = fs::remove_file(&c_path);
        }
        let message = String::from_utf8_lossy(&result.stderr);
        return Err(format!("the C compiler reported an error:\n{}", message.trim_end()));
    }

    if !options.keep_c {
        let _ = fs::remove_file(&c_path);
    }
    Ok(output)
}

/// Detect target OS from the target triple, defaulting to the host OS.
fn detect_target_os(target_triple: &Option<String>) -> String {
    // If target is specified, parse it
    if let Some(target) = target_triple {
        if target.contains("windows") {
            return "windows".to_string();
        }
        if target.contains("darwin") || target.contains("macos") || target.contains("ios") {
            return "macos".to_string();
        }
        if target.contains("linux") || target.contains("gnu") || target.contains("musl") {
            return "linux".to_string();
        }
    }
    // Default to host OS
    if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "linux"
    }
    .to_string()
}

#[derive(Debug)]
pub struct RunError {
    formatted: String,
}

impl RunError {
    fn with_context(sm: &SourceMap, line: usize, col: usize, msg: &str) -> Self {
        Self {
            formatted: sm.format(line, col, msg),
        }
    }

    fn simple(msg: String) -> Self {
        Self {
            formatted: format!("error: {}", msg),
        }
    }
}

impl std::fmt::Display for RunError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.formatted)
    }
}

impl std::error::Error for RunError {}

pub fn print_version() {
    println!("nect 0.1.0");
}

pub fn print_help() {
    println!("Nect Programming Language v0.1.0");
    println!();
    println!("USAGE:");
    println!("    nect <COMMAND> [ARGS]");
    println!();
    println!("COMMANDS:");
    println!("    run [--interp] <file> [args...]   Run a .nct source file (--interp uses the");
    println!("                            tree-walking interpreter instead of the VM)");
    println!("    run -                   Run source from stdin");
    println!("    check <file>            Check syntax without running");
    println!("    check -                 Check source from stdin");
    println!("    disasm <file>           Print bytecode and inferred types");
    println!("    build <file>            Compile to a standalone native executable");
    println!("        -o <path>             Where to write the executable");
    println!("        --cc <compiler>       C compiler to use (default: $CC, then cc)");
    println!("        --target <triple>     Target triple for cross-compilation (e.g., x86_64-unknown-linux-gnu)");
    println!("        --sysroot <path>      Sysroot for cross-compilation (target system headers/libs)");
    println!("        --emit-c              Print the generated C instead of building");
    println!("        --keep-c              Keep the generated C next to the executable");
    println!("    pkg init [name] [version]   Create a new package (default: current dir name, 0.1.0)");
    println!("    pkg add <name> [version]    Add a dependency (default version: *)");
    println!("    pkg add --dev <name> [version]  Add a dev dependency");
    println!("    pkg remove <name>         Remove a dependency");
    println!("    pkg install               Install dependencies from nect.lock");
    println!("    pkg build                 Build the package");
    println!("    pkg publish               Publish package to registry");
    println!("    pkg run <script>          Run a script from nect.toml");
    println!("    pkg update                Update dependencies to latest versions");
    println!("    pkg list                  List installed dependencies");
    println!("    pkg outdated              Check for outdated dependencies");
    println!("    lsp                       Start the Language Server Protocol server");
    println!("    fmt [file]                Format a .nct source file (stdin if omitted)");
    println!("        --check               Check if file is formatted (exit code 1 if not)");
    println!("        --indent <n>          Indent size (default: 4)");
    println!("        --trailing-comma      Add trailing commas in arrays/objects");
    println!("        --single-quote        Use single quotes for strings");
    println!("        --brace-style <style> Brace style: same-line (default) or next-line");
    println!("    lint [file]               Lint a .nct source file (stdin if omitted)");
    println!("        --no-unused           Disable unused variable check");
    println!("        --no-shadowing        Disable shadowing check");
    println!("        --no-dead-code        Disable dead code check");
    println!("        --no-unused-params    Disable unused parameter check");
    println!("    debug [file]              Debug a .nct source file interactively");
    println!("    --version               Print version");
    println!("    --help                  Print this help message");
    println!();
    println!("ENVIRONMENT:");
    println!("    NECT_NO_JIT=1          Disable native compilation (bytecode only)");
    println!("    CC                      C compiler used by `nect build`");
}

pub fn execute(args: &[String]) -> i32 {
    if args.is_empty() {
        print_help();
        return 0;
    }
    match args[0].as_str() {
        "run" => {
            let mut engine = Engine::Vm;
            let mut positional: Vec<String> = Vec::new();
            for arg in args.iter().skip(1) {
                match arg.as_str() {
                    "--interp" | "--interpreter" => engine = Engine::Interpreter,
                    other => positional.push(other.to_string()),
                }
            }
            let Some(file) = positional.first().cloned() else {
                eprintln!("usage: nect run [--interp] <file> [args...]");
                return 1;
            };
            crate::builtins::set_script_args(positional[1..].to_vec());
            let result = if file == "-" {
                run_stdin_with_engine(engine)
            } else {
                run_file_with_engine(&file, engine)
            };
            match result {
                Ok(()) => 0,
                Err(e) => {
                    eprintln!("{}", e);
                    1
                }
            }
        }
        "check" => {
            if args.len() < 2 {
                eprintln!("usage: nect check <file>");
                return 1;
            }
            match check_file(&args[1]) {
                Ok(()) => 0,
                Err(e) => {
                    eprintln!("{}", e);
                    1
                }
            }
        }
        "disasm" => {
            if args.len() < 2 {
                eprintln!("usage: nect disasm <file>");
                return 1;
            }
            match disassemble_file(&args[1]) {
                Ok(text) => {
                    print!("{}", text);
                    0
                }
                Err(e) => {
                    eprintln!("{}", e);
                    1
                }
            }
        }
        "build" => {
            let mut options = BuildOptions::default();
            let mut file: Option<String> = None;
            let mut emit_c = false;
            let mut args = args.iter().skip(1);
            while let Some(arg) = args.next() {
                match arg.as_str() {
                    "-o" | "--output" => match args.next() {
                        Some(path) => options.output = Some(path.to_string()),
                        None => {
                            eprintln!("usage: nect build <file> -o <path>");
                            return 1;
                        }
                    },
                    "--cc" => match args.next() {
                        Some(cc) => options.cc = Some(cc.to_string()),
                        None => {
                            eprintln!("usage: nect build <file> --cc <compiler>");
                            return 1;
                        }
                    },
                    "--target" => match args.next() {
                        Some(target) => options.target = Some(target.to_string()),
                        None => {
                            eprintln!("usage: nect build <file> --target <triple>");
                            return 1;
                        }
                    },
                    "--sysroot" => match args.next() {
                        Some(sysroot) => options.sysroot = Some(sysroot.to_string()),
                        None => {
                            eprintln!("usage: nect build <file> --sysroot <path>");
                            return 1;
                        }
                    },
                    "--emit-c" => emit_c = true,
                    "--keep-c" => options.keep_c = true,
                    other => file = Some(other.to_string()),
                }
            }
            let Some(file) = file else {
                eprintln!("usage: nect build <file> [-o <path>] [--emit-c]");
                return 1;
            };
            if emit_c {
                return match compile_to_c(&file) {
                    Ok(text) => {
                        print!("{}", text);
                        0
                    }
                    Err(e) => {
                        eprintln!("error: {}", e);
                        1
                    }
                };
            }
            match build_file(&file, &options) {
                Ok(output) => {
                    println!("built {}", output);
                    0
                }
                Err(e) => {
                    eprintln!("error: {}", e);
                    1
                }
            }
        }
        "pkg" => {
            return handle_pkg_command(&args[1..]);
        }
        "lsp" => {
            return handle_lsp_command();
        }
        "fmt" => {
            return handle_fmt_command(&args[1..]);
        }
        "lint" => {
            return handle_lint_command(&args[1..]);
        }
        "debug" => {
            return handle_debug_command(&args[1..]);
        }
        "--version" | "-V" => {
            print_version();
            0
        }
        "--help" | "-h" | "help" => {
            print_help();
            0
        }
        other => {
            eprintln!("unknown command '{}'", other);
            eprintln!();
            print_help();
            1
        }
    }
}

fn handle_pkg_command(args: &[String]) -> i32 {
    if args.is_empty() {
        eprintln!("usage: nect pkg <command> [args...]");
        eprintln!("commands: init, add, remove, install, update, build, publish, run, list, outdated");
        return 1;
    }

    let project_dir = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let project_name = project_dir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("my-package")
        .to_string();
    let registry_url = std::env::var("NECT_REGISTRY").ok();
    let pm = crate::package::create_package_manager(project_dir, registry_url);

    match args[0].as_str() {
        "init" => {
            let name = args.get(1).cloned().unwrap_or_else(|| project_name.clone());
            let version = args.get(2).cloned().unwrap_or_else(|| "0.1.0".to_string());
            match pm.init(&name, &version) {
                Ok(()) => {
                    println!("Created package {} v{}", name, version);
                    0
                }
                Err(e) => {
                    eprintln!("error: {}", e);
                    1
                }
            }
        }
        "add" => {
            let mut dev = false;
            let mut name = None;
            let mut version = None;
            
            let mut i = 1;
            while i < args.len() {
                match args[i].as_str() {
                    "--dev" | "-d" => dev = true,
                    other if name.is_none() => name = Some(other.to_string()),
                    other if version.is_none() => version = Some(other.to_string()),
                    _ => {}
                }
                i += 1;
            }
            
            let Some(name) = name else {
                eprintln!("usage: nect pkg add [--dev] <name> [version]");
                return 1;
            };
            let version = version.unwrap_or_else(|| "*".to_string());
            
            match pm.add_dependency(&name, &version, dev) {
                Ok(()) => {
                    println!("Added {} v{} ({})", name, version, if dev { "dev" } else { "prod" });
                    0
                }
                Err(e) => {
                    eprintln!("error: {}", e);
                    1
                }
            }
        }
        "remove" => {
            let Some(name) = args.get(1) else {
                eprintln!("usage: nect pkg remove <name>");
                return 1;
            };
            match pm.remove_dependency(name) {
                Ok(()) => {
                    println!("Removed {}", name);
                    0
                }
                Err(e) => {
                    eprintln!("error: {}", e);
                    1
                }
            }
        }
        "install" => {
            match pm.install() {
                Ok(_) => {
                    println!("Dependencies installed");
                    0
                }
                Err(e) => {
                    eprintln!("error: {}", e);
                    1
                }
            }
        }
        "build" => {
            match pm.build() {
                Ok(()) => 0,
                Err(e) => {
                    eprintln!("error: {}", e);
                    1
                }
            }
        }
        "publish" => {
            match pm.publish() {
                Ok(()) => 0,
                Err(e) => {
                    eprintln!("error: {}", e);
                    1
                }
            }
        }
        "list" => {
            match pm.list_dependencies() {
                Ok(deps) => {
                    for (name, version) in deps {
                        println!("{} v{}", name, version);
                    }
                    0
                }
                Err(e) => {
                    eprintln!("error: {}", e);
                    1
                }
            }
        }
        "outdated" => {
            match pm.outdated() {
                Ok(outdated) => {
                    if outdated.is_empty() {
                        println!("All dependencies up to date");
                    } else {
                        for (name, current, latest) in outdated {
                            println!("{} v{} -> v{}", name, current, latest);
                        }
                    }
                    0
                }
                Err(e) => {
                    eprintln!("error: {}", e);
                    1
                }
            }
        }
        "update" => {
            match pm.update() {
                Ok(()) => {
                    println!("Dependencies updated");
                    0
                }
                Err(e) => {
                    eprintln!("error: {}", e);
                    1
                }
            }
        }
        "run" => {
            if args.len() < 2 {
                eprintln!("usage: nect pkg run <script>");
                return 1;
            }
            match pm.run_script(&args[1]) {
                Ok(()) => 0,
                Err(e) => {
                    eprintln!("error: {}", e);
                    1
                }
            }
        }
        other => {
            eprintln!("unknown pkg command '{}'", other);
            eprintln!("commands: init, add, remove, install, update, build, publish, run, list, outdated");
            1
        }
    }
}

fn handle_lsp_command() -> i32 {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    if let Err(e) = runtime.block_on(crate::lsp::start_lsp_server()) {
        eprintln!("LSP server error: {}", e);
        return 1;
    }
    0
}

fn handle_fmt_command(args: &[String]) -> i32 {
    let mut check = false;
    let mut indent = 4;
    let mut trailing_comma = false;
    let mut single_quote = false;
    let mut brace_style = crate::formatter::BraceStyle::SameLine;
    let mut file = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--check" => check = true,
            "--indent" => {
                if let Some(val) = args.get(i + 1) {
                    if let Ok(n) = val.parse::<usize>() {
                        indent = n;
                    } else {
                        eprintln!("invalid indent value: {}", val);
                        return 1;
                    }
                    i += 1;
                } else {
                    eprintln!("--indent requires a value");
                    return 1;
                }
            }
            "--trailing-comma" => trailing_comma = true,
            "--single-quote" => single_quote = true,
            "--brace-style" => {
                if let Some(val) = args.get(i + 1) {
                    brace_style = match val.as_str() {
                        "same-line" => crate::formatter::BraceStyle::SameLine,
                        "next-line" => crate::formatter::BraceStyle::NextLine,
                        _ => {
                            eprintln!("invalid brace style: {} (use 'same-line' or 'next-line')", val);
                            return 1;
                        }
                    };
                    i += 1;
                } else {
                    eprintln!("--brace-style requires a value");
                    return 1;
                }
            }
            other if !other.starts_with('-') || other == "-" => {
                if file.is_none() {
                    file = Some(other.to_string());
                } else {
                    eprintln!("unexpected argument: {}", other);
                    return 1;
                }
            }
            other => {
                eprintln!("unknown option: {}", other);
                return 1;
            }
        }
        i += 1;
    }

    let config = crate::formatter::FormatConfig {
        indent_size: indent,
        trailing_comma,
        single_quote,
        brace_style,
        ..Default::default()
    };

    let file_path = file.clone();
    let source = if let Some(ref path) = file_path {
        if *path == "-" {
            let mut buf = String::new();
            match std::io::stdin().read_to_string(&mut buf) {
                Ok(_) => buf,
                Err(e) => {
                    eprintln!("error reading stdin: {}", e);
                    return 1;
                }
            }
        } else {
            match std::fs::read_to_string(&path) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("error reading {}: {}", path, e);
                    return 1;
                }
            }
        }
    } else {
        let mut buf = String::new();
        match std::io::stdin().read_to_string(&mut buf) {
            Ok(_) => buf,
            Err(e) => {
                eprintln!("error reading stdin: {}", e);
                return 1;
            }
        }
    };

    let formatted = match crate::formatter::format_source(&source, config) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("format error: {}", e);
            return 1;
        }
    };

    if check {
        if source.trim_end() != formatted.trim_end() {
            eprintln!("file is not formatted");
            return 1;
        }
        return 0;
    }

    if let Some(path) = &file {
        if path == "-" {
            print!("{}", formatted);
        } else if let Err(e) = std::fs::write(path, &formatted) {
            eprintln!("error writing {}: {}", path, e);
            return 1;
        } else {
            println!("formatted {}", path);
        }
    } else {
        print!("{}", formatted);
    }
    0
}

fn handle_lint_command(args: &[String]) -> i32 {
    let mut no_unused = false;
    let mut no_shadowing = false;
    let mut no_dead_code = false;
    let mut no_unused_params = false;
    let mut file = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--no-unused" => no_unused = true,
            "--no-shadowing" => no_shadowing = true,
            "--no-dead-code" => no_dead_code = true,
            "--no-unused-params" => no_unused_params = true,
            other if !other.starts_with('-') || other == "-" => {
                if file.is_none() {
                    file = Some(other.to_string());
                } else {
                    eprintln!("unexpected argument: {}", other);
                    return 1;
                }
            }
            other => {
                eprintln!("unknown option: {}", other);
                return 1;
            }
        }
        i += 1;
    }

    let config = crate::linter::LinterConfig {
        unused_variables: !no_unused,
        shadowing: !no_shadowing,
        dead_code: !no_dead_code,
        unused_parameters: !no_unused_params,
        ..Default::default()
    };

    let file_path = file.clone();
    let source = if let Some(ref path) = file_path {
        if *path == "-" {
            let mut buf = String::new();
            match std::io::stdin().read_to_string(&mut buf) {
                Ok(_) => buf,
                Err(e) => {
                    eprintln!("error reading stdin: {}", e);
                    return 1;
                }
            }
        } else {
            match std::fs::read_to_string(&path) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("error reading {}: {}", path, e);
                    return 1;
                }
            }
        }
    } else {
        let mut buf = String::new();
        match std::io::stdin().read_to_string(&mut buf) {
            Ok(_) => buf,
            Err(e) => {
                eprintln!("error reading stdin: {}", e);
                return 1;
            }
        }
    };

    let lints = match crate::linter::lint_source(&source, config) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("lint error: {}", e);
            return 1;
        }
    };

    let mut has_errors = false;
    for lint in &lints {
        let prefix = match lint.severity {
            crate::linter::Severity::Error => "error",
            crate::linter::Severity::Warning => "warning",
            crate::linter::Severity::Info => "info",
        };
        if matches!(lint.severity, crate::linter::Severity::Error) {
            has_errors = true;
        }
        eprintln!("{}:{}:{}: {}: {} ({})", 
            if file.as_ref().map(|f| f != "-").unwrap_or(false) { 
                file_path.as_ref().unwrap() 
            } else { 
                "<stdin>" 
            },
            lint.line, lint.column, prefix, lint.message, lint.rule);
    }

    if has_errors { 1 } else { 0 }
}

fn handle_debug_command(args: &[String]) -> i32 {
    let file = if args.is_empty() {
        None
    } else if args[0] == "-" {
        Some("-".to_string())
    } else {
        Some(args[0].clone())
    };

    let source = if let Some(path) = file {
        if path == "-" {
            let mut buf = String::new();
            match std::io::stdin().read_to_string(&mut buf) {
                Ok(_) => buf,
                Err(e) => {
                    eprintln!("error reading stdin: {}", e);
                    return 1;
                }
            }
        } else {
            match std::fs::read_to_string(&path) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("error reading {}: {}", path, e);
                    return 1;
                }
            }
        }
    } else {
        eprintln!("usage: nect debug <file>");
        return 1;
    };

    match crate::debugger::debug_source(&source) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("debug error: {}", e);
            1
        }
    }
}
