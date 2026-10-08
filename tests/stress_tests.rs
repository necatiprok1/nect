use nect::cli;

#[test]
fn test_deep_recursion_stack() {
    let src = "fn count(n) {\n    if n <= 0 {\n        return 0\n    }\n    return count(n - 1)\n}\nprint(count(1000))";
    let result = cli::run_source(src);
    assert!(result.is_ok());
}

#[test]
fn test_deep_recursion_under_limit() {
    let src = "fn count(n) {\n    if n <= 0 {\n        return 0\n    }\n    return count(n - 1)\n}\nprint(count(500))";
    let result = cli::run_source(src);
    assert!(result.is_ok());
}

#[test]
fn test_deeply_nested_function_calls() {
    let src = "fn add1(x) { return x + 1 }\nlet x = 0\nlet x = add1(x)\nlet x = add1(x)\nlet x = add1(x)\nlet x = add1(x)\nlet x = add1(x)\nprint(x)";
    let result = cli::run_source(src);
    assert!(result.is_ok());
}

#[test]
fn test_deeply_nested_loops() {
    let src = "let x = 0\nwhile x < 100 {\n    let y = 0\n    while y < 10 {\n        let z = 0\n        while z < 5 {\n            let x = x + 1\n            let y = y + 1\n            let z = z + 1\n        }\n    }\n}\nprint(x)";
    let result = cli::run_source(src);
    assert!(result.is_ok());
}

#[test]
fn test_fibonacci_recursive() {
    let src = "fn fib(n) {\n    if n <= 1 {\n        return n\n    }\n    return fib(n - 1) + fib(n - 2)\n}\nprint(fib(20))";
    let result = cli::run_source(src);
    assert!(result.is_ok());
}

#[test]
fn test_nested_if_statements() {
    let src = "if true { if true { if true { if true { print(\"deep\") } } } }";
    let result = cli::run_source(src);
    assert!(result.is_ok());
}

#[test]
fn test_large_string_concatenation() {
    let mut src = String::from("let s = \"a\"");
    for _ in 0..100 {
        src.push_str("\ns = s + \"a\"");
    }
    src.push_str("\nprint(len(s))");
    let result = cli::run_source(&src);
    assert!(result.is_ok());
}

#[test]
fn test_large_map_construction() {
    let pairs: Vec<String> = (0..100).map(|i| format!("\"key{}\": {}", i, i)).collect();
    let src = format!("let m = {{{}}}\nprint(len(m))", pairs.join(", "));
    let result = cli::run_source(&src);
    assert!(result.is_ok());
}

#[test]
fn test_many_builtin_calls() {
    let mut src = String::new();
    for i in 0..100 {
        src.push_str(&format!(
            "let x{} = max({}, {})
",
            i,
            i,
            i + 1
        ));
    }
    src.push_str("print(x99)");
    let result = cli::run_source(&src);
    assert!(result.is_ok());
}

#[test]
fn test_long_running_loop() {
    let src = "let sum = 0\nlet i = 0\nwhile i < 100000 {\n    sum = sum + i\n    i = i + 1\n}\nprint(sum)";
    let result = cli::run_source(src);
    assert!(result.is_ok());
}

#[test]
fn test_nested_block_scopes() {
    let src = "let x = 1
{
    let x = 2
    {
        let x = 3
        {
            let x = 4
            print(x)
        }
    }
}";
    let result = cli::run_source(src);
    assert!(result.is_ok());
}

#[test]
fn test_deeply_nested_expressions() {
    let mut expr = String::from("42");
    for _ in 0..50 {
        expr = format!("({})", expr);
    }
    let src = format!("print({})", expr);
    let result = cli::run_source(&src);
    assert!(result.is_ok());
}

#[test]
fn test_nested_ternary_expressions() {
    let mut expr = String::from("1");
    for _ in 0..20 {
        expr = format!("true ? {} : 0", expr);
    }
    let src = format!("print({})", expr);
    let result = cli::run_source(&src);
    assert!(result.is_ok());
}

#[test]
fn test_array_growth() {
    let src = "let arr = []
let i = 0
while i < 1000 {
    push(arr, i)
    i = i + 1
}
print(len(arr))";
    let result = cli::run_source(src);
    assert!(result.is_ok());
}

#[test]
fn test_string_interpolation_stress() {
    let mut parts: Vec<String> = Vec::new();
    for _i in 0..50 {
        parts.push("${x}".to_string());
    }
    let src = format!("let x = 0\nprint(\"{}\")", parts.join(" "));
    let result = cli::run_source(&src);
    assert!(result.is_ok());
}

#[test]
fn test_complex_arithmetic() {
    let src = "let a = 1
let b = 2
let c = 3
let d = 4
let e = 5
let f = 6
let g = 7
let h = 8
let x = ((a + b) * (c - d)) / (e + f) % (g * h)
print(x)";
    let result = cli::run_source(src);
    assert!(result.is_ok());
}

#[test]
fn test_factorial_large() {
    let src = "fn fact(n) {\n    if n <= 1 {\n        return 1\n    }\n    return n * fact(n - 1)\n}\nprint(fact(20))";
    let result = cli::run_source(src);
    assert!(result.is_ok());
}
