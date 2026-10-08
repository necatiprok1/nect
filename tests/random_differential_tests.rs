//! Randomised differential testing.
//!
//! The hand-written cases in `differential_tests.rs` pin the edges someone
//! thought of. This file generates programs instead, so the engines are compared
//! on expressions nobody wrote by hand — which is where a miscompiled fused
//! opcode or a wrong constant-folding decision would actually surface.
//!
//! Generation is driven by a seeded xorshift PRNG rather than the system clock.
//! That matters for a test: a failure has to be reproducible from the seed printed
//! in the assertion message, and a run that passes today must pass tomorrow
//! without recording a new baseline. Re-running with the same seed therefore
//! produces byte-identical programs.
//!
//! Programs are kept small and numeric. That is not a limitation of the
//! generator but the point of it: a numeric program is one the JIT will compile,
//! so the native path is genuinely under test rather than being skipped for
//! calling something the type inference rejects.

use std::io::Write;
use std::process::{Command, Stdio};

/// A seeded xorshift64* generator.
///
/// The language's own `random` is not used here: a test that generated its own
/// inputs with the code under test could agree with a broken engine by
/// construction.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        // A zero state is a fixed point for xorshift, so it is nudged off.
        Rng(seed | 1)
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// A value in `0..bound`. Returns 0 for a bound of 0.
    fn below(&mut self, bound: u64) -> u64 {
        if bound == 0 {
            0
        } else {
            self.next_u64() % bound
        }
    }

    /// Picks an element, or `None` for an empty list.
    fn pick<'a, T>(&mut self, items: &'a [T]) -> Option<&'a T> {
        if items.is_empty() {
            None
        } else {
            items.get(self.below(items.len() as u64) as usize)
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Output {
    stdout: String,
    stderr: String,
    success: bool,
}

fn run(source: &str, args: &[&str], no_jit: bool) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_nect"));
    command.arg("run");
    for arg in args {
        command.arg(arg);
    }
    command.arg("-");
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if no_jit {
        command.env("NECT_NO_JIT", "1");
    }
    let mut child = command.spawn().expect("failed to start the nect binary");
    child
        .stdin
        .as_mut()
        .expect("stdin is piped")
        .write_all(source.as_bytes())
        .expect("failed to write source");
    let output = child.wait_with_output().expect("failed to run nect");
    Output {
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr)
            .trim_end()
            .to_string(),
        success: output.status.success(),
    }
}

fn interpreter(source: &str) -> Output {
    run(source, &["--interp"], false)
}

fn bytecode(source: &str) -> Output {
    run(source, &[], true)
}

fn jit(source: &str) -> Output {
    run(source, &[], false)
}

/// Checks all three engines against each other, naming the seed on failure.
#[track_caller]
fn assert_engines_agree(source: &str, seed: u64) {
    let reference = interpreter(source);
    let vm = bytecode(source);
    let native = jit(source);
    assert_eq!(
        vm, reference,
        "bytecode VM disagrees with the interpreter (seed {seed}) for:\n{source}"
    );
    assert_eq!(
        native, vm,
        "native compilation changed behaviour (seed {seed}) for:\n{source}"
    );
}

/// A number literal that is always a whole number, so printed output is stable
/// and a comparison against a literal in the program is meaningful.
fn number_literal(rng: &mut Rng) -> String {
    match rng.below(4) {
        0 => "0".to_string(),
        1 => "1".to_string(),
        2 => rng.below(200).to_string(),
        _ => {
            // A negative literal, written as a subtraction so the generator never
            // has to know whether the grammar accepts a leading minus.
            format!("0 - {}", rng.below(200))
        }
    }
}

/// Arithmetic operators. `/` is deliberately absent: the JIT's type inference
/// rejects division, so including it would make most generated programs fall back
/// to the bytecode VM and stop exercising the native path. `%` is here instead,
/// which the native path does handle.
const ARITHMETIC_OPS: [&str; 4] = ["+", "-", "*", "%"];

/// The comparison operators, which yield a boolean.
const COMPARISON_OPS: [&str; 4] = ["<", "<=", "==", "!="];

/// Builds an expression that evaluates to a number.
///
/// Generation is type-directed: arithmetic only ever takes numeric operands, so a
/// generated program is one that actually runs rather than a program that
/// happens to fail identically in all three engines. Engine agreement on an error
/// is worth testing too, but `differential_tests.rs` covers the error catalogue
/// deliberately, and a generator that mostly produced type errors would test the
/// error path instead of the arithmetic.
fn numeric_expression(rng: &mut Rng, variables: &[String], depth: u32) -> String {
    if depth == 0 {
        return match rng.pick(variables) {
            Some(name) => name.clone(),
            None => number_literal(rng),
        };
    }
    match rng.below(12) {
        // A nested arithmetic operation, the case most likely to expose a fused
        // three-address opcode reading the wrong slot.
        0..=5 => {
            let op = rng.pick(&ARITHMETIC_OPS).copied().unwrap_or("+");
            let left = numeric_expression(rng, variables, depth - 1);
            // Modulo's right operand is always a non-zero literal. The generator
            // does no value analysis, so any other choice could produce a
            // division by zero — and because a literal cannot itself contain a
            // `%`, this also guarantees a modulo result never becomes a divisor.
            let right = if op == "%" {
                (1 + rng.below(9)).to_string()
            } else {
                numeric_expression(rng, variables, depth - 1)
            };
            format!("{left} {op} {right}")
        }
        // A conditional expression, which puts a branch in the middle of an
        // arithmetic expression and exercises the join both backends have to
        // reconcile.
        6 => {
            let condition = condition(rng, variables, depth - 1);
            let then_branch = numeric_expression(rng, variables, depth - 1);
            let else_branch = numeric_expression(rng, variables, depth - 1);
            format!("({condition} ? {then_branch} : {else_branch})")
        }
        // A call, so a function call sits inside an expression.
        7 => {
            let argument = numeric_expression(rng, variables, depth - 1);
            format!("helper({argument})")
        }
        // A short-circuited conjunction reduced to a number. The language has no
        // boolean-to-number coercion, so `and` cannot stand where a value is
        // expected; the conditional is what turns the test result back into one.
        8 => {
            let left = condition(rng, variables, depth - 1);
            let right = condition(rng, variables, depth - 1);
            format!("(({left} and {right}) ? 1 : 0)")
        }
        // A parenthesised sub-expression, which the parser collapses to a
        // `Grouping` node the backends must agree to ignore.
        9 => {
            let inner = numeric_expression(rng, variables, depth - 1);
            format!("({inner})")
        }
        // Negation, which is a unary opcode with its own fast path.
        10 => {
            let inner = numeric_expression(rng, variables, depth - 1);
            format!("(0 - {inner})")
        }
        _ => match rng.pick(variables) {
            Some(name) => name.clone(),
            None => number_literal(rng),
        },
    }
}

/// Builds an expression that evaluates to a truthy or falsy value.
fn condition(rng: &mut Rng, variables: &[String], depth: u32) -> String {
    if depth == 0 {
        return match rng.pick(variables) {
            Some(name) => format!("({name} > 0)"),
            None => format!("({} > 0)", number_literal(rng)),
        };
    }
    match rng.below(6) {
        0..=3 => {
            let op = rng.pick(&COMPARISON_OPS).copied().unwrap_or("<");
            let left = numeric_expression(rng, variables, depth - 1);
            let right = numeric_expression(rng, variables, depth - 1);
            format!("({left} {op} {right})")
        }
        4 => {
            let left = condition(rng, variables, depth - 1);
            let right = condition(rng, variables, depth - 1);
            format!("({left} or {right})")
        }
        _ => {
            let inner = condition(rng, variables, depth - 1);
            format!("(not {inner})")
        }
    }
}

/// Generates a whole program: a helper, some variables, a loop, and prints.
fn program(rng: &mut Rng) -> String {
    // A helper the expression generator can call, so calls appear in expressions.
    let mut lines: Vec<String> = vec!["fn helper(n) {".to_string()];
    lines.push("    let doubled = n * 2".to_string());
    lines.push("    if doubled > 100 {".to_string());
    lines.push("        return doubled - 100".to_string());
    lines.push("    }".to_string());
    lines.push("    return doubled + 1".to_string());
    lines.push("}".to_string());

    // A handful of module-level bindings to read from.
    let binding_count = 1 + rng.below(4) as usize;
    let mut variables = Vec::new();
    for index in 0..binding_count {
        let name = format!("v{index}");
        lines.push(format!("let {name} = {}", number_literal(rng)));
        variables.push(name);
    }

    // A loop, because the JIT only compiles functions and module prefixes that
    // show evidence of repeated work; without one the native path is barely used.
    let loop_bound = 1 + rng.below(6);
    lines.push("let total = 0".to_string());
    lines.push(format!("let limit = {loop_bound}"));
    lines.push("let i = 0".to_string());
    lines.push("while i < limit {".to_string());
    lines.push(format!(
        "    total = total + {}",
        numeric_expression(rng, &variables, 2)
    ));
    lines.push("    i = i + 1".to_string());
    lines.push("}".to_string());
    lines.push("print(total)".to_string());

    // A few top-level expressions, printed so a wrong value cannot hide.
    let print_count = 1 + rng.below(4);
    for _ in 0..print_count {
        lines.push(format!("print({})", numeric_expression(rng, &variables, 3)));
    }

    let mut source = lines.join("\n");
    source.push('\n');
    source
}

/// How many programs one round generates.
const PROGRAMS_PER_SEED: usize = 12;

/// The seeds the suite runs. Each is a separate `#[test]` so a failure names the
/// one seed that broke rather than "the randomised test".
const SEEDS: [u64; 12] = [
    0x0000_0000_0000_0001,
    0x1234_5678_9abc_def0,
    0xdead_beef_cafe_f00d,
    0x5eed_0000_0000_0001,
    0x0bad_c0de_dead_beef,
    0x0000_0000_0000_ffff,
    0x7fff_ffff_ffff_ffff,
    0x0000_0000_0bad_f00d,
    0xa5a5_5a5a_c3c3_9696,
    0x1111_2222_3333_4444,
    0xfeed_face_cafe_0001,
    0x0f0f_0f0f_f0f0_f0f0,
];

fn generated_programs(seed: u64) -> Vec<String> {
    let mut rng = Rng::new(seed);
    (0..PROGRAMS_PER_SEED).map(|_| program(&mut rng)).collect()
}

macro_rules! seed_test {
    ($name:ident, $seed:expr) => {
        #[test]
        fn $name() {
            for source in generated_programs($seed) {
                assert_engines_agree(&source, $seed);
            }
        }
    };
}

seed_test!(generated_programs_seed_1, SEEDS[0]);
seed_test!(generated_programs_seed_2, SEEDS[1]);
seed_test!(generated_programs_seed_3, SEEDS[2]);
seed_test!(generated_programs_seed_4, SEEDS[3]);
seed_test!(generated_programs_seed_5, SEEDS[4]);
seed_test!(generated_programs_seed_6, SEEDS[5]);
seed_test!(generated_programs_seed_7, SEEDS[6]);
seed_test!(generated_programs_seed_8, SEEDS[7]);
seed_test!(generated_programs_seed_9, SEEDS[8]);
seed_test!(generated_programs_seed_10, SEEDS[9]);
seed_test!(generated_programs_seed_11, SEEDS[10]);
seed_test!(generated_programs_seed_12, SEEDS[11]);

#[test]
fn the_same_seed_generates_the_same_programs() {
    // Without this, a failure could not be reproduced and the seed in the
    // assertion message would be meaningless.
    assert_eq!(generated_programs(SEEDS[2]), generated_programs(SEEDS[2]));
}

#[test]
fn different_seeds_generate_different_programs() {
    // Otherwise the six seed tests would all be checking the same programs.
    assert_ne!(generated_programs(SEEDS[0]), generated_programs(SEEDS[1]));
}

#[test]
fn a_generated_program_actually_runs() {
    // A generator that emitted nothing, or something every engine rejected, would
    // make the seed tests above pass without testing anything.
    for source in generated_programs(SEEDS[0]) {
        let output = bytecode(&source);
        assert!(
            output.success,
            "a generated program should run, got {:?} for:\n{source}",
            output.stderr
        );
        assert!(
            !output.stdout.is_empty(),
            "a generated program should print something:\n{source}"
        );
    }
}

#[test]
fn generated_programs_reach_the_native_path() {
    // The point of generating numeric programs is to exercise native
    // compilation. This checks the premise rather than assuming it.
    let source = generated_programs(SEEDS[0]).remove(0);
    let listing = Command::new(env!("CARGO_BIN_EXE_nect"))
        .args(["disasm", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            child
                .stdin
                .as_mut()
                .expect("stdin")
                .write_all(source.as_bytes())?;
            child.wait_with_output()
        })
        .expect("runs disasm");
    let text = String::from_utf8_lossy(&listing.stdout);
    assert!(
        text.contains("native") || text.contains("JIT"),
        "disasm should report a native-compilation decision:\n{text}"
    );
}
