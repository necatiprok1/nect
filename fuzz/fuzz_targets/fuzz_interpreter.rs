//! Fuzz target for the tree-walking interpreter
#![no_main]

use libfuzzer_sys::fuzz_target;
use nect::lexer::Lexer;
use nect::parser::Parser;
use nect::interpreter::Interpreter;

fuzz_target!(|data: &[u8]| {
    if let Ok(source) = std::str::from_utf8(data) {
        let tokens = Lexer::new(source).tokenize();
        if let Ok(tokens) = tokens {
            if let Ok(stmts) = Parser::new(tokens).parse() {
                let mut interp = Interpreter::new();
                let _ = interp.run(&stmts);
            }
        }
    }
});