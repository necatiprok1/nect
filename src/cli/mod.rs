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
    let output = options.output.clone().unwrap_or_else(|| stem.clone());

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

    let run = std::process::Command::new(&cc)
        .arg("-O2")
        // Fused multiply-add would round once where the VM rounds twice, which
        // changes iterated floating-point results (mandelbrot escapes at a
        // different iteration). Building without contraction keeps the
        // generated program bit-identical to the VM.
        .arg("-ffp-contract=off")
        .arg("-o")
        .arg(&output)
        .arg(&c_path)
        .arg("-lm")
        .output();

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
    println!("        --emit-c              Print the generated C instead of building");
    println!("        --keep-c              Keep the generated C next to the executable");
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
