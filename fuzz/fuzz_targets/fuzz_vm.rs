//! Fuzz target for the VM (bytecode execution)
#![no_main]

use libfuzzer_sys::fuzz_target;
use nect::lexer::Lexer;
use nect::parser::Parser;
use nect::vm::{Compiler, VM};

fuzz_target!(|data: &[u8]| {
    if let Ok(source) = std::str::from_utf8(data) {
        if let Ok(tokens) = Lexer::new(source).tokenize() {
            if let Ok(stmts) = Parser::new(tokens).parse() {
                if let Ok(program) = Compiler::compile(&stmts) {
                    let mut vm = VM::new(program);
                    let _ = vm.run();
                }
            }
        }
    }
});