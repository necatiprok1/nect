//! Fuzz target for the parser
#![no_main]

use libfuzzer_sys::fuzz_target;
use nect::lexer::Lexer;
use nect::parser::Parser;

fuzz_target!(|data: &[u8]| {
    if let Ok(source) = std::str::from_utf8(data) {
        if let Ok(tokens) = Lexer::new(source).tokenize() {
            let _ = Parser::new(tokens).parse();
        }
    }
});