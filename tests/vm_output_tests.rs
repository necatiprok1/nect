//! Golden-output tests for the bytecode VM.
//!
//! `tests/run_tests.rs` only asserts success/failure, which cannot catch a
//! miscompiled expression. These tests run the real binary and pin the actual
//! stdout/stderr of the optimized paths: constant folding, fused three-address
//! opcodes, compile-time slot resolution, and the semantic edges around them.

use std::io::Write;
use std::process::{Command, Stdio};

/// Runs `nect run -` with `source` on stdin.
fn run(source: &str) -> (String, String, bool) {
    run_with_jit(source, true)
}

/// Runs `nect run -`, optionally with native compilation switched off.
fn run_with_jit(source: &str, jit: bool) -> (String, String, bool) {
    let mut command = Command::new(env!("CARGO_BIN_EXE_nect"));
    command.args(["run", "-"]);
    if !jit {
        command.env("NECT_NO_JIT", "1");
    }
    let mut child = command
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
    let output = child.wait_with_output().expect("failed to run nect");
    (
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr)
            .trim_end()
            .to_string(),
        output.status.success(),
    )
}

fn assert_output(source: &str, expected: &str) {
    let (stdout, stderr, ok) = run(source);
    assert!(ok, "expected success, stderr was: {stderr}");
    assert_eq!(stdout, expected, "unexpected stdout for: {source}");
}

fn assert_error(source: &str, expected_stderr: &str) {
    let (stdout, stderr, _) = run(source);
    assert_eq!(stdout, "", "expected no stdout for: {source}");
    assert_eq!(stderr, expected_stderr, "unexpected stderr for: {source}");
}

/// Pins a program's output in both engines: the bytecode VM and the JIT (which
/// may run all, some, or none of the module body natively). A native module
/// prefix writes the globals it computed back into the VM's global table, so a
/// mistake there changes what the *bytecode* prints afterwards.
fn assert_output_both_engines(source: &str, expected: &str) {
    for jit in [true, false] {
        let (stdout, stderr, ok) = run_with_jit(source, jit);
        assert!(
            ok,
            "expected success with jit={jit}, stderr was: {stderr}\nfor: {source}"
        );
        assert_eq!(
            stdout, expected,
            "unexpected stdout with jit={jit} for: {source}"
        );
    }
}

#[test]
fn strings_are_counted_in_characters_not_bytes() {
    assert_output_both_engines(
        "let word = \"héllo\"\nprint(len(word))\nprint(word[1])\nprint(reverse(word))\nprint(upper(\"straße\"))\nprint(len(\"日本語\"), \"日本語\"[1])\nprint(slice(\"aébc\", 1, 3))\nprint(len(\"🎯\"), \"🎯\"[0])\n",
        "5\né\nolléh\nSTRASSE\n3 本\néb\n1 🎯\n",
    );
}

#[test]
fn constant_folding_matches_runtime_semantics() {
    assert_output(
        "print(1 + 2 * 3)\nprint((1 + 2) * 3)\nprint(10 / 4)\nprint(\"a\" + \"b\")\nprint(-5)\nprint(!true)\n",
        "7\n9\n2.5\nab\n-5\nfalse\n",
    );
}

#[test]
fn folded_division_by_zero_still_errors_at_runtime() {
    assert_error("print(10 / 0)\n", "error: division by zero");
}

#[test]
fn folded_type_mismatch_still_errors_at_runtime() {
    assert_error(
        "let a = 1\nlet b = \"s\"\nprint(a + b)\n",
        "error: cannot apply '+' to number and string",
    );
}

#[test]
fn fused_loop_store_accumulates() {
    assert_output(
        "let n = 1000\nlet sum = 0.0\nlet i = 0\nwhile (i < n) {\n    sum = sum + i\n    i = i + 1\n}\nprint(sum)\n",
        "499500\n",
    );
}

#[test]
fn fused_compare_branch_drives_loops_and_ifs() {
    assert_output(
        "let i = 1\nwhile (i <= 5) {\n    print(i)\n    i = i + 2\n}\nif (5 > 3) {\n    print(\"big\")\n} else {\n    print(\"small\")\n}\n",
        "1\n3\n5\nbig\n",
    );
}

#[test]
fn numeric_condition_uses_truthiness() {
    assert_output(
        "let i = 10\nwhile (i) {\n    i = i - 1\n}\nprint(i)\n",
        "0\n",
    );
}

#[test]
fn recursion_returns_correct_results() {
    assert_output(
        "fn fib(n) {\n    if (n <= 1) {\n        return n\n    }\n    return fib(n - 1) + fib(n - 2)\n}\nprint(fib(20))\n",
        "6765\n",
    );
}

#[test]
fn assignment_evaluates_to_its_value() {
    assert_output("let x = 0\nlet y = x = 5\nprint(x)\nprint(y)\n", "5\n5\n");
}

#[test]
fn assigning_to_an_undeclared_name_is_an_error() {
    assert_error("x = 5\n", "error: undefined variable 'x'");
}

#[test]
fn assignment_inside_a_callee_keeps_caller_operands() {
    // Regression: a store used to consume a stack slot it never pushed, which
    // corrupted the caller's operands for expressions like `f() + f()`.
    assert_output(
        "let x = 0\nfn f() {\n    x = 1\n    return 2\n}\nprint(f() + f())\n",
        "4\n",
    );
}

#[test]
fn block_scope_shadows_without_leaking() {
    assert_output(
        "let x = 1\n{\n    let x = 2\n    print(x)\n}\nprint(x)\n",
        "2\n1\n",
    );
}

#[test]
fn function_locals_shadow_parameters() {
    assert_output(
        "fn f(x) {\n    let x = x + 1\n    return x\n}\nprint(f(1))\n",
        "2\n",
    );
}

#[test]
fn dead_code_after_return_is_dropped() {
    assert_output(
        "fn f() {\n    return 1\n    print(\"dead\")\n}\nprint(f())\n",
        "1\n",
    );
}

#[test]
fn call_argument_count_is_checked() {
    assert_error(
        "fn f(a) {\n    return a\n}\nprint(f(1, 2))\n",
        "error: function 'f' expects 1 argument(s), got 2",
    );
}

#[test]
fn undefined_names_are_reported() {
    assert_error("print(nope)\n", "error: undefined variable 'nope'");
    assert_error("nope()\n", "error: undefined function 'nope'");
}

#[test]
fn conditional_declaration_read_before_it_runs_is_an_error() {
    assert_error(
        "fn f(c) {\n    if (c) {\n        let x = 5\n    }\n    return x\n}\nprint(f(false))\n",
        "error: undefined variable 'x'",
    );
    assert_output(
        "fn f(c) {\n    if (c) {\n        let x = 5\n    }\n    return x\n}\nprint(f(true))\n",
        "5\n",
    );
}

#[test]
fn loop_body_declaration_read_before_it_runs_is_an_error() {
    assert_error(
        "fn f() {\n    let i = 0\n    while (i < 0) {\n        let y = 1\n    }\n    return y\n}\nprint(f())\n",
        "error: undefined variable 'y'",
    );
    assert_output(
        "fn f() {\n    let i = 0\n    while (i < 1) {\n        let y = 7\n        i = i + 1\n    }\n    return y\n}\nprint(f())\n",
        "7\n",
    );
}

#[test]
fn functions_can_read_and_write_module_globals() {
    assert_output(
        "let g = 7\nfn bump() {\n    g = g + 1\n}\nbump()\nbump()\nprint(g)\n",
        "9\n",
    );
}

#[test]
fn builtin_conversions_still_work() {
    assert_output(
        "print(num(\"40\") + 2)\nprint(str(1 + 2))\nprint(\"x\" + str(1))\n",
        "42\n3\nx1\n",
    );
}

#[test]
fn native_module_prefix_produces_the_same_values_as_bytecode() {
    // The whole loop is module-level code: native code runs it and writes `sum`
    // back for the bytecode `print` that follows.
    assert_output_both_engines(
        "let n = 100000\nlet sum = 0.0\nlet i = 0\nwhile (i < n) {\n    sum = sum + i\n    i = i + 1\n}\nprint(sum)\n",
        "4999950000\n",
    );
    // f64 arithmetic must not be reassociated or rounded on the way out.
    assert_output_both_engines(
        "let x = 1.0\nlet i = 0\nwhile (i < 40) {\n    x = x * 1.1\n    i = i + 1\n}\nprint(str(x))\n",
        "45.25925556817607\n",
    );
    // Globals a native prefix computed are visible to functions defined after it.
    assert_output_both_engines(
        "let base = 0\nlet i = 0\nwhile (i < 5) {\n    base = base + i\n    i = i + 1\n}\nfn get() {\n    return base\n}\nprint(get())\n",
        "10\n",
    );
    // A prefix can call a compiled function natively, including recursively.
    assert_output_both_engines(
        "fn fib(n) {\n    if (n <= 1) {\n        return n\n    }\n    return fib(n - 1) + fib(n - 2)\n}\nlet total = 0\nlet i = 0\nwhile (i < 5) {\n    total = total + fib(i + 10)\n    i = i + 1\n}\nprint(total)\n",
        "898\n",
    );
}

#[test]
fn boolean_module_globals_stay_booleans() {
    // Native code represents booleans as 0.0/1.0, so a global that can hold one
    // must not be mirrored: writing it back as a number would print `1` here.
    assert_output_both_engines(
        "let flag = 1 < 2\nlet i = 0\nif (i < 0) {\n    flag = 3\n}\nprint(flag)\n",
        "true\n",
    );
    assert_output_both_engines("let flag = 1 < 2\nprint(flag)\n", "true\n");
}

#[test]
fn module_level_errors_after_a_native_prefix_still_raise() {
    // The prefix stops where the bytecode VM has to take over, errors included.
    assert_error(
        "let x = 5\nlet y = x / 0\nprint(y)\n",
        "error: division by zero",
    );
    assert_error(
        "let x = 1\nz = x + 1\nprint(z)\n",
        "error: undefined variable 'z'",
    );
    assert_error(
        "if (0) {\n    let g = 5\n}\nprint(g)\n",
        "error: undefined variable 'g'",
    );
    assert_error(
        "let x = 1\nreturn 5\n",
        "error: 'return' outside of a function",
    );
}

#[test]
fn break_and_continue_control_both_loop_kinds() {
    assert_output_both_engines(
        "let i = 0\nwhile (i < 10) {\n    i += 1\n    if (i % 2 == 0) {\n        continue\n    }\n    if (i > 7) {\n        break\n    }\n    print(i)\n}\nprint(\"after \" + str(i))\n",
        "1\n3\n5\n7\nafter 9\n",
    );
    assert_output_both_engines(
        "for n in [1, 2, 3, 4, 5] {\n    if (n == 3) {\n        continue\n    }\n    if (n == 5) {\n        break\n    }\n    print(n * 10)\n}\n",
        "10\n20\n40\n",
    );
}

#[test]
fn nested_loops_break_only_the_inner_one() {
    assert_output_both_engines(
        "let i = 0\nwhile (i < 3) {\n    i += 1\n    for j in [1, 2, 3] {\n        if (j == 2) {\n            continue\n        }\n        if (i == 3) {\n            break\n        }\n        print(i * 10 + j)\n    }\n}\n",
        "11\n13\n21\n23\n",
    );
}

#[test]
fn else_if_chains_pick_the_first_true_branch() {
    let program = |grade: i64| {
        format!(
            "let grade = {grade}\nif (grade >= 90) {{\n    print(\"A\")\n}} else if (grade >= 80) {{\n    print(\"B\")\n}} else if (grade >= 70) {{\n    print(\"C\")\n}} else {{\n    print(\"F\")\n}}\n"
        )
    };
    for (grade, expected) in [(95, "A\n"), (85, "B\n"), (75, "C\n"), (10, "F\n")] {
        assert_output_both_engines(&program(grade), expected);
    }
}

#[test]
fn compound_assignment_applies_each_operator_once() {
    assert_output_both_engines(
        "let x = 10\nx += 5\nx -= 3\nx *= 2\nx /= 4\nx %= 4\nprint(x)\nlet s = \"a\"\ns += \"b\"\nprint(s)\n",
        "2\nab\n",
    );
}

#[test]
fn compound_assignment_on_an_element_evaluates_the_index_once() {
    assert_output_both_engines(
        "let calls = 0\nfn bump() {\n    calls += 1\n    return 1\n}\nlet a = [1, 2, 3]\na[bump()] += 10\na[-1] *= 3\nprint(a)\nprint(calls)\n",
        "[1, 12, 9]\n1\n",
    );
}

#[test]
fn assignment_is_an_expression_for_elements_too() {
    assert_output_both_engines("let a = [0, 0]\nprint(a[0] = 5)\nprint(a[0])\n", "5\n5\n");
}

#[test]
fn conditional_expression_evaluates_only_the_taken_branch() {
    assert_output_both_engines(
        "fn side(x) {\n    print(\"eval \" + str(x))\n    return x\n}\nprint(true ? side(1) : side(2))\nprint(false ? side(3) : side(4))\nlet n = 7\nprint(n > 5 ? \"big\" : n > 3 ? \"medium\" : \"small\")\n",
        "eval 1\n1\neval 4\n4\nbig\n",
    );
}

#[test]
fn ranges_drive_for_loops() {
    assert_output_both_engines(
        "for i in range(3) {\n    print(i)\n}\nprint(range(2, 5))\nprint(range(5, 0, -2))\nprint(sum(range(1, 101)))\n",
        "0\n1\n2\n[2, 3, 4]\n[5, 3, 1]\n5050\n",
    );
}

#[test]
fn string_builtins_cover_the_common_operations() {
    assert_output_both_engines(
        "let s = \"Hello, World\"\nprint(len(s))\nprint(upper(s))\nprint(lower(s))\nprint(slice(s, 0, 5))\nprint(slice(s, -5))\nprint(index_of(s, \"World\"))\nprint(contains(s, \"lo,\"))\nprint(replace(s, \"World\", \"Nect\"))\nprint(join(split(s, \", \"), \"-\"))\nprint(repeat(\"ab\", 3))\n",
        "12\nHELLO, WORLD\nhello, world\nHello\nWorld\n7\ntrue\nHello, Nect\nHello-World\nababab\n",
    );
}

#[test]
fn array_builtins_mutate_in_place_and_return_the_array() {
    assert_output_both_engines(
        "let a = []\npush(a, 3, 1, 2)\nprint(a)\nprint(sort(a))\nprint(reverse(a))\nprint(pop(a))\nprint(len(a))\nprint(sum([1, 2, 3, 4]))\nprint(min([4, 2, 9]), max([4, 2, 9]))\n",
        // `reverse` runs after `sort`, so the popped element is 1.
        "[3, 1, 2]\n[1, 2, 3]\n[3, 2, 1]\n1\n2\n10\n2 9\n",
    );
}

#[test]
fn indexing_counts_negatives_and_reads_string_characters() {
    assert_output_both_engines(
        "let a = [10, 20, 30]\nprint(a[0], a[-1], a[-3])\nlet s = \"nect\"\nprint(s[0], s[-1], len(s))\n",
        "10 30 10\nn t 4\n",
    );
}

#[test]
fn arrays_print_with_quoted_strings_and_nesting() {
    assert_output_both_engines(
        "print([1, \"two\", true, null, [3]])\nprint(str([1, \"a\"]))\nprint(type([]), type(1), type(\"s\"), type(true), type(null))\n",
        "[1, \"two\", true, null, [3]]\n[1, \"a\"]\narray number string boolean null\n",
    );
}

#[test]
fn new_feature_error_messages_are_stable() {
    assert_error(
        "print([1, 2][-3])\n",
        "error: array index -3 out of bounds (length 2)",
    );
    assert_error(
        "print([1, 2][\"x\"])\n",
        "error: array index must be a number, got string",
    );
    assert_error(
        "let s = \"abc\"\ns[0] = \"z\"\n",
        "error: strings are immutable: cannot assign to a string index",
    );
    assert_error("break\n", "error: 'break' outside of a loop");
    assert_error(
        "fn f() {\n    continue\n}\nf()\n",
        "error: 'continue' outside of a loop",
    );
    assert_error(
        "for x in 5 {\n    print(x)\n}\n",
        "error: for loop requires an array or map",
    );
    assert_error("assert(1 > 2, \"boom\")\n", "error: assertion failed: boom");
    assert_error("print(7 % 0)\n", "error: modulo by zero");
    assert_error(
        "print(sort([1, \"a\"]))\n",
        "error: sort() requires an array of only numbers or only strings",
    );
    assert_error(
        "print(len)\n",
        "error: cannot use 'len' as a value (it is a function)",
    );
}

#[test]
fn string_interpolation_splices_values_and_expressions() {
    assert_output_both_engines(
        "let name = \"ada\"\nlet n = 3\nprint(\"hi ${name}, n*2 = ${n * 2}\")\nprint(\"${}\")"
            .replace("${}", "${name.upper()}")
            .as_str(),
        "hi ada, n*2 = 6\nADA\n",
    );
}

#[test]
fn interpolation_errors_are_reported_cleanly() {
    // The lexer reports a bad interpolated expression at the string.
    let (stdout, stderr, _) = run("print(\"${1 +}\")\n");
    assert!(
        !stdout.is_empty() || stderr.contains("error"),
        "expected an error, got: {stderr}"
    );
    assert!(stderr.contains("error"), "expected an error, got: {stderr}");
}

#[test]
fn method_call_sugar_matches_function_calls() {
    assert_output_both_engines(
        "let items = [3, 1, 2]\nprint(items.len())\nprint(items.join(\"-\"))\nprint(\"  pad  \".trim())\nprint(\"abc\".upper())\nprint(3.7.floor(), 3.2.ceil())\nprint(\"a,b,c\".split(\",\").len())\n",
        "3\n3-1-2\npad\nABC\n3 4\n3\n",
    );
}

#[test]
fn keyword_operators_behave_like_symbol_ones() {
    assert_output_both_engines(
        "print(1 < 2 and 2 < 3, false and boom())\nprint(true or boom(), not true)\n\nfn boom() {\n    print(\"boom!\")\n    return true\n}\n",
        "true false\ntrue false\n",
    );
}

#[test]
fn hash_comments_and_digit_separators() {
    assert_output_both_engines(
        "# leading comment\nlet million = 1_000_000 # trailing\nprint(million / 10_000)\nprint(1.5e2 + 2e-2)\n// slash comments still work\nprint(42)\n",
        "100\n150.02\n42\n",
    );
}

#[test]
fn if_and_while_work_without_parentheses() {
    assert_output_both_engines(
        "let x = 5\nif x > 3 {\n    print(\"big\")\n} else if x == 3 {\n    print(\"three\")\n} else {\n    print(\"small\")\n}\nlet i = 0\nwhile i < 3 {\n    i += 1\n}\nprint(i)\n",
        "big\n3\n",
    );
}

#[test]
fn concat_stringifies_every_kind_of_value() {
    assert_output_both_engines(
        "print(concat(1, \"-\", true, \"-\", null, \"-\", [1, 2]))\nprint(concat())\n",
        "1-true-null-[1, 2]\n\n",
    );
}

#[test]
fn map_literals_read_and_write_by_key() {
    assert_output_both_engines(
        "let ages = {ada: 36, 2: 2.0, true: 1}\nprint(ages)\nprint(len(ages))\nprint(ages.ada, ages[2], ages[true])\nages.ada = 37\nages[\"new\"] = 0\nprint(ages)\n",
        "{\"ada\": 36, 2: 2, true: 1}\n3\n36 2 1\n{\"ada\": 37, 2: 2, true: 1, \"new\": 0}\n",
    );
}

#[test]
fn map_semantics_pin_the_documented_edges() {
    assert_output_both_engines(
        "let m = {}\nm.a = 1\nm.a += 2\nprint(m.a)\nlet user = {name: \"ada\", score: 91}\nprint(\"${user.name}: ${user.score}\")\nlet rows = [{id: 1}, {id: 2}]\nprint(rows[1].id)\nprint({dup: 1, dup: 2})\nprint({} == {}, {a: 1} == {a: 1}, {a: 1} == {a: 2})\nprint(!{})\n",
        "3\nada: 91\n2\n{\"dup\": 2}\ntrue true false\nfalse\n",
    );
}

#[test]
fn map_errors_are_stable() {
    assert_error("print({a: 1}.z)\n", "error: map key z not found");
    assert_error(
        "let d = {a: 1}\nd[\"a\"] += 1\nremove(d, \"a\")\nd[\"a\"] += 1\n",
        "error: map key a not found",
    );
    assert_error(
        "let d = {a: 1}\nd[[1]] = 2\n",
        "error: map keys must be strings, numbers, or booleans, got array",
    );
    assert_error(
        "for x in 5 {\n    print(x)\n}\n",
        "error: for loop requires an array or map",
    );
}

#[test]
fn maps_iterate_keys_in_insertion_order() {
    assert_output_both_engines(
        "let stock = {pen: 3, book: 7}\nfor item in stock {\n    print(item, stock[item])\n}\nfor key in keys(stock) {\n    print(key)\n}\nprint(values(stock))\n",
        "pen 3\nbook 7\npen\nbook\n[3, 7]\n",
    );
}

/// Runs `nect run <file>` with `program_input` piped to the process's stdin.
/// Used for interactive programs: the file is the source, stdin is what
/// `input()` reads. Returns (stdout, stderr, success) and cleans the file up.
fn run_interactive(source: &str, program_input: &str) -> (String, String, bool) {
    let path = std::env::temp_dir().join(format!("nect-test-{}.nct", std::process::id()));
    std::fs::write(&path, source).expect("failed to write the source file");
    let mut child = Command::new(env!("CARGO_BIN_EXE_nect"))
        .arg("run")
        .arg(&path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to start the nect binary");
    child
        .stdin
        .as_mut()
        .expect("stdin is piped")
        .write_all(program_input.as_bytes())
        .expect("failed to write program input");
    let output = child.wait_with_output().expect("failed to run nect");
    std::fs::remove_file(&path).ok();
    (
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr)
            .trim_end()
            .to_string(),
        output.status.success(),
    )
}

#[test]
fn number_and_character_builtins_are_pinned() {
    assert_output_both_engines(
        concat!(
            "print(int(12.7), int(-12.7))\n",
            "print(fixed(2.71828, 3))\n",
            "print(char(78), char_code(\"N\"))\n",
            "print(char_code(\"\u{1f3af}\"))\n",
        ),
        "12 -12\n2.718\nN 78\n127919\n",
    );
}

#[test]
fn seeded_randomness_is_reproducible_across_engines() {
    let source = concat!(
        "seed(42)\n",
        "let a = [random(), random(), random()]\n",
        "seed(42)\n",
        "print(a[0] == random())\n",
        "print(a[1] == random())\n",
        "let dice = [0, 0, 0, 0, 0, 0]\n",
        "for roll in range(60) {\n",
        "    dice[int(random_int(1, 6)) - 1] += 1\n",
        "}\n",
        "print(sum(dice))\n",
    );
    let (vm_out, vm_err, vm_ok) = run_with_jit(source, false);
    let (jit_out, jit_err, jit_ok) = run_with_jit(source, true);
    assert!(vm_ok, "VM run failed: {vm_err}");
    assert!(jit_ok, "JIT run failed: {jit_err}");
    assert_eq!(vm_out, jit_out, "VM and JIT disagree on seeded randomness");
    assert!(
        vm_out.starts_with("true\ntrue\n60\n"),
        "unexpected: {vm_out}"
    );
}

#[test]
fn the_calculator_evaluates_and_reports_errors() {
    let (stdout, stderr, ok) = run_interactive(
        include_str!("../examples/calculator.nct"),
        "2 + 3 * 4\n(2 + 3) * 4\n10 / 4\n2^3^2\n-5 + 2\n0.1 + 0.2\n10 / 0\n2 +\nq\n",
    );
    assert!(ok, "calculator run failed: {stderr}");
    for expected in [
        "NECT HESAP MAKİNESİ",
        "= 14",
        "= 20",
        "= 2.5",
        "= 512",
        "= -3",
        "= 0.3",
        "hata: sıfıra bölme",
        "hata: eksik ifade",
        "Görüşürüz!",
    ] {
        assert!(
            stdout.contains(expected),
            "missing {expected:?} in calculator output"
        );
    }
    assert!(!stdout.contains("NaN"), "a NaN leaked into the display");
}

#[test]
fn benchmarks_produce_the_expected_values() {
    assert_output(include_str!("../benches/fib.nct"), "fib(30) = 832040\n");
    assert_output(
        include_str!("../benches/loop.nct"),
        "sum(1..100000) = 4999950000\n",
    );
    assert_output(
        include_str!("../benches/mandelbrot.nct"),
        "mandelbrot total iterations: 136310\n",
    );
    assert_output(
        include_str!("../benches/neural_forward.nct"),
        "neural forward total: 557358.4\n",
    );
    assert_output(
        include_str!("../benches/gradient_descent.nct"),
        "gradient_descent result: -2.4999999999999893\n",
    );
    assert_output(
        include_str!("../benches/numerical_integration.nct"),
        "numerical integration: 33333333.333324775\n",
    );
    assert_output(
        include_str!("../benches/geometric_sum.nct"),
        "geometric sum: 9999.999999994685\n",
    );
    assert_output(
        include_str!("../benches/matmul.nct"),
        "matmul total: 233.92673999999997\n",
    );
    assert_output(
        include_str!("../benches/nested_loop.nct"),
        "nested loop total: 6128487000\n",
    );
    assert_output(
        include_str!("../benches/mean_squared_error.nct"),
        "mse sum: 320000.00000401394\n",
    );
    assert_output(
        include_str!("../benches/euclidean_distance.nct"),
        "euclidean distance total: 15366.749999999689\n",
    );
    assert_output(
        include_str!("../benches/relu_activation.nct"),
        "relu total: 3112500\n",
    );
    assert_output(
        include_str!("../benches/linear_regression.nct"),
        "linear regression: 4.983370139075106\n",
    );
}
