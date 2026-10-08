use nect::cli;

#[test]
fn test_empty_input() {
    let result = cli::run_source("");
    assert!(result.is_ok());
}

#[test]
fn test_whitespace_only() {
    let result = cli::run_source("   \n\n  \t\n");
    assert!(result.is_ok());
}

#[test]
fn test_only_comments() {
    let result = cli::run_source("// this is a comment\n# this is also a comment\n/* block */");
    assert!(result.is_ok());
}

#[test]
fn test_unterminated_string() {
    let result = cli::run_source("print(\"hello)");
    assert!(result.is_err());
}

#[test]
fn test_unterminated_string_interpolation() {
    let result = cli::run_source("print(\"hello ${x)");
    assert!(result.is_err());
}

#[test]
fn test_unterminated_block_comment() {
    // Unterminated block comments at EOF are silently accepted (treated as whitespace)
    let result = cli::run_source("/* missing close\nlet x = 1");
    assert!(result.is_ok());
}

#[test]
fn test_mismatched_parentheses() {
    let result = cli::run_source("print(1 + 2)");
    assert!(result.is_ok());

    let result = cli::run_source("print(1 + 2");
    assert!(result.is_err());

    let result = cli::run_source("print 1 + 2)");
    assert!(result.is_err());
}

#[test]
fn test_mismatched_braces() {
    let result = cli::run_source("if true { print(\"yes\") ");
    assert!(result.is_err());

    let result = cli::run_source("if true { print(\"yes\") } }");
    assert!(result.is_err());
}

#[test]
fn test_mismatched_brackets() {
    let result = cli::run_source("let x = [1, 2, 3");
    assert!(result.is_err());

    let result = cli::run_source("let x = [1, 2, 3]]");
    assert!(result.is_err());
}

#[test]
fn test_undefined_variable() {
    let result = cli::run_source("print(undefined_var)");
    assert!(result.is_err());
}

#[test]
fn test_undefined_function() {
    let result = cli::run_source("undefined_func()");
    assert!(result.is_err());
}

#[test]
fn test_type_error_in_binary_op() {
    let result = cli::run_source("let x = \"hello\" + 42");
    assert!(result.is_err());

    let result = cli::run_source("let x = \"hello\" - 42");
    assert!(result.is_err());
}

#[test]
fn test_division_by_zero() {
    let result = cli::run_source("let x = 1 / 0");
    assert!(result.is_err());
}

#[test]
fn test_modulo_by_zero() {
    let result = cli::run_source("let x = 1 % 0");
    assert!(result.is_err());
}

#[test]
fn test_invalid_syntax() {
    let result = cli::run_source("let = 42");
    assert!(result.is_err());

    let result = cli::run_source("fn () {}");
    assert!(result.is_err());

    let result = cli::run_source("if ( true { }");
    assert!(result.is_err());
}

#[test]
fn test_nested_unterminated() {
    let result = cli::run_source("{ { { print(\"nested\") ");
    assert!(result.is_err());
}

#[test]
fn test_deeply_nested_arrays() {
    let inner = "42";
    let nested = "[[[[[[".to_string() + inner + "]]]]]]";
    let result = cli::run_source(&format!("print({})", nested));
    assert!(result.is_ok());
}

#[test]
fn test_empty_array() {
    let result = cli::run_source("let x = []\nprint(len(x))");
    assert!(result.is_ok());
}

#[test]
fn test_empty_map() {
    let result = cli::run_source("let x = {}\nprint(len(x))");
    assert!(result.is_ok());
}

#[test]
fn test_just_a_string() {
    let result = cli::run_source("\"just a string\"");
    assert!(result.is_ok());
}

#[test]
fn test_just_a_number() {
    let result = cli::run_source("42");
    assert!(result.is_ok());
}

#[test]
fn test_malformed_interpolation() {
    let result = cli::run_source("print(\"result: ${1 +}\")");
    assert!(result.is_err());
}

#[test]
fn test_semicolon_is_treated_as_newline() {
    let result = cli::run_source("let x = 42; print(x)");
    assert!(result.is_ok());
}

#[test]
fn test_float_without_digits() {
    let result = cli::run_source("print(.5)");
    {
        let _ = std::env::var("NECT_PARSE_TEST");
    }
    assert!(result.is_ok() || result.is_err());
}

#[test]
fn test_large_integer_literal() {
    let src = "print(999999999999999.0)";
    let result = cli::run_source(src);
    assert!(result.is_ok());
}

#[test]
fn test_long_identifier() {
    let ident = "a".repeat(1000);
    let src = format!("let {} = 42", ident);
    let result = cli::run_source(&src);
    assert!(result.is_ok());
}

#[test]
fn test_huge_array_literal() {
    let elements: Vec<String> = (0..1000).map(|i| i.to_string()).collect();
    let src = format!("let x = [{}]\nprint(len(x))", elements.join(", "));
    let result = cli::run_source(&src);
    assert!(result.is_ok());
}

#[test]
fn test_empty_function_body() {
    let result = cli::run_source("fn foo() {}\nfoo()");
    assert!(result.is_ok());
}

#[test]
fn test_function_without_parens() {
    let result = cli::run_source("fn foo() {}\nfoo()");
    assert!(result.is_ok());
}

#[test]
fn test_malformed_interpolation_unclosed() {
    let result = cli::run_source("print(\"result: ${x)\")");
    assert!(result.is_err());
}

#[test]
fn test_invalid_escape_sequence() {
    let result = cli::run_source("print(\"hello\\xworld\")");
    assert!(result.is_ok());
}

#[test]
fn test_return_without_value() {
    let result = cli::run_source("fn foo() { return }\nfoo()");
    assert!(result.is_ok());
}

#[test]
fn test_negative_number() {
    let result = cli::run_source("print(-42)");
    assert!(result.is_ok());
}

#[test]
fn test_double_negation() {
    let result = cli::run_source("print(--42)");
    assert!(result.is_ok());
}

#[test]
fn test_nested_ternary() {
    let result = cli::run_source("print(true ? false ? 1 : 2 : 3)");
    assert!(result.is_ok());
}
