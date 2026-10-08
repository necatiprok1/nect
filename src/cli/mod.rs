pub mod doc;

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
        let raw =
            fs::read_to_string(path).map_err(|e| format!("cannot read file '{}': {}", path, e))?;
        let base = std::path::Path::new(path).parent().map(|p| p.to_path_buf());
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
                    let canonical = candidate
                        .canonicalize()
                        .map_err(|e| format!("cannot import '{}': {}", path, e))?;
                    if seen.insert(canonical.clone()) {
                        let text = fs::read_to_string(&canonical)
                            .map_err(|e| format!("cannot import '{}': {}", path, e))?;
                        out.push_str(&expand_imports_inner(&text, canonical.parent(), seen)?);
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

/// Runs a source file with runtime statistics collection and prints them
/// to stderr. Used by `nect mem-profile <file>`.
pub fn mem_profile_file(path: &str) -> Result<(), String> {
    let source = read_source(path)?;
    unsafe {
        std::env::set_var("NECT_VM_STATS", "1");
    }
    run_source_with_profiling(&source)
}
pub fn run_source_with_profiling(source: &str) -> Result<(), String> {
    let sm = SourceMap::new(source);

    let lexer = Lexer::new(source);
    let tokens = lexer
        .tokenize()
        .map_err(|e| RunError::with_context(&sm, e.line, e.col, &e.message).to_string())?;

    let parser = Parser::new(tokens);
    let stmts = parser
        .parse()
        .map_err(|e| RunError::with_context(&sm, e.line, e.col, &e.message).to_string())?;

    let program = crate::vm::Compiler::compile(&stmts)
        .map_err(|e| RunError::simple(e.message.clone()).to_string())?;
    let mut vm = crate::vm::VM::new(program);

    match vm.run() {
        Ok(()) => {
            if std::env::var_os("NECT_VM_STATS").is_some() {
                eprintln!("\n=== Runtime Statistics ===");
                eprint!("{}", vm.stats().format());
            }
            Ok(())
        }
        Err(e) => {
            let stack_trace = std::env::var_os("NECT_STACK_TRACE").is_some();
            if stack_trace {
                let trace = vm.stack_trace();
                eprintln!("{}", e);
                eprintln!("\nStack trace:");
                for (i, frame) in trace.iter().enumerate() {
                    eprintln!("  frame {}: {}", i, frame);
                }
            } else {
                eprintln!("{}", e);
            }
            Err(e.to_string())
        }
    }
}

pub fn check_file(path: &str) -> Result<(), String> {
    let source = read_source(path)?;
    let sm = SourceMap::new(&source);

    let lexer = Lexer::new(&source);
    let tokens = lexer
        .tokenize()
        .map_err(|e| sm.format(e.line, e.col, &e.message))?;

    let parser = Parser::new(tokens);
    parser
        .parse()
        .map_err(|e| sm.format(e.line, e.col, &e.message))?;

    println!("checked {} - no errors found", path);
    Ok(())
}

/// Compiles a source file and renders its bytecode, annotated with the type
/// inference results that decide native compilation.
pub fn disassemble_file(path: &str) -> Result<String, String> {
    let source = read_source(path)?;
    let sm = SourceMap::new(&source);

    let lexer = Lexer::new(&source);
    let tokens = lexer
        .tokenize()
        .map_err(|e| sm.format(e.line, e.col, &e.message))?;

    let parser = Parser::new(tokens);
    let stmts = parser
        .parse()
        .map_err(|e| sm.format(e.line, e.col, &e.message))?;

    let program =
        crate::vm::Compiler::compile(&stmts).map_err(|e| format!("error: {}", e.message))?;
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
        return Err(format!(
            "the C compiler reported an error:\n{}",
            message.trim_end()
        ));
    }

    if !options.keep_c {
        let _ = fs::remove_file(&c_path);
    }
    Ok(output)
}

/// Detect target OS from the target triple, defaulting to the host OS.
fn detect_target_os(target_triple: &Option<String>) -> String {
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
    println!(
        "nect {} (rust {})",
        env!("CARGO_PKG_VERSION"),
        env!("CARGO_PKG_RUST_VERSION")
    );
    // The optional built-in groups are what decides whether `gui_window` or
    // `http_get` resolves, so the build has to be able to say which it has.
    let groups = crate::builtins::enabled_groups();
    if groups.is_empty() {
        println!("built-ins: core only");
    } else {
        println!("built-ins: core + {}", groups.join(", "));
    }
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
    println!(
        "        --target <triple>     Target triple for cross-compilation (e.g., x86_64-unknown-linux-gnu)"
    );
    println!(
        "        --sysroot <path>      Sysroot for cross-compilation (target system headers/libs)"
    );
    println!("        --emit-c              Print the generated C instead of building");
    println!("        --keep-c              Keep the generated C next to the executable");
    println!(
        "    pkg init [name] [version]   Create a new package (default: current dir name, 0.1.0)"
    );
    println!("    pkg add <name> [version]    Add a dependency (default version: *)");
    println!("    pkg add --dev <name> [version]  Add a dev dependency");
    println!("    pkg remove <name>         Remove a dependency");
    println!("    pkg install [--offline]   Install dependencies from nect.lock");
    println!("    pkg build                 Build the package");
    println!("    pkg publish               Publish package to registry");
    println!("    pkg run <script>          Run a script from nect.toml");
    println!("    pkg update                Update dependencies to latest versions");
    println!("    pkg list                  List installed dependencies");
    println!("    pkg outdated              Check for outdated dependencies");
    println!("    pkg tree                  Display the dependency tree");
    println!("    pkg audit                 Check for security vulnerabilities");
    println!("    pkg search <query>        Search for packages in the registry");
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
    println!("    mem-profile <file>        Run a file with runtime statistics (prints");
    println!("                             instruction count, call counts, peak stack");
    println!("                             depth, and stack growths to stderr)");
    println!("    test [--filter <name>]    Run test files in tests/ directory");
    println!("    bench [name]              Run benchmarks from benches/ directory");
    println!("    doctor                      Check the toolchain for common issues");
    println!("    new <name>                  Create a new Nect project");
    println!("    clean                      Remove build artifacts (target/, .nect/)");
    println!("    completions <shell>        Generate shell completion script (bash|zsh|fish)");
    println!("    doc [path]                Generate a Markdown API reference from the source");
    println!("    dap [file]                Serve the Debug Adapter Protocol on stdio");
    println!("        --out, -o <dir>      Write <dir>/API.md instead of stdout");
    println!("        --check              Exit 1 if the committed reference is stale");
    println!("    --version               Print version");
    println!("    --help                  Print this help message");
    println!();
    println!("ENVIRONMENT:");
    println!("    NECT_NO_JIT=1          Disable native compilation (bytecode VM only)");
    println!("    NECT_VM_STATS=1        Enable runtime statistics collection (for mem-profile)");
    println!("    NECT_STACK_TRACE=1     Print a call stack trace on runtime error");
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
            let mut stack_trace = false;
            let mut positional: Vec<String> = Vec::new();
            for arg in args.iter().skip(1) {
                match arg.as_str() {
                    "--interp" | "--interpreter" => engine = Engine::Interpreter,
                    "--stack-trace" => stack_trace = true,
                    other => positional.push(other.to_string()),
                }
            }
            let Some(file) = positional.first().cloned() else {
                eprintln!("usage: nect run [--interp] [--stack-trace] <file> [args...]");
                return 1;
            };
            crate::builtins::set_script_args(positional[1..].to_vec());
            let use_stack_trace = stack_trace || std::env::var_os("NECT_STACK_TRACE").is_some();
            let result = if use_stack_trace && engine == Engine::Vm {
                unsafe {
                    std::env::set_var("NECT_STACK_TRACE", "1");
                }
                let source = if file == "-" {
                    read_source("-")
                } else {
                    read_source(&file)
                };
                match source {
                    Ok(s) => run_source_with_profiling(&s),
                    Err(e) => Err(e),
                }
            } else if file == "-" {
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
        "pkg" => handle_pkg_command(&args[1..]),
        "lsp" => handle_lsp_command(),
        "test" => handle_test_command(&args[1..]),
        "bench" => handle_bench_command(&args[1..]),
        "fmt" => handle_fmt_command(&args[1..]),
        "lint" => handle_lint_command(&args[1..]),
        "debug" => handle_debug_command(&args[1..]),
        "mem-profile" => {
            if args.len() < 2 {
                eprintln!("usage: nect mem-profile <file>");
                return 1;
            }
            match mem_profile_file(&args[1]) {
                Ok(()) => 0,
                Err(e) => {
                    eprintln!("{}", e);
                    1
                }
            }
        }
        "doctor" => handle_doctor_command(),
        "new" => handle_new_command(&args[1..]),
        "clean" => handle_clean_command(),
        "completions" => handle_completions_command(&args[1..]),
        "doc" => handle_doc_command(&args[1..]),
        "dap" => handle_dap_command(&args[1..]),
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
    // The package manager is behind the `pkg` feature: manifest parsing, the
    // registry client, and the archive reader are all optional dependencies.
    #[cfg(not(feature = "pkg"))]
    {
        let _ = args;
        eprintln!("`nect pkg` needs the pkg feature (rebuild with --features pkg)");
        1
    }
    #[cfg(feature = "pkg")]
    {
        if args.is_empty() {
            eprintln!("usage: nect pkg <command> [args...]");
            eprintln!(
                "commands: init, add, remove, install, update, build, publish, run, list, outdated, tree, audit, search"
            );
            return 1;
        }

        let project_dir = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        let project_name = project_dir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("my-package")
            .to_string();
        let registry_url = std::env::var("NECT_REGISTRY").ok();
        // The package manager is behind the `pkg` feature.
        #[cfg(not(feature = "pkg"))]
        {
            let _ = (project_dir, registry_url);
            eprintln!("`nect pkg` needs the pkg feature (rebuild with --features pkg)");
            return 1;
        }
        #[cfg(feature = "pkg")]
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
                        println!(
                            "Added {} v{} ({})",
                            name,
                            version,
                            if dev { "dev" } else { "prod" }
                        );
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
                let mut offline = false;
                for arg in args.iter().skip(1) {
                    if arg.as_str() == "--offline" {
                        offline = true
                    }
                }
                if offline {
                    match pm.install_offline() {
                        Ok(true) => {
                            println!("Dependencies available from cache (offline)");
                            0
                        }
                        Ok(false) => {
                            eprintln!(
                                "error: not all dependencies are available in cache (offline mode)"
                            );
                            1
                        }
                        Err(e) => {
                            eprintln!("error: {}", e);
                            1
                        }
                    }
                } else {
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
            }
            "build" => match pm.build() {
                Ok(()) => 0,
                Err(e) => {
                    eprintln!("error: {}", e);
                    1
                }
            },
            "publish" => match pm.publish() {
                Ok(()) => 0,
                Err(e) => {
                    eprintln!("error: {}", e);
                    1
                }
            },
            "list" => match pm.list_dependencies() {
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
            },
            "outdated" => match pm.outdated() {
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
            },
            "update" => match pm.update() {
                Ok(()) => {
                    println!("Dependencies updated");
                    0
                }
                Err(e) => {
                    eprintln!("error: {}", e);
                    1
                }
            },
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
            "tree" => match pm.dependency_tree() {
                Ok(tree) => {
                    tree.print();
                    0
                }
                Err(e) => {
                    eprintln!("error: {}", e);
                    1
                }
            },
            "audit" => match pm.audit() {
                Ok(result) => {
                    if result.has_vulnerabilities() {
                        println!("Found {} vulnerabilities:", result.total);
                        for vuln in &result.vulnerabilities {
                            println!(
                                "  {} [{}] {} affects {} {} (range: {})",
                                vuln.title,
                                vuln.severity,
                                vuln.advisory_id,
                                vuln.package_name,
                                vuln.installed_version,
                                vuln.version_range
                            );
                            if let Some(url) = &vuln.url {
                                println!("    {}", url);
                            }
                        }
                        1
                    } else {
                        println!("No vulnerabilities found");
                        0
                    }
                }
                Err(e) => {
                    eprintln!("error: {}", e);
                    1
                }
            },
            "search" => {
                if args.len() < 2 {
                    eprintln!("usage: nect pkg search <query>");
                    return 1;
                }
                match pm.search_packages(&args[1]) {
                    Ok(results) => {
                        if results.is_empty() {
                            println!("No packages found matching '{}'", args[1]);
                        } else {
                            println!("Found {} package(s):", results.len());
                            for pkg in &results {
                                println!(
                                    "  {} v{} ({} downloads)",
                                    pkg.name, pkg.version, pkg.downloads
                                );
                                if !pkg.description.is_empty() {
                                    println!("    {}", pkg.description);
                                }
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
            other => {
                eprintln!("unknown pkg command '{}'", other);
                eprintln!(
                    "commands: init, add, remove, install, update, build, publish, run, list, outdated, tree, audit, search"
                );
                1
            }
        }
    }
}

fn handle_lsp_command() -> i32 {
    // `tower-lsp` and the async runtime are behind the `lsp` feature, so a build
    // without it says how to get one instead of failing on an unknown command.
    #[cfg(not(feature = "lsp"))]
    {
        eprintln!("the `lsp` command needs the lsp feature (rebuild with --features lsp)");
        1
    }
    #[cfg(feature = "lsp")]
    {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        if let Err(e) = runtime.block_on(crate::lsp::start_lsp_server()) {
            eprintln!("LSP server error: {}", e);
            return 1;
        }
        0
    }
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
                            eprintln!(
                                "invalid brace style: {} (use 'same-line' or 'next-line')",
                                val
                            );
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
            match std::fs::read_to_string(path) {
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
            match std::fs::read_to_string(path) {
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
        eprintln!(
            "{}:{}:{}: {}: {} ({})",
            if file.as_ref().map(|f| f != "-").unwrap_or(false) {
                file_path.as_ref().unwrap()
            } else {
                "<stdin>"
            },
            lint.line,
            lint.column,
            prefix,
            lint.message,
            lint.rule
        );
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

fn handle_doctor_command() -> i32 {
    println!("Nect Doctor");
    println!("===========");
    println!();

    let mut all_ok = true;

    println!(
        "Platform: {} {}",
        if cfg!(target_os = "linux") {
            "Linux"
        } else if cfg!(target_os = "macos") {
            "macOS"
        } else if cfg!(target_os = "windows") {
            "Windows"
        } else {
            "Unknown"
        },
        if cfg!(target_arch = "x86_64") {
            "x64"
        } else if cfg!(target_arch = "aarch64") {
            "ARM64"
        } else {
            "other"
        }
    );
    println!();

    println!("Toolchain:");
    println!("  nect compiler: v{}", env!("CARGO_PKG_VERSION"));
    println!("  Rust compiler: {}", {
        let output = std::process::Command::new("rustc")
            .arg("--version")
            .output();
        match output {
            Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
            _ => "not found".to_string(),
        }
    });
    println!();

    println!("C compiler (for `nect build`):");
    let cc = std::env::var("CC")
        .or_else(|_| std::env::var("cc"))
        .unwrap_or_else(|_| "cc".to_string());
    let cc_path = if cc == "cc" { "cc" } else { &cc };
    match std::process::Command::new(cc_path)
        .arg("--version")
        .output()
    {
        Ok(output) if output.status.success() => {
            let version = String::from_utf8_lossy(&output.stdout);
            let first_line = version.lines().next().unwrap_or("unknown");
            println!("  {}: {}", cc, first_line);
        }
        _ => {
            println!("  {}: not found", cc);
            println!("  Warning: C compiler not found; `nect build` will not work.");
            all_ok = false;
        }
    }
    println!();

    println!("JIT (Cranelift) status:");
    // The JIT is always compiled in — it is not behind a cargo feature. What
    // varies is whether *this* process will use it: `NECT_NO_JIT=1` is a runtime
    // switch, and a program with nothing worth compiling never initialises
    // Cranelift at all. Reporting a build-time flag here would claim the JIT is
    // absent when it is present and merely switched off.
    println!("  Cranelift JIT: compiled in");
    if std::env::var_os("NECT_NO_JIT").is_some() {
        println!("  current run: disabled by NECT_NO_JIT (bytecode VM only)");
    } else {
        println!("  current run: enabled");
    }
    println!();

    // Which optional built-in groups this binary has decides whether `gui_show`
    // or `http_get` resolves, so it belongs in a diagnostic report: the symptom
    // of a lean build meeting a GUI program is an error naming the name, and
    // this is where the user looks first.
    println!("Optional features:");
    let groups = crate::builtins::enabled_groups();
    if groups.is_empty() {
        println!("  core built-ins only, no optional groups");
    } else {
        println!("  core built-ins plus: {}", groups.join(", "));
    }
    for (feature, names) in [
        ("net", crate::builtins::NET_NAMES),
        ("server", crate::builtins::SERVER_NAMES),
        ("db", crate::builtins::DB_NAMES),
        ("gui", crate::builtins::GUI_NAMES),
    ] {
        let state = if crate::builtins::has_feature(feature) {
            "available"
        } else {
            "not in this build"
        };
        println!("  {feature:<7} {state}: {}", names.join(", "));
    }
    if groups.is_empty() {
        println!("  Rebuild from source for more: cargo install --path . --features full");
    }
    println!();

    println!(
        "Home directory: {}",
        std::env::var("HOME").unwrap_or_else(|_| "not set".to_string())
    );
    println!();

    if all_ok {
        println!("Nect toolchain is ready.");
    } else {
        println!("Some checks failed. Review the output above.");
    }
    0
}

fn handle_new_command(args: &[String]) -> i32 {
    let name = args.first().cloned().unwrap_or_else(|| {
        std::env::current_dir()
            .ok()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
            .unwrap_or_else(|| "my-project".to_string())
    });

    let project_dir = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let project_path = project_dir.join(&name);

    if project_path.exists() {
        eprintln!("error: directory '{}' already exists", name);
        return 1;
    }

    std::fs::create_dir_all(project_path.join("src"))
        .map_err(|e| {
            eprintln!("error: failed to create project directory: {}", e);
            1
        })
        .unwrap();

    let manifest = format!(
        "[package]
name = \"{}\"
version = \"0.1.0\"
description = \"\"
authors = []
license = \"\"
repository = \"\"
homepage = \"\"
keywords = []
categories = []

[dependencies]
",
        name
    );

    std::fs::write(project_path.join("nect.toml"), manifest)
        .map_err(|e| {
            eprintln!("error: failed to write nect.toml: {}", e);
            1
        })
        .unwrap();

    std::fs::write(
        project_path.join("nect.lock"),
        "# Nect lockfile - will be generated by `nect pkg install`\n",
    )
    .map_err(|e| {
        eprintln!("error: failed to write nect.lock: {}", e);
        1
    })
    .unwrap();

    let main_nct = format!("print(\"Hello, {}!\")", name);
    std::fs::write(project_path.join("src").join("main.nct"), main_nct)
        .map_err(|e| {
            eprintln!("error: failed to write src/main.nct: {}", e);
            1
        })
        .unwrap();

    let gitignore_content = "target/\n.nect/\nnect.lock\n";
    std::fs::write(project_path.join(".gitignore"), gitignore_content)
        .map_err(|e| {
            eprintln!("error: failed to write .gitignore: {}", e);
            1
        })
        .unwrap();

    println!("Created project '{}' in {}", name, project_path.display());
    println!();
    println!("To get started:");
    println!("  cd {}", name);
    println!("  nect run src/main.nct");
    0
}

fn handle_clean_command() -> i32 {
    let project_dir = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let target_dir = project_dir.join("target");
    let nect_cache = project_dir.join(".nect");

    let mut cleaned = false;

    if target_dir.exists() {
        match std::fs::remove_dir_all(&target_dir) {
            Ok(()) => {
                println!("Removed target/");
                cleaned = true;
            }
            Err(e) => {
                eprintln!("warning: failed to remove target/: {}", e);
            }
        }
    }

    if nect_cache.exists() {
        match std::fs::remove_dir_all(&nect_cache) {
            Ok(()) => {
                println!("Removed .nect/");
                cleaned = true;
            }
            Err(e) => {
                eprintln!("warning: failed to remove .nect/: {}", e);
            }
        }
    }

    if cleaned {
        println!("Cleaned build artifacts.");
    } else {
        println!("Nothing to clean.");
    }
    0
}

fn handle_test_command(args: &[String]) -> i32 {
    let mut filter = String::new();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--filter" | "-f" => {
                if let Some(val) = args.get(i + 1) {
                    filter = val.clone();
                    i += 1;
                } else {
                    eprintln!("error: {} requires a value", args[i]);
                    return 1;
                }
            }
            other if other.starts_with('-') => {
                eprintln!("unknown option: {}", other);
                return 1;
            }
            _ => {}
        }
        i += 1;
    }

    let project_dir = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let tests_dir = project_dir.join("tests");

    if !tests_dir.exists() {
        eprintln!("error: no tests/ directory found");
        return 1;
    }

    let mut passed = 0;
    let mut failed = 0;
    let mut test_files: Vec<std::path::PathBuf> = Vec::new();

    if let Ok(entries) = std::fs::read_dir(&tests_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "nct")
                && (filter.is_empty()
                    || path
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .map(|s| s.contains(&filter))
                        .unwrap_or(false))
            {
                test_files.push(path);
            }
        }
    }

    if test_files.is_empty() {
        println!("No test files found.");
        return 0;
    }

    test_files.sort();

    for test_file in &test_files {
        let name = test_file.file_stem().unwrap().to_string_lossy().to_string();
        print!("running test: {} ... ", name);
        std::io::Write::flush(&mut std::io::stdout()).ok();

        let path_str = test_file.to_string_lossy().to_string();
        match run_file(&path_str) {
            Ok(()) => {
                println!("PASSED");
                passed += 1;
            }
            Err(e) => {
                println!("FAILED");
                eprintln!("  {}", e);
                failed += 1;
            }
        }
    }

    println!();
    println!("test result: {} passed, {} failed", passed, failed);

    if failed > 0 { 1 } else { 0 }
}

fn handle_bench_command(args: &[String]) -> i32 {
    let mut bench_name = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            other if other.starts_with('-') => {
                eprintln!("unknown option: {}", other);
                return 1;
            }
            other => {
                if bench_name.is_none() {
                    bench_name = Some(other.to_string());
                }
            }
        }
        i += 1;
    }

    let project_dir = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let benches_dir = project_dir.join("benches");

    if !benches_dir.exists() {
        eprintln!("error: no benches/ directory found");
        return 1;
    }

    if let Some(name) = bench_name {
        let bench_file = benches_dir.join(format!("{}.nct", name));
        if !bench_file.exists() {
            eprintln!("error: benchmark '{}' not found", name);
            return 1;
        }
        println!("Running benchmark: {}", name);
        return match run_file(bench_file.to_string_lossy().as_ref()) {
            Ok(()) => 0,
            Err(e) => {
                eprintln!("error: {}", e);
                1
            }
        };
    }

    let mut bench_files: Vec<std::path::PathBuf> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&benches_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "nct") {
                bench_files.push(path);
            }
        }
    }

    if bench_files.is_empty() {
        println!("No benchmark files found in benches/");
        return 0;
    }

    bench_files.sort();
    for bench_file in &bench_files {
        let name = bench_file
            .file_stem()
            .unwrap()
            .to_string_lossy()
            .to_string();
        println!("Running benchmark: {}", name);
        match run_file(bench_file.to_string_lossy().as_ref()) {
            Ok(()) => {}
            Err(e) => {
                eprintln!("  error: {}", e);
            }
        }
        println!();
    }

    0
}

/// `nect dap [file]` — serve the Debug Adapter Protocol on stdio.
///
/// An editor launches this instead of `nect debug` when it wants to drive the
/// debugger itself: the protocol is JSON-RPC over stdio, and stdout belongs to
/// the client, so every diagnostic the program produces goes to stderr.
fn handle_dap_command(args: &[String]) -> i32 {
    let mut file: Option<String> = None;
    for arg in args {
        match arg.as_str() {
            "--help" | "-h" => {
                println!("usage: nect dap <file>");
                println!();
                println!("    Serves the Debug Adapter Protocol on stdio for an editor");
                println!("    to drive. The protocol arrives on stdin, so the program has");
                println!("    to come from a file or from the `launch` request's");
                println!("    `program` argument — it cannot share stdin with the");
                println!("    protocol.");
                return 0;
            }
            other if other.starts_with('-') => {
                eprintln!("error: unknown option '{}' for `nect dap`", other);
                return 1;
            }
            other => file = Some(other.to_string()),
        }
    }

    // With no file the program is supplied by the `launch` request instead.
    let program = match file {
        Some(path) => match read_source(&path) {
            Ok(source) => Some(source),
            Err(e) => {
                eprintln!("error: {}", e);
                return 1;
            }
        },
        None => None,
    };

    match crate::debugger::dap::serve(program) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("error: {}", e);
            1
        }
    }
}

/// `nect doc` — generate (or verify) a Markdown API reference for a project.
fn handle_doc_command(args: &[String]) -> i32 {
    let mut options = doc::DocOptions::default();
    let mut root: Option<String> = None;

    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--out" | "-o" => {
                index += 1;
                match args.get(index) {
                    Some(path) => options.out_dir = Some(std::path::PathBuf::from(path)),
                    None => {
                        eprintln!("error: --out needs a directory");
                        return 1;
                    }
                }
            }
            "--check" => options.check = true,
            "--help" | "-h" => {
                println!("usage: nect doc [file-or-dir] [--out <dir>] [--check]");
                println!();
                println!("    Generates a Markdown API reference from the project's own");
                println!("    source: the functions each file defines, its module-level");
                println!("    values, and the built-ins it calls.");
                println!();
                println!("        --out, -o <dir>   Write <dir>/API.md instead of stdout");
                println!("        --check          Exit 1 if the committed reference is stale");
                return 0;
            }
            other if other.starts_with('-') => {
                eprintln!("error: unknown option '{}' for `nect doc`", other);
                eprintln!("try `nect doc --help`");
                return 1;
            }
            other => root = Some(other.to_string()),
        }
        index += 1;
    }

    // With no path, document the current directory: that is what a developer
    // standing in a project means.
    let target = std::path::PathBuf::from(root.unwrap_or_else(|| ".".to_string()));
    let report = match doc::generate(&target, &options) {
        Ok(report) => report,
        Err(e) => {
            eprintln!("error: {}", e);
            return 1;
        }
    };

    if let Some(stale) = report.stale {
        eprintln!("error: {}", stale);
        return 1;
    }

    match &options.out_dir {
        // With no destination the reference is the output, so it goes to stdout.
        None => print!("{}", report.markdown),
        // `--check` has already reported staleness above and should not also
        // claim to have written a file.
        Some(_) if options.check => {}
        Some(directory) => println!(
            "wrote {} ({} file(s) analyzed)",
            directory.join("API.md").display(),
            report.files.len()
        ),
    }

    // A file that failed to parse is a real problem, so it is reported and
    // reflected in the exit status rather than passed over in silence.
    let broken: Vec<&doc::DocFile> = report
        .files
        .iter()
        .filter(|f| !f.problems.is_empty())
        .collect();
    if !broken.is_empty() {
        for file in broken {
            for problem in &file.problems {
                eprintln!("warning: {}", problem);
            }
        }
        return 1;
    }

    0
}

/// The completion script for `shell`, or `None` when the shell is unsupported.
///
/// Exposed so tests can assert the scripts stay in step with the command table
/// instead of only checking that the command exits successfully.
pub fn completions_script(shell: &str) -> Option<&'static str> {
    match shell {
        "bash" => Some(BASH_COMPLETION),
        "zsh" => Some(ZSH_COMPLETION),
        "fish" => Some(FISH_COMPLETION),
        _ => None,
    }
}

/// The shells `nect completions` can emit.
pub const COMPLETION_SHELLS: [&str; 3] = ["bash", "zsh", "fish"];

fn handle_completions_command(args: &[String]) -> i32 {
    if args.is_empty() {
        eprintln!("usage: nect completions <shell>");
        eprintln!("supported shells: bash, zsh, fish");
        return 1;
    }

    let shell = &args[0];
    let completion = match completions_script(shell) {
        Some(script) => script,
        None => {
            eprintln!("error: unsupported shell '{}'", shell);
            eprintln!("supported shells: bash, zsh, fish");
            return 1;
        }
    };

    println!("{}", completion);
    0
}

const BASH_COMPLETION: &str = r##"
_nect_completions() {
    local cur prev words cword
    _init_completion || return

    local commands="run check disasm build pkg test bench fmt lint debug mem-profile doctor new init clean completions doc lsp"

    if [ $cword -eq 1 ]; then
        COMPREPLY=( $(compgen -W "$commands" -- "$cur") )
    elif [ $cword -eq 2 ]; then
        case "$prev" in
            run|check|disasm|mem-profile|debug)
                if [[ "$cur" == -* ]]; then
                    COMPREPLY=( $(compgen -W "--interp --stack-trace" -- "$cur") )
                else
                    _filedir "*.nct"
                fi
                ;;
            build)
                if [[ "$cur" == -* ]]; then
                    COMPREPLY=( $(compgen -W "-o --cc --target --sysroot --emit-c --keep-c" -- "$cur") )
                else
                    _filedir "*.nct"
                fi
                ;;
            pkg)
                COMPREPLY=( $(compgen -W "init add remove install update build publish run list outdated tree audit search" -- "$cur") )
                ;;
            fmt)
                if [[ "$cur" == -* ]]; then
                    COMPREPLY=( $(compgen -W "--check --indent --trailing-comma --single-quote --brace-style" -- "$cur") )
                else
                    _filedir "*.nct"
                fi
                ;;
            lint)
                if [[ "$cur" == -* ]]; then
                    COMPREPLY=( $(compgen -W "--no-unused --no-shadowing --no-dead-code --no-unused-params" -- "$cur") )
                else
                    _filedir "*.nct"
                fi
                ;;
            new|init)
                COMPREPLY=( $(compgen -f -- "$cur") )
                ;;
            completions)
                COMPREPLY=( $(compgen -W "bash zsh fish" -- "$cur") )
                ;;
            doc)
                if [[ "$cur" == -* ]]; then
                    COMPREPLY=( $(compgen -W "--out -o --check --help" -- "$cur") )
                else
                    _filedir
                fi
                ;;
            test)
                if [[ "$cur" == -* ]]; then
                    COMPREPLY=( $(compgen -W "--filter" -- "$cur") )
                else
                    _filedir "*.nct"
                fi
                ;;
            lsp)
                ;;
        esac
    fi
}

complete -F _nect_completions nect
"##;

const ZSH_COMPLETION: &str = r##"
#compdef nect

_nect() {
    local curcontext="$curcontext" state line
    typeset -A opt_args

    local commands=(
        'run:Run a .nct source file'
        'check:Check syntax without running'
        'disasm:Print bytecode and inferred types'
        'build:Compile to a standalone native executable'
        'pkg:Package manager commands'
        'test:Run test files'
        'bench:Run benchmarks'
        'fmt:Format a .nct source file'
        'lint:Lint a .nct source file'
        'debug:Debug a .nct source file'
        'mem-profile:Run with runtime statistics'
        'doctor:Check toolchain health'
        'new:Create a new Nect project'
        'init:Create nect.toml in an existing directory'
        'clean:Remove build artifacts'
        'completions:Generate shell completion'
        'doc:Generate a Markdown API reference'
        'lsp:Start the language server on stdio'
    )

    _arguments -C \
        '1: :->cmds' \
        '*::arg:->args'

    if [[ $state == cmds ]]; then
        _describe 'nect commands' commands
        return
    fi

    case $line[1] in
        run|check|disasm|mem-profile|debug)
            _arguments \
                '--interp[run on the tree-walking interpreter]' \
                '--stack-trace[print the call stack on a runtime error]' \
                '*.nct' && _files '*.nct'
            ;;
        build)
            _arguments '-o[output]:output file:_files' \
                '--cc[C compiler]:compiler:' \
                '--target[target triple]:triple:' \
                '--sysroot[sysroot]:path:_files -/' \
                '--emit-c' \
                '--keep-c' \
                '*.nct' \
                && _files '*.nct'
            ;;
        pkg)
            _arguments '1:pkg_cmd:(init add remove install update build publish run list outdated tree audit search)'
            ;;
        fmt)
            _arguments '--check' '--indent[indent size]:size:' \
                '--trailing-comma' '--single-quote' \
                '--brace-style[brace style]:style:(same-line next-line)' \
                '*.nct' \
                && _files '*.nct'
            ;;
        lint)
            _arguments '--no-unused' '--no-shadowing' '--no-dead-code' '--no-unused-params' \
                '*.nct' \
                && _files '*.nct'
            ;;
        bench)
            _files '*.nct'
            ;;
        new|init)
            _files -/
            ;;
        test)
            _arguments '--filter[only run matching test files]:name:' \
                '*.nct' && _files '*.nct'
            ;;
        doc)
            _arguments '--out[write the reference to a directory]:dir:_files -/' \
                '-o[write the reference to a directory]:dir:_files -/' \
                '--check[fail if the committed reference is stale]' \
                '*:file:_files'
            ;;
        completions)
            _values 'shell' bash zsh fish
            ;;
    esac
}

_nect "$@"
"##;

const FISH_COMPLETION: &str = r##"
complete -c nect -n '__fish_use_subcommand' -a 'run'      -d 'Run a .nct source file'
complete -c nect -n '__fish_use_subcommand' -a 'check'    -d 'Check syntax without running'
complete -c nect -n '__fish_use_subcommand' -a 'disasm'   -d 'Print bytecode and inferred types'
complete -c nect -n '__fish_use_subcommand' -a 'build'    -d 'Compile to native executable'
complete -c nect -n '__fish_use_subcommand' -a 'pkg'      -d 'Package manager commands'
complete -c nect -n '__fish_use_subcommand' -a 'test'     -d 'Run test files'
complete -c nect -n '__fish_use_subcommand' -a 'bench'    -d 'Run benchmarks'
complete -c nect -n '__fish_use_subcommand' -a 'fmt'      -d 'Format source file'
complete -c nect -n '__fish_use_subcommand' -a 'lint'     -d 'Lint source file'
complete -c nect -n '__fish_use_subcommand' -a 'debug'    -d 'Debug source file'
complete -c nect -n '__fish_use_subcommand' -a 'mem-profile' -d 'Run with runtime statistics'
complete -c nect -n '__fish_use_subcommand' -a 'doctor'   -d 'Check toolchain health'
complete -c nect -n '__fish_use_subcommand' -a 'new'      -d 'Create a new project'
complete -c nect -n '__fish_use_subcommand' -a 'clean'    -d 'Remove build artifacts'
complete -c nect -n '__fish_use_subcommand' -a 'completions' -d 'Generate shell completion'
complete -c nect -n '__fish_use_subcommand' -a 'init'      -d 'Create nect.toml in an existing directory'
complete -c nect -n '__fish_use_subcommand' -a 'doc'       -d 'Generate a Markdown API reference'
complete -c nect -n '__fish_use_subcommand' -a 'lsp'       -d 'Start the language server on stdio'

complete -c nect -n '__fish_seen_subcommand_from run' -a '(test)' -f
complete -c nect -n '__fish_seen_subcommand_from run' -a '(--interp --stack-trace)'
complete -c nect -n '__fish_seen_subcommand_from debug' -a '(--stack-trace)'
complete -c nect -n '__fish_seen_subcommand_from check' -a '(-h --help)'
complete -c nect -n '__fish_seen_subcommand_from disasm' -a '(-h --help)'
complete -c nect -n '__fish_seen_subcommand_from check' -a '(test)' -f
complete -c nect -n '__fish_seen_subcommand_from disasm' -a '(test)' -f
complete -c nect -n '__fish_seen_subcommand_from mem-profile' -a '(test)' -f
complete -c nect -n '__fish_seen_subcommand_from debug' -a '(test)' -f

complete -c nect -n '__fish_seen_subcommand_from fmt' -a '(--check --indent --trailing-comma --single-quote --brace-style)'
complete -c nect -n '__fish_seen_subcommand_from lint' -a '(--no-unused --no-shadowing --no-dead-code --no-unused-params)'

complete -c nect -n '__fish_seen_subcommand_from pkg' -a '(init add remove install update build publish run list outdated tree audit search)'

complete -c nect -n '__fish_seen_subcommand_from completions' -a '(bash zsh fish)'

complete -c nect -n '__fish_seen_subcommand_from test' -a '(--filter)' -d 'Only run matching test files'
complete -c nect -n '__fish_seen_subcommand_from test' -l filter -d 'Only run matching test files' -r

complete -c nect -n '__fish_seen_subcommand_from doc' -l out -s o -d 'Write the reference to a directory' -r
complete -c nect -n '__fish_seen_subcommand_from doc' -l check -d 'Fail if the committed reference is stale'

complete -c nect -n '__fish_seen_subcommand_from new' -a '(-h --help)'
complete -c nect -n '__fish_seen_subcommand_from init' -a '(-h --help)'
complete -c nect -n '__fish_seen_subcommand_from lsp' -a '(-h --help)'

complete -c nect -n '__fish_seen_subcommand_from build' -a '(-o -o --cc --target --sysroot --emit-c --keep-c)'
complete -c nect -n '__fish_seen_subcommand_from doc' -a '(-h --help)'
"##;
