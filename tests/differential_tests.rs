//! Differential tests.
//!
//! Two implementations of Nect must agree on observable behaviour: the
//! tree-walking interpreter (the reference), the bytecode VM, and the VM with
//! native compilation enabled. Anything that compiles or inlines has a chance to
//! change semantics, so every program below is run through all three and their
//! stdout, stderr, and exit status are compared.
//!
//! Programs that the engines are *known* to handle differently are pinned
//! separately in `documented_divergences`, so the differences stay intentional
//! rather than accidental.

use std::io::Write;
use std::process::{Command, Stdio};

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
        stderr: String::from_utf8_lossy(&output.stderr).trim_end().to_string(),
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

/// Checks all three engines against each other.
#[track_caller]
fn assert_engines_agree(source: &str) {
    let reference = interpreter(source);
    let vm = bytecode(source);
    let native = jit(source);
    assert_eq!(
        vm, reference,
        "bytecode VM disagrees with the interpreter for:\n{source}"
    );
    assert_eq!(
        native, vm,
        "native compilation changed behaviour for:\n{source}"
    );
}

/// Checks the VM against itself with native compilation on and off. Used for
/// programs the interpreter cannot run (it recurses on the host stack).
#[track_caller]
fn assert_vm_variants_agree(source: &str) {
    let vm = bytecode(source);
    let native = jit(source);
    assert_eq!(
        native, vm,
        "native compilation changed behaviour for:\n{source}"
    );
}

const NEWLINE: &str = "\n";

fn program(lines: &[&str]) -> String {
    let mut source = lines.join(NEWLINE);
    source.push('\n');
    source
}

/// The `--- module:` line of `nect disasm` for a file.
fn module_line(file: &str) -> String {
    disasm_file(file)
        .lines()
        .find(|line| line.starts_with("--- module"))
        .unwrap_or_else(|| panic!("no module line for {file}"))
        .to_string()
}

fn disasm_file(file: &str) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_nect"))
        .args(["disasm", file])
        .output()
        .expect("failed to run disasm");
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// The same report for source read from stdin.
fn disasm(source: &str) -> String {
    let mut child = Command::new(env!("CARGO_BIN_EXE_nect"))
        .args(["disasm", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to start the nect binary");
    child
        .stdin
        .as_mut()
        .expect("stdin is piped")
        .write_all(source.as_bytes())
        .expect("failed to write source");
    let output = child.wait_with_output().expect("failed to run disasm");
    String::from_utf8_lossy(&output.stdout).into_owned()
}

const CORPUS: &[&str] = &[
    "print(1 + 2 * 3)",
    "print((1 + 2) * 3)",
    "print(10 / 4)",
    "print(-5)",
    "print(!true)",
    "print(1 == 1.0)",
    "print(1 != 2)",
    "print(true && false)",
    "print(false || true)",
    "print(true && true && false)",
    "print(false || false || true)",
    "print(\"a\" + \"b\")",
    "print(\"a\" < \"b\")",
    "print(\"b\" > \"a\")",
    "print(\"a\" >= \"a\")",
    "print(\"b\" <= \"a\")",
    "print(\"a\" == \"a\")",
    "print(num(\"40\") + 2)",
    "print(str(1 + 2))",
    "print(\"x\" + str(1))",
    "let x = 42\nprint(x)",
    "let x = 1\nlet x = 2\nprint(x)",
    "let x = 1\n{\n    let x = 2\n    print(x)\n}\nprint(x)",
    "let i = 0\nwhile (i < 3) {\n    i = i + 1\n}\nprint(i)",
    "let i = 10\nwhile (i) {\n    i = i - 1\n}\nprint(i)",
    "let i = 1\nwhile (i <= 5) {\n    print(i)\n    i = i + 2\n}",
    "if (1 < 2) {\n    print(\"yes\")\n} else {\n    print(\"no\")\n}",
    "if (0) {\n    print(\"yes\")\n} else {\n    print(\"no\")\n}",
    "let y = x = 5\nprint(x)\nprint(y)",
    "x = 1\ny = 2\nprint(x + y)",
    "fn f(a, b) {\n    return a + b\n}\nprint(f(2, 3))",
    "fn f(a, b) {\n    return a + b\n}\nprint(f(2, f(3, 4)))",
    "fn fib(n) {\n    if (n <= 1) {\n        return n\n    }\n    return fib(n - 1) + fib(n - 2)\n}\nprint(fib(12))",
    "fn fact(n) {\n    let r = 1\n    let i = 1\n    while (i <= n) {\n        r = r * i\n        i = i + 1\n    }\n    return r\n}\nprint(fact(20))",
    "fn sum(n) {\n    let t = 0\n    let i = 0\n    while (i < n) {\n        t = t + i\n        i = i + 1\n    }\n    return t\n}\nprint(sum(1000))",
    "fn nothing() {\n}\nprint(nothing())",
    "fn f() {\n    return\n}\nprint(f())",
    "fn f(x) {\n    let x = x + 1\n    return x\n}\nprint(f(1))",
    "let g = 7\nfn bump() {\n    g = g + 1\n}\nbump()\nbump()\nprint(g)",
    "let n = 3\nlet total = 0\nlet i = 0\nwhile (i < n) {\n    total = total + i\n    i = i + 1\n}\nprint(total)",
    "fn side() {\n    print(\"called\")\n    return true\n}\nprint(false && side())",
    "fn side() {\n    print(\"called\")\n    return true\n}\nprint(true || side())",
    "fn side() {\n    print(\"called\")\n    return true\n}\nprint(true && side())",
    "fn side() {\n    print(\"called\")\n    return true\n}\nprint(false || side())",
    "let s = num(\"nan\")\nprint(s < 1)\nprint(s > 1)\nprint(s <= 1)\nprint(s >= 1)",
    "// a comment\n/* and a block */\nprint(\"ok\")",
    "print(10 / 0)",
    "print(nope)",
    "print(1 + \"s\")",
    "return 5",
    "let a = 1\nprint(a < \"b\")",
    // --- control flow added with the loop keywords -------------------------
    "let i = 0\nwhile (i < 10) {\n    i += 1\n    if (i % 2 == 0) {\n        continue\n    }\n    if (i > 7) {\n        break\n    }\n    print(i)\n}\nprint(i)",
    "for n in [1, 2, 3, 4, 5] {\n    if (n == 3) {\n        continue\n    }\n    if (n == 5) {\n        break\n    }\n    print(n * 10)\n}",
    "let i = 0\nwhile (i < 3) {\n    i += 1\n    for j in [1, 2, 3] {\n        if (j == 2) {\n            continue\n        }\n        if (i == 3) {\n            break\n        }\n        print(i * 10 + j)\n    }\n}",
    "fn first_even(items) {\n    for item in items {\n        if (item % 2 == 0) {\n            return item\n        }\n    }\n    return -1\n}\nprint(first_even([1, 3, 4, 5]))\nprint(first_even([1, 3]))",
    "fn count(n) {\n    let total = 0\n    let i = 0\n    while (i < n) {\n        i += 1\n        if (i % 3 == 0) {\n            continue\n        }\n        if (i > n - 2) {\n            break\n        }\n        total += i\n    }\n    return total\n}\nprint(count(12))",
    "let grade = 87\nif (grade >= 90) {\n    print(\"A\")\n} else if (grade >= 80) {\n    print(\"B\")\n} else if (grade >= 70) {\n    print(\"C\")\n} else {\n    print(\"F\")\n}",
    // --- compound assignment, modulo, conditional expression ----------------
    "let x = 10\nx += 5\nx -= 3\nx *= 2\nx /= 4\nx %= 4\nprint(x)",
    "let s = \"a\"\ns += \"b\"\ns += \"c\"\nprint(s)",
    "let calls = 0\nfn bump() {\n    calls += 1\n    return 1\n}\nlet a = [1, 2, 3]\na[bump()] += 10\na[-1] *= 3\nprint(a)\nprint(calls)",
    "let a = [0, 0]\nprint(a[0] = 5)\nprint(a[0])",
    "print(17 % 5)\nprint(-17 % 5)\nprint(5.5 % 2)",
    "let n = 7\nprint(n % 2 == 0 ? \"even\" : \"odd\")\nprint(n > 5 ? \"big\" : n > 3 ? \"medium\" : \"small\")",
    "fn side(x) {\n    print(\"eval \" + str(x))\n    return x\n}\nprint(true ? side(1) : side(2))\nprint(false ? side(3) : side(4))",
    // --- strings and arrays -------------------------------------------------
    "let s = \"Hello, World\"\nprint(len(s))\nprint(upper(s))\nprint(lower(s))\nprint(trim(\"  pad  \"))\nprint(slice(s, 0, 5))\nprint(slice(s, -5))\nprint(index_of(s, \"World\"))\nprint(index_of(s, \"zz\"))\nprint(contains(s, \"lo,\"))\nprint(replace(s, \"World\", \"Nect\"))\nprint(join(split(s, \", \"), \"-\"))\nprint(repeat(\"ab\", 3))\nprint(reverse(\"abc\"))",
    "let a = []\npush(a, 3, 1, 2)\nprint(a)\nprint(sort(a))\nprint(reverse(a))\nprint(pop(a))\nprint(len(a))\nprint(sum([1, 2, 3, 4]))\nprint(min([4, 2, 9]), max(4, 9, 2))\nprint(type(a), type(1), type(\"s\"), type(true), type(null))\nprint(bool(0), bool(\"\"), bool([]), bool(null), bool(1))",
    "let a = [10, 20, 30]\nprint(a[0], a[-1], a[-3])\nlet s = \"nect\"\nprint(s[0], s[-1], len(s))\nprint(join([1, \"two\"], \"+\"))\nprint([1, \"two\", true, null, [3]])",
    "for i in range(3) {\n    print(i)\n}\nprint(range(2, 5))\nprint(range(5, 0, -2))\nprint(sum(range(1, 101)))\nprint(len(range(0, 10, 2)))",
    // --- new failure modes --------------------------------------------------
    "print([1, 2][-3])",
    "print([1, 2][\"x\"])",
    "let s = \"abc\"\ns[0] = \"z\"",
    "break",
    "fn f() {\n    continue\n}\nf()",
    "for x in 5 {\n    print(x)\n}",
    "for c in \"abc\" {\n    print(c)\n}",
    "print(sort([1, \"a\"]))",
    "print(range(0, 5, 0))",
    "print(range(0, 99999999))",
    "assert(1 > 2, \"boom\")",
    "print(7 % 0)",
    "print(len)",
    "print(split(\"ab\", \"\"))",
    "print(slice(\"hello\", 3, 1))",
    // --- expressions laid out over several lines -----------------------------
    "let values = [\n    3,\n    1,\n    2,\n]\nprint(\n    \"sorted:\",\n    sort(values),\n)",
    "fn add(\n    a,\n    b,\n) {\n    return a + b\n}\nlet total = sum([\n    10,\n    20,\n]) + add(1,\n         2)\nif (\n    total > 30\n) {\n    print(\"big\")\n}\nprint(total)",
    // --- non-ASCII strings (UTF-8 decoding) ---------------------------------
    "let word = \"héllo\"\nprint(len(word))\nprint(word[1])\nprint(reverse(word))\nprint(upper(\"straße\"))",
    "print(len(\"日本語\"), \"日本語\"[1], slice(\"aébc\", 1, 3), index_of(\"naïve\", \"ï\"))",
    "print(\"emoji: 🎯\", len(\"🎯\"), \"🎯\"[0])",
    "let text = \"café ☕\"\nfor ch in split(text, \" \") {\n    print(len(ch), ch)\n}",
    // --- friendly syntax: interpolation, methods, word operators ------------
    "# hash comments work like line comments\nlet name = \"ada\"\nprint(\"hello ${name}!\")  # trailing comment",
    "print(\"${1 + 2}\", \"${[9, 8][0]}\", \"${\"nested ${1}\"}\")",
    "let items = [3, 1, 2]\nprint(items.len())\nprint(items.join(\"-\"))\nprint(\"  pad  \".trim().upper())\nprint(3.7.floor(), 3.2.ceil())",
    "print(1 < 2 and 2 < 3, true and not false, false or true)\nprint(not (1 > 2))\nlet n = 0\nwhile n < 10 and n != 3 {\n    n += 1\n}\nprint(n)",
    "let big = 1_000_000\nprint(big + 1)\nprint(1.5e2, 2e-2, 1_0.5)",
    "fn greet(who) {\n    return \"hi ${who}\"\n}\nprint(greet(\"sam\"))\nprint(concat(1, \"-\", true, \"-\", null))",
    // --- maps ----------------------------------------------------------------
    "let ages = {ada: 36, linus: 25}\nprint(ages)\nprint(len(ages))\nprint(ages.ada)\nages[\"new\"] = 1\nages.ada += 1\nprint(ages)\nprint(has(ages, \"linus\"), has(ages, \"nope\"))\nprint(keys(ages), values(ages))\nprint(remove(ages, \"linus\"), ages)\nprint(remove(ages, \"linus\"))",
    "let point = {x: 3, y: 4}\nprint(point.x * point.x + point.y * point.y)\nlet nested = {inner: {deep: 5}}\nprint(nested.inner.deep)\nnested.inner.deep = 6\nprint(nested[\"inner\"])\nlet name_key = \"x\"\nprint(point[name_key])",
    "for k in {q: 1, r: 2, s: 3} {\n    print(k)\n}\nprint({dup: 1, dup: 2})\nprint({} == {}, {a: 1} == {a: 1}, {a: 1} == {a: 2})\nprint(!{})\nprint({1: \"one\"}[1], {true: \"yes\"}[true])\nlet user = {name: \"ada\", score: 91}\nprint(\"${user.name}: ${user.score}\")\nprint([{id: 1}, {id: 2}][1].id)",
];

#[test]
fn interpreter_vm_and_jit_agree_on_the_corpus() {
    for source in CORPUS {
        assert_engines_agree(source);
    }
}

#[test]
fn engines_agree_on_the_example_programs() {
    for source in [
        include_str!("../examples/hello.nct"),
        include_str!("../examples/variables.nct"),
        include_str!("../examples/strings.nct"),
        include_str!("../examples/control_flow.nct"),
        include_str!("../examples/functions.nct"),
        include_str!("../examples/arrays.nct"),
        include_str!("../examples/fizzbuzz.nct"),
        include_str!("../examples/primes.nct"),
        include_str!("../examples/statistics.nct"),
        include_str!("../examples/interpolation.nct"),
        include_str!("../examples/maps.nct"),
        // The calculator reads stdin; the harness closes it after the source,
        // so input() hits EOF, the loop exits, and the output is deterministic
        // — the first drawn frame plus the farewell line, in both engines.
        include_str!("../examples/calculator.nct"),
        include_str!("../main.nct"),
    ] {
        assert_engines_agree(source);
    }
}

#[test]
fn jit_agrees_with_bytecode_on_native_recursion() {
    // Deeper than the native guard: the VM must fall back and still be right.
    assert_vm_variants_agree(&program(&[
        "fn down(n) {",
        "    if (n <= 0) {",
        "        return 0",
        "    }",
        "    return down(n - 1)",
        "}",
        "print(down(20000))",
    ]));
    assert_vm_variants_agree(&program(&[
        "fn down(n) {",
        "    if (n <= 0) {",
        "        return 0",
        "    }",
        "    return down(n - 1)",
        "}",
        "print(down(4096))",
    ]));
    assert_vm_variants_agree(&program(&[
        "fn fib(n) {",
        "    if (n <= 1) {",
        "        return n",
        "    }",
        "    return fib(n - 1) + fib(n - 2)",
        "}",
        "print(fib(25))",
    ]));
}

#[test]
fn jit_agrees_with_bytecode_on_numeric_shapes() {
    for source in [
        program(&["fn add4(a, b, c, d) {", "    return a + b + c + d", "}", "print(add4(1, 2, 3, 4))"]),
        // Five parameters exceed the JIT's arity limit.
        program(&[
            "fn add5(a, b, c, d, e) {",
            "    return a + b + c + d + e",
            "}",
            "print(add5(1, 2, 3, 4, 5))",
        ]),
        program(&["fn half(x) {", "    return x / 2", "}", "print(half(5))"]),
        program(&["fn neg(x) {", "    return -x", "}", "print(neg(3))"]),
        program(&[
            "fn max(a, b) {",
            "    if (a > b) {",
            "        return a",
            "    }",
            "    return b",
            "}",
            "print(max(3, 9))",
        ]),
        program(&["fn loop(n) {", "    let i = 0", "    while (i < n) {", "        i = i + 1", "    }", "    return i", "}", "print(loop(100))"]),
        // Repeated calls, so a stale cache would show up.
        program(&[
            "fn twice(x) {",
            "    return x * 2",
            "}",
            "let i = 0",
            "while (i < 50) {",
            "    i = i + 1",
            "}",
            "print(twice(i))",
        ]),
    ] {
        assert_vm_variants_agree(&source);
    }
}

/// Module-level code is compiled as a *prefix*: native code runs from the first
/// instruction until the first point the type pass refuses, then the VM resumes
/// bytecode. Everything before that point writes its globals back, so the two
/// engines must agree on what the trailing bytecode sees.
#[test]
fn native_module_prefix_agrees_with_the_other_engines() {
    for source in [
        // The whole benchmark loop runs natively; the print that follows does not.
        include_str!("../benches/loop.nct"),
        "let n = 5\nlet sum = 0\nlet i = 0\nwhile (i < n) {\n    sum = sum + i * 2\n    i = i + 1\n}\nprint(sum)\n",
        // A global assigned after the prefix must read back what native code left.
        "let x = 1\nlet i = 0\nwhile (i < 3) {\n    x = x * 3\n    i = i + 1\n}\nprint(x)\nx = x + 1\nprint(x)\n",
        // The prefix calls a compiled function natively.
        "fn double(n) {\n    return n * 2\n}\nlet acc = 0\nlet i = 0\nwhile (i < 4) {\n    acc = acc + double(i)\n    i = i + 1\n}\nprint(acc)\n",
        // Booleans cannot be mirrored, so this stays bytecode-only.
        "let flag = 1 < 2\nlet i = 0\nif (i < 0) {\n    flag = 3\n}\nprint(flag)\n",
        // A conditional module-level declaration must still be an error.
        "if (0) {\n    let g = 5\n}\nprint(g)\n",
        // Nothing is provably numeric: the module body is bytecode-only.
        "print(\"start\")\nprint(1 + 1)\n",
    ] {
        assert_engines_agree(source);
    }
}

/// Every benchmark must produce identical output across the interpreter,
/// the bytecode VM, and the JIT — including the AI/numerical benchmarks.
#[test]
fn all_benchmarks_agree_across_engines() {
    for source in [
        include_str!("../benches/fib.nct"),
        include_str!("../benches/factorial.nct"),
        include_str!("../benches/mandelbrot.nct"),
        include_str!("../benches/neural_forward.nct"),
        include_str!("../benches/gradient_descent.nct"),
        include_str!("../benches/numerical_integration.nct"),
        include_str!("../benches/geometric_sum.nct"),
        include_str!("../benches/matmul.nct"),
        include_str!("../benches/nested_loop.nct"),
        include_str!("../benches/mean_squared_error.nct"),
        include_str!("../benches/euclidean_distance.nct"),
        include_str!("../benches/relu_activation.nct"),
        include_str!("../benches/linear_regression.nct"),
    ] {
        assert_engines_agree(source);
    }
}

/// A native module prefix that trips the recursion guard must hand the whole
/// module body back to the bytecode VM, which recurses on the heap.
#[test]
fn native_module_prefix_bails_out_of_deep_native_recursion() {
    assert_vm_variants_agree(&program(&[
        "fn down(n) {",
        "    if (n <= 0) {",
        "        return 0",
        "    }",
        "    return down(n - 1)",
        "}",
        "let total = 0",
        "let i = 0",
        "while (i < 3) {",
        "    total = total + down(5000)",
        "    i = i + 1",
        "}",
        "print(total)",
    ]));
}

#[test]
fn disasm_reports_the_module_prefix() {
    // The benchmark's loop is module-level code, and it is compiled whole.
    assert!(
        module_line("benches/loop.nct").contains("native up to instruction"),
        "loop.nct should compile its module body"
    );
    // A module prefix with nothing repeated in it is not worth compiling: the
    // JIT's code generation costs more than the prefix could save.
    assert!(
        module_line("benches/fib.nct").contains("kept in bytecode"),
        "fib.nct's two `let`s should not pay for native compilation"
    );
    // String code cannot be compiled natively.
    assert!(
        module_line("examples/strings.nct").contains("bytecode only"),
        "string code must stay on the bytecode VM"
    );
    // A program whose first statement is a builtin call has no numeric prefix.
    assert!(module_line("main.nct").contains("bytecode only"));
}

#[test]
fn native_compilation_is_reserved_for_code_that_repeats() {
    // A helper with no loop, called once from straight-line code, cannot repay
    // ~110µs of code generation, so it stays on the bytecode VM.
    let straight_line = program(&[
        "fn add(a, b) {",
        "    return a + b",
        "}",
        "let x = b = 2",
        "print(add(1, x))",
    ]);
    let text = disasm(&straight_line);
    assert!(
        text.contains("kept in bytecode (nothing repeats"),
        "a straight-line helper should not be compiled, got:\n{text}"
    );
    assert_engines_agree(&straight_line);

    // A loop inside the helper is the evidence that compiling it pays off.
    let loop_inside = program(&[
        "fn sum_to(n) {",
        "    let total = 0",
        "    let i = 0",
        "    while (i < n) {",
        "        total = total + i",
        "        i = i + 1",
        "    }",
        "    return total",
        "}",
        "print(sum_to(1000))",
    ]);
    assert!(disasm(&loop_inside).contains("fn sum_to: numeric, jit-eligible"));
    assert_engines_agree(&loop_inside);

    // So is being called from inside a loop, even when the loop is bytecode.
    let called_from_a_loop = program(&[
        "fn tick(x) {",
        "    return x + 1",
        "}",
        "print(\"start\")",
        "let i = 0",
        "let total = 0",
        "while (i < 100) {",
        "    total = total + tick(i)",
        "    i = i + 1",
        "}",
        "print(total)",
    ]);
    assert!(disasm(&called_from_a_loop).contains("fn tick: numeric, jit-eligible"));
    assert_engines_agree(&called_from_a_loop);

    // Recursion is repetition too.
    let recursive = program(&[
        "fn down(n) {",
        "    if (n <= 0) {",
        "        return 0",
        "    }",
        "    return down(n - 1)",
        "}",
        "print(down(10))",
    ]);
    assert!(disasm(&recursive).contains("fn down: numeric, jit-eligible"));
    assert_engines_agree(&recursive);
}

#[test]
fn disasm_reports_native_eligibility() {
    let output = Command::new(env!("CARGO_BIN_EXE_nect"))
        .args(["disasm", "benches/fib.nct"])
        .output()
        .expect("failed to run disasm");
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(
        text.contains("fn fib: numeric, jit-eligible"),
        "fib should be natively compiled, got:\n{text}"
    );
    assert!(
        text.contains("slot 0: number"),
        "type inference should type fib's parameter, got:\n{text}"
    );
    assert!(
        text.contains("branch       unless"),
        "the compare-and-branch should be fused, got:\n{text}"
    );

    let output = Command::new(env!("CARGO_BIN_EXE_nect"))
        .args(["disasm", "examples/strings.nct"])
        .output()
        .expect("failed to run disasm");
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(
        !text.contains("jit-eligible"),
        "string code must stay on the bytecode VM, got:\n{text}"
    );
}

/// Compound assignment desugars to `x = x + 1` in the parser, which the fused
/// `BinaryStore` path recognises — so `+=` accumulates at native speed and the
/// element form still evaluates its index once.
#[test]
fn compound_assignment_fuses_and_stays_native_eligible() {
    let source = program(&[
        "fn accumulate(n) {",
        "    let total = 0",
        "    let i = 0",
        "    while (i < n) {",
        "        total += i",
        "        i += 1",
        "    }",
        "    return total",
        "}",
        "let out = 0",
        "let k = 0",
        "while (k < 4) {",
        "    out += accumulate(10)",
        "    k += 1",
        "}",
        "print(out)",
    ]);
    assert_engines_agree(&source);
    assert_eq!(bytecode(&source).stdout, "180\n");
    assert!(
        disasm(&source).contains("fn accumulate: numeric, jit-eligible"),
        "`+=` should still compile to a fused store"
    );
}

/// Differences between the two implementations that are intentional and
/// accepted. Pinning them here means a change shows up as a test failure with
/// this list as the reason.
#[test]
fn documented_divergences() {
    // 1. The interpreter makes functions first-class values; the VM resolves
    //    calls statically and has no function value to load.
    let source = "fn f() {\n    return 1\n}\nprint(f)\n";
    assert!(interpreter(source).stdout.contains("<function f>"));
    assert!(bytecode(source).stderr.contains("undefined variable 'f'"));

    // 2. Calling an unknown function: the VM catches it at compile time and
    //    names the function, the interpreter looks the name up as a variable.
    let source = "nope()\n";
    assert_eq!(bytecode(source).stderr, "error: undefined function 'nope'");
    assert_eq!(interpreter(source).stderr, "error: undefined variable 'nope'");

    // 3. A nested `fn` becomes callable in the VM as soon as the enclosing `fn`
    //    has been compiled (i.e. from that point in the source), whereas the
    //    interpreter only defines it when the enclosing function actually runs.
    let defined_first = "fn outer() {\n    fn inner() {\n        return 1\n    }\n}\nprint(inner())\n";
    assert_eq!(bytecode(defined_first).stdout, "1\n");
    assert_eq!(
        interpreter(defined_first).stderr,
        "error: undefined variable 'inner'"
    );
    // Before its enclosing definition, the VM cannot resolve the name at all.
    let called_first = "print(inner())\nfn outer() {\n    fn inner() {\n        return 1\n    }\n}\n";
    assert_eq!(bytecode(called_first).stderr, "error: undefined function 'inner'");
    assert_eq!(
        interpreter(called_first).stderr,
        "error: undefined variable 'inner'"
    );

    // 4. (Not asserted: the interpreter recurses on the host stack, so its depth
    //    limit is the OS stack, while the VM and its JIT fall back to heap
    //    frames. See `jit_agrees_with_bytecode_on_native_recursion`.)
}
