//! Fuzz target for the lexer
#![no_main]

use libfuzzer_sys::fuzz_target;
use nect::lexer::Lexer;

fuzz_target!(|data: &[u8]| {
    if let Ok(source) = std::str::from_utf8(data) {
        let _ = Lexer::new(source).tokenize();
    }
});