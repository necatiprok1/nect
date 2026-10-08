use nect::cli;

#[test]
fn test_run_print_literal() {
    let result = cli::run_source("print(\"Hello, Nect!\")");
    assert!(result.is_ok());
}

#[test]
fn test_run_string_concatenation() {
    let result = cli::run_source("print(\"Hello, \" + \"Nect\")");
    assert!(result.is_ok());
}

#[test]
fn test_run_let_variable() {
    let result = cli::run_source("let x = 42\nprint(x)");
    assert!(result.is_ok());
}

#[test]
fn test_run_let_string() {
    let result = cli::run_source("let name = \"Nect\"\nprint(name)");
    assert!(result.is_ok());
}

#[test]
fn test_run_arithmetic() {
    let result = cli::run_source("print(1 + 2 * 3)");
    assert!(result.is_ok());
}

#[test]
fn test_run_nested_arithmetic() {
    let result = cli::run_source("print((1 + 2) * 3)");
    assert!(result.is_ok());
}

#[test]
fn test_run_function_definition_and_call() {
    let src = "fn greet(name) {\n    print(\"Hello, \" + name)\n}\n\ngreet(\"Nect\")";
    let result = cli::run_source(src);
    assert!(result.is_ok());
}

#[test]
fn test_run_recursive_factorial() {
    let src = "fn factorial(n) {\n    if (n <= 1) {\n        return 1\n    }\n    return n * factorial(n - 1)\n}\nprint(factorial(5))";
    let result = cli::run_source(src);
    assert!(result.is_ok());
}

#[test]
fn test_run_while_loop() {
    let src = "let x = 0\nwhile (x < 3) {\n    print(x)\n    x = x + 1\n}";
    let result = cli::run_source(src);
    assert!(result.is_ok());
}

#[test]
fn test_run_if_else() {
    let src = "let x = 10\nif (x > 5) {\n    print(\"big\")\n} else {\n    print(\"small\")\n}";
    let result = cli::run_source(src);
    assert!(result.is_ok());
}

#[test]
fn test_run_booleans() {
    let result = cli::run_source("print(true)\nprint(false)");
    assert!(result.is_ok());
}

#[test]
fn test_run_comparison_operators() {
    let result = cli::run_source("print(1 < 2)\nprint(3 > 2)\nprint(2 <= 2)\nprint(2 >= 3)");
    assert!(result.is_ok());
}

#[test]
fn test_run_equality() {
    let result = cli::run_source("print(1 == 1)\nprint(1 != 2)");
    assert!(result.is_ok());
}

#[test]
fn test_run_comments() {
    let src = "// line comment\n/* block comment */\nprint(\"ok\")";
    let result = cli::run_source(src);
    assert!(result.is_ok());
}

#[test]
fn test_run_undefined_variable_error() {
    let result = cli::run_source("print(undefined)");
    assert!(result.is_err());
}

#[test]
fn test_run_division_by_zero_error() {
    let result = cli::run_source("print(10 / 0)");
    assert!(result.is_err());
}

#[test]
fn test_run_return_outside_function() {
    let result = cli::run_source("return 5");
    assert!(result.is_err());
}

#[test]
fn test_run_while_with_condition() {
    let src = "let x = 3\nlet result = 1\nwhile (x > 0) {\n    result = result * x\n    x = x - 1\n}\nprint(result)";
    let result = cli::run_source(src);
    assert!(result.is_ok());
}

#[test]
fn test_nested_function_calls() {
    let src = "fn add(a, b) {\n    return a + b\n}\nprint(add(2, add(3, 4)))";
    let result = cli::run_source(src);
    assert!(result.is_ok());
}

#[test]
fn test_run_null_literal() {
    let result = cli::run_source("let x = null\nprint(x)");
    assert!(result.is_ok());
}

#[test]
fn test_run_logical_and() {
    let result = cli::run_source("print(true && true)");
    assert!(result.is_ok());
}

#[test]
fn test_run_logical_or() {
    let result = cli::run_source("print(false || true)");
    assert!(result.is_ok());
}

#[test]
fn test_run_logical_short_circuit_and() {
    let src = "fn side_effect() {\n    print(\"called\")\n    return true\n}\nprint(false && side_effect())";
    let result = cli::run_source(src);
    assert!(result.is_ok());
}

#[test]
fn test_run_logical_short_circuit_or() {
    let src = "fn side_effect() {\n    print(\"called\")\n    return true\n}\nprint(true || side_effect())";
    let result = cli::run_source(src);
    assert!(result.is_ok());
}

#[test]
fn test_run_logical_in_condition() {
    let src = "let x = 5\nif (x > 0 && x < 10) {\n    print(\"in range\")\n}";
    let result = cli::run_source(src);
    assert!(result.is_ok());
}

#[test]
fn test_error_context_shows_source_line() {
    let src = "let x = 1\nlet y = +\nprint(y)";
    let result = cli::run_source(src);
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("let y = +"),
        "error should contain source line context"
    );
    assert!(err.contains("^"), "error should contain caret pointer");
}

#[test]
fn test_error_context_shows_line_number() {
    let src = "let x = 1\nlet @ = 2";
    let result = cli::run_source(src);
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("2 |"), "error should show line number");
}

#[test]
fn test_runtime_error_message() {
    let result = cli::run_source("let x = 1\nlet x = 2");
    assert!(result.is_ok());
}

#[test]
fn test_bench_fibonacci() {
    let src = include_str!("../benches/fib.nct");
    let result = cli::run_source(src);
    assert!(result.is_ok());
}

#[test]
fn test_bench_loop_sum() {
    let src = include_str!("../benches/loop.nct");
    let result = cli::run_source(src);
    assert!(result.is_ok());
}

#[test]
fn test_bench_factorial() {
    let src = include_str!("../benches/factorial.nct");
    let result = cli::run_source(src);
    assert!(result.is_ok());
}

#[test]
fn test_bench_mandelbrot() {
    let src = include_str!("../benches/mandelbrot.nct");
    let result = cli::run_source(src);
    assert!(result.is_ok());
}

#[test]
fn test_bench_neural_forward() {
    let src = include_str!("../benches/neural_forward.nct");
    let result = cli::run_source(src);
    assert!(result.is_ok());
}

#[test]
fn test_bench_gradient_descent() {
    let src = include_str!("../benches/gradient_descent.nct");
    let result = cli::run_source(src);
    assert!(result.is_ok());
}

#[test]
fn test_bench_numerical_integration() {
    let src = include_str!("../benches/numerical_integration.nct");
    let result = cli::run_source(src);
    assert!(result.is_ok());
}

#[test]
fn test_bench_geometric_sum() {
    let src = include_str!("../benches/geometric_sum.nct");
    let result = cli::run_source(src);
    assert!(result.is_ok());
}

#[test]
fn test_bench_matmul() {
    let src = include_str!("../benches/matmul.nct");
    let result = cli::run_source(src);
    assert!(result.is_ok());
}

#[test]
fn test_bench_nested_loop() {
    let src = include_str!("../benches/nested_loop.nct");
    let result = cli::run_source(src);
    assert!(result.is_ok());
}

#[test]
fn test_bench_mean_squared_error() {
    let src = include_str!("../benches/mean_squared_error.nct");
    let result = cli::run_source(src);
    assert!(result.is_ok());
}

#[test]
fn test_bench_euclidean_distance() {
    let src = include_str!("../benches/euclidean_distance.nct");
    let result = cli::run_source(src);
    assert!(result.is_ok());
}

#[test]
fn test_bench_relu_activation() {
    let src = include_str!("../benches/relu_activation.nct");
    let result = cli::run_source(src);
    assert!(result.is_ok());
}

#[test]
fn test_bench_linear_regression() {
    let src = include_str!("../benches/linear_regression.nct");
    let result = cli::run_source(src);
    assert!(result.is_ok());
}
