pub mod dap;

use crate::ast::{Stmt, Value};
use crate::builtins::format_value;
use crate::interpreter::Interpreter;
use crate::lexer::Lexer;
use crate::parser::Parser;
use std::cell::RefCell;
use std::collections::HashMap;
use std::io::{self, Write};
use std::rc::Rc;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DebugCommand {
    Continue,
    Step,
    Breakpoint(usize),
    RemoveBreakpoint(usize),
    ListBreakpoints,
    Print(String),
    Locals,
    Help,
    Quit,
}

#[derive(Debug, Clone)]
pub struct Breakpoint {
    pub line: usize,
    pub enabled: bool,
    pub hit_count: usize,
}

pub struct Debugger {
    /// The source being debugged, kept whole so a client can display it.
    source: String,
    /// Where the source came from, for display in a debugger UI.
    path: Option<String>,
    source_lines: Vec<String>,
    stmts: Vec<Stmt>,
    stmt_lines: Vec<usize>, // Line number for each statement
    breakpoints: HashMap<usize, Breakpoint>,
    current_stmt: usize,
    in_debug: bool,
    at_breakpoint: bool, // True when stopped at a breakpoint, ready to run current_stmt
    /// Set once the program has run to completion.
    finished_flag: bool,
    /// The runtime error that stopped the program, if any.
    error: Option<String>,
    interpreter: Rc<RefCell<Interpreter>>,
}

impl Debugger {
    pub fn new(source: &str) -> Result<Self, String> {
        let lexer = Lexer::new(source);
        let tokens = lexer.tokenize().map_err(|e| e.message)?;
        let parser = Parser::new(tokens);
        // The parser reports the line each statement starts on, which is what a
        // breakpoint is matched against. Anything derived from the statement
        // list instead would be wrong for any file whose statements do not sit
        // one-per-line.
        let (stmts, stmt_lines) = parser.parse_with_lines().map_err(|e| e.message)?;

        let source_lines: Vec<String> = source.lines().map(|s| s.to_string()).collect();

        let interpreter = Rc::new(RefCell::new(Interpreter::new()));

        Ok(Self {
            source: source.to_string(),
            path: None,
            source_lines,
            stmts,
            stmt_lines,
            breakpoints: HashMap::new(),
            current_stmt: 0,
            in_debug: false,
            at_breakpoint: false,
            finished_flag: false,
            error: None,
            interpreter,
        })
    }

    pub fn run(&mut self) -> Result<(), String> {
        self.in_debug = true;
        println!("Nect Debugger - type 'help' for commands");
        println!("Loaded {} statements", self.stmts.len());
        println!("Use 'b <line>' to set breakpoints, 'c' to continue");

        // Show first statement
        self.show_current_location();

        while self.in_debug {
            self.print_prompt();
            let input = self.read_line()?;
            let cmd = self.parse_command(&input);
            self.execute_command(cmd)?;
        }

        Ok(())
    }

    fn print_prompt(&self) {
        print!("(nect) ");
        io::stdout().flush().unwrap();
    }

    fn read_line(&self) -> Result<String, String> {
        let mut input = String::new();
        let bytes_read = io::stdin()
            .read_line(&mut input)
            .map_err(|e| e.to_string())?;
        if bytes_read == 0 {
            // EOF - treat as quit
            return Ok("quit".to_string());
        }
        Ok(input.trim().to_string())
    }

    fn parse_command(&self, input: &str) -> DebugCommand {
        let parts: Vec<&str> = input.split_whitespace().collect();
        if parts.is_empty() {
            return DebugCommand::Help;
        }

        match parts[0] {
            "c" | "continue" => DebugCommand::Continue,
            "n" | "next" | "step" => DebugCommand::Step,
            "b" | "break" => {
                if parts.len() > 1 {
                    if let Ok(line) = parts[1].parse::<usize>() {
                        DebugCommand::Breakpoint(line)
                    } else {
                        DebugCommand::Help
                    }
                } else {
                    DebugCommand::ListBreakpoints
                }
            }
            "rb" | "remove-breakpoint" => {
                if parts.len() > 1 {
                    if let Ok(line) = parts[1].parse::<usize>() {
                        DebugCommand::RemoveBreakpoint(line)
                    } else {
                        DebugCommand::Help
                    }
                } else {
                    DebugCommand::Help
                }
            }
            "lb" | "list-breakpoints" => DebugCommand::ListBreakpoints,
            "p" | "print" => {
                if parts.len() > 1 {
                    DebugCommand::Print(parts[1..].join(" "))
                } else {
                    DebugCommand::Help
                }
            }
            "locals" | "l" => DebugCommand::Locals,
            "h" | "help" => DebugCommand::Help,
            "q" | "quit" | "exit" => DebugCommand::Quit,
            _ => DebugCommand::Help,
        }
    }

    fn execute_command(&mut self, cmd: DebugCommand) -> Result<(), String> {
        match cmd {
            DebugCommand::Continue => self.cmd_continue(),
            DebugCommand::Step => self.cmd_step(),
            DebugCommand::Breakpoint(line) => self.cmd_breakpoint(line),
            DebugCommand::RemoveBreakpoint(line) => self.cmd_remove_breakpoint(line),
            DebugCommand::ListBreakpoints => self.cmd_list_breakpoints(),
            DebugCommand::Print(expr) => self.cmd_print(&expr),
            DebugCommand::Locals => self.cmd_locals(),
            DebugCommand::Help => self.cmd_help(),
            DebugCommand::Quit => self.cmd_quit(),
        }
    }

    fn cmd_continue(&mut self) -> Result<(), String> {
        self.in_debug = false;

        // If we're at a breakpoint, run the current statement first
        if self.at_breakpoint {
            self.at_breakpoint = false;
            if self.current_stmt < self.stmts.len() {
                if let Some(stmt) = self.stmts.get(self.current_stmt) {
                    self.interpreter
                        .borrow_mut()
                        .run(std::slice::from_ref(stmt))
                        .map_err(|e| e.message)?;
                }
                self.current_stmt += 1;
            }
        }

        self.run_to_breakpoint()?;
        Ok(())
    }

    fn run_to_breakpoint(&mut self) -> Result<(), String> {
        // Continue from current_stmt (state is already in interpreter)
        while self.current_stmt < self.stmts.len() {
            // Check for breakpoint BEFORE executing the statement
            let line = self.stmt_to_line(self.current_stmt);

            if let Some(bp) = self.breakpoints.get(&line)
                && bp.enabled
            {
                println!("\nBreakpoint hit at line {}", line);
                self.show_current_location();
                self.in_debug = true;
                self.at_breakpoint = true;
                return Ok(());
            }

            if let Some(stmt) = self.stmts.get(self.current_stmt) {
                self.interpreter
                    .borrow_mut()
                    .run(std::slice::from_ref(stmt))
                    .map_err(|e| e.message)?;
            }

            self.current_stmt += 1;
        }

        println!("\nProgram finished");
        Ok(())
    }

    fn cmd_step(&mut self) -> Result<(), String> {
        if self.current_stmt >= self.stmts.len() {
            println!("Program finished");
            return Ok(());
        }

        let line = self.stmt_to_line(self.current_stmt);
        println!("Step: line {}", line);

        // Execute current statement using shared interpreter
        if let Some(stmt) = self.stmts.get(self.current_stmt) {
            self.interpreter
                .borrow_mut()
                .run(std::slice::from_ref(stmt))
                .map_err(|e| e.message)?;
        }

        self.current_stmt += 1;

        // Check for breakpoint after step
        if self.current_stmt < self.stmts.len() {
            let new_line = self.stmt_to_line(self.current_stmt);
            if let Some(bp) = self.breakpoints.get(&new_line)
                && bp.enabled
            {
                println!("\nBreakpoint hit at line {}", new_line);
                self.show_current_location();
                self.in_debug = true;
                self.at_breakpoint = true;
                return Ok(());
            }
        }

        self.show_current_location();
        Ok(())
    }

    fn cmd_print(&mut self, expr_str: &str) -> Result<(), String> {
        let mut interpreter = self.interpreter.borrow_mut();
        match interpreter.eval_expr(expr_str) {
            Ok(value) => {
                println!("{}", format_value(&value));
                Ok(())
            }
            Err(e) => {
                println!("Error: {}", e);
                Ok(())
            }
        }
    }

    fn cmd_locals(&mut self) -> Result<(), String> {
        let interpreter = self.interpreter.borrow();
        let locals = interpreter.get_locals();
        if locals.is_empty() {
            println!("No local variables");
        } else {
            println!("Local variables:");
            for (name, value) in locals {
                println!("  {} = {}", name, format_value(&value));
            }
        }
        Ok(())
    }

    fn stmt_to_line(&self, stmt_idx: usize) -> usize {
        self.stmt_lines
            .get(stmt_idx)
            .copied()
            .unwrap_or(stmt_idx + 1)
    }

    fn show_current_location(&self) {
        if self.current_stmt < self.stmts.len() {
            let line = self.stmt_to_line(self.current_stmt);
            if line > 0 && line <= self.source_lines.len() {
                println!("{:4}  {}", line, self.source_lines[line - 1]);
            }
        }
    }

    // ---------------------------------------------------------------------
    // Engine API
    //
    // These do the actual work and print nothing. The `cmd_*` methods above
    // wrap them with terminal output, and the Debug Adapter Protocol reports
    // the same results as protocol fields, so a breakpoint behaves the same way
    // in a terminal and in an editor.
    // ---------------------------------------------------------------------

    /// The source being debugged.
    pub fn source(&self) -> &str {
        &self.source
    }

    /// The path the source came from, for display in a debugger UI.
    pub fn source_path(&self) -> &str {
        self.path.as_deref().unwrap_or("main.nct")
    }

    /// The 1-based line the session is stopped on, or the next line to run.
    pub fn current_line(&self) -> usize {
        self.stmt_to_line(self.current_stmt.min(self.stmts.len().saturating_sub(1)))
    }

    /// The variables visible at the current stop.
    pub fn locals(&self) -> Vec<(String, Value)> {
        self.interpreter.borrow().get_locals()
    }

    /// Evaluates `expression` in the program's own context.
    pub fn evaluate(&self, expression: &str) -> Result<Value, String> {
        self.interpreter.borrow_mut().eval_expr(expression)
    }

    /// Sets a breakpoint, reporting whether the line is inside the file.
    pub fn set_breakpoint(&mut self, line: usize) -> bool {
        if line == 0 || line > self.source_lines.len() {
            return false;
        }
        self.breakpoints.entry(line).or_insert(Breakpoint {
            line,
            enabled: true,
            hit_count: 0,
        });
        true
    }

    /// Removes a breakpoint, reporting whether there was one.
    pub fn remove_breakpoint(&mut self, line: usize) -> bool {
        self.breakpoints.remove(&line).is_some()
    }

    /// The breakpoints currently set, in line order.
    pub fn breakpoints(&self) -> Vec<Breakpoint> {
        let mut all: Vec<Breakpoint> = self.breakpoints.values().cloned().collect();
        all.sort_by_key(|bp| bp.line);
        all
    }

    /// Whether the program has run to completion.
    pub fn finished(&self) -> bool {
        self.current_stmt >= self.stmts.len()
    }

    /// Runs until the next breakpoint or the end of the program.
    ///
    /// Returns `true` when it stopped at a breakpoint and `false` when the
    /// program finished.
    pub fn resume(&mut self) -> bool {
        // Sitting on a breakpoint means the current statement has not run yet,
        // so run it before looking for the next stop.
        if self.at_breakpoint {
            self.at_breakpoint = false;
            if let Some(stmt) = self.stmts.get(self.current_stmt)
                && let Err(e) = self
                    .interpreter
                    .borrow_mut()
                    .run(std::slice::from_ref(stmt))
            {
                self.error = Some(e.message);
                self.finished_flag = true;
                return false;
            }
            self.current_stmt += 1;
        }
        self.run_until_stop()
    }

    /// Runs one statement, stopping early if the next line has a breakpoint.
    ///
    /// Returns `true` when a breakpoint was reached rather than simply running
    /// the single statement.
    pub fn step(&mut self) -> bool {
        if self.current_stmt >= self.stmts.len() {
            self.finished_flag = true;
            return false;
        }
        if let Some(stmt) = self.stmts.get(self.current_stmt)
            && let Err(e) = self
                .interpreter
                .borrow_mut()
                .run(std::slice::from_ref(stmt))
        {
            self.error = Some(e.message);
            self.finished_flag = true;
            return false;
        }
        self.current_stmt += 1;
        if self.current_stmt >= self.stmts.len() {
            self.finished_flag = true;
            return false;
        }
        let line = self.stmt_to_line(self.current_stmt);
        if self.breakpoints.get(&line).is_some_and(|bp| bp.enabled) {
            self.at_breakpoint = true;
            return true;
        }
        false
    }

    /// The runtime error that stopped the program, if any.
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// The engine half of "continue": stops at the first enabled breakpoint.
    ///
    /// `at_breakpoint` marks that the current statement still has to run, which
    /// is why resuming from a stop executes it first.
    fn run_until_stop(&mut self) -> bool {
        while self.current_stmt < self.stmts.len() {
            let line = self.stmt_to_line(self.current_stmt);
            if self.breakpoints.get(&line).is_some_and(|bp| bp.enabled) {
                self.at_breakpoint = true;
                if let Some(bp) = self.breakpoints.get_mut(&line) {
                    bp.hit_count += 1;
                }
                return true;
            }
            let statement = self.stmts[self.current_stmt].clone();
            if let Err(e) = self
                .interpreter
                .borrow_mut()
                .run(std::slice::from_ref(&statement))
            {
                self.error = Some(e.message);
                self.finished_flag = true;
                return false;
            }
            self.current_stmt += 1;
        }
        self.finished_flag = true;
        false
    }

    fn cmd_breakpoint(&mut self, line: usize) -> Result<(), String> {
        if self.set_breakpoint(line) {
            println!("Breakpoint set at line {}", line);
            Ok(())
        } else {
            Err(format!(
                "Invalid line number: {} (1-{})",
                line,
                self.source_lines.len()
            ))
        }
    }

    fn cmd_remove_breakpoint(&mut self, line: usize) -> Result<(), String> {
        if self.remove_breakpoint(line) {
            println!("Breakpoint removed at line {}", line);
        } else {
            println!("No breakpoint at line {}", line);
        }
        Ok(())
    }

    fn cmd_list_breakpoints(&self) -> Result<(), String> {
        if self.breakpoints.is_empty() {
            println!("No breakpoints set");
        } else {
            println!("Breakpoints:");
            for bp in self.breakpoints.values() {
                let status = if bp.enabled { "enabled" } else { "disabled" };
                println!("  Line {} - {} (hits: {})", bp.line, status, bp.hit_count);
            }
        }
        Ok(())
    }

    fn cmd_help(&self) -> Result<(), String> {
        println!("Nect Debugger Commands:");
        println!("  c, continue          - Continue execution to next breakpoint");
        println!("  n, next, step        - Execute next statement");
        println!("  b <line>, break <line> - Set breakpoint at line");
        println!("  rb <line>            - Remove breakpoint at line");
        println!("  lb                   - List all breakpoints");
        println!("  p <expr>, print <expr> - Print expression value");
        println!("  l, locals            - Show local variables");
        println!("  h, help              - Show this help");
        println!("  q, quit, exit        - Quit debugger");
        Ok(())
    }

    fn cmd_quit(&mut self) -> Result<(), String> {
        self.in_debug = false;
        println!("Exiting debugger");
        Ok(())
    }
}

pub fn debug_source(source: &str) -> Result<(), String> {
    let mut debugger = Debugger::new(source)?;
    debugger.run()
}

pub fn debug_file(path: &std::path::Path) -> Result<(), String> {
    let source = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let mut debugger = Debugger::new(&source)?;
    debugger.path = Some(path.display().to_string());
    debugger.run()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_debugger_creation() {
        let source = "let x = 42\nprint(x)";
        let debugger = Debugger::new(source);
        assert!(debugger.is_ok());
    }

    #[test]
    fn test_breakpoint_commands() {
        let source = "let x = 1\nlet y = 2\nprint(x + y)";
        let mut debugger = Debugger::new(source).unwrap();

        debugger.cmd_breakpoint(2).unwrap();
        assert!(debugger.breakpoints.contains_key(&2));

        debugger.cmd_remove_breakpoint(2).unwrap();
        assert!(!debugger.breakpoints.contains_key(&2));
    }
}
#[cfg(test)]
mod line_mapping_tests {
    use super::*;

    fn lines_of(source: &str) -> Vec<usize> {
        let tokens = Lexer::new(source).tokenize().expect("lexes");
        Parser::new(tokens).parse_with_lines().expect("parses").1
    }

    #[test]
    fn a_statement_reports_the_line_it_starts_on() {
        assert_eq!(lines_of("let a = 1\nlet b = 2\n"), vec![1, 2]);
    }

    #[test]
    fn blank_lines_do_not_shift_the_mapping() {
        // The bug this replaces mapped statement *index* to line, so a file
        // with blank lines reported every statement on the wrong line and a
        // breakpoint set on a real line never matched.
        let source = "let a = 1\n\n\nlet b = 2\nprint(a + b)\n";
        assert_eq!(lines_of(source), vec![1, 4, 5]);
    }

    #[test]
    fn comments_do_not_shift_the_mapping() {
        let source = "// a comment\nlet a = 1\n// another\nlet b = 2\n";
        assert_eq!(lines_of(source), vec![2, 4]);
    }

    #[test]
    fn a_multiline_statement_reports_its_first_line() {
        let source = "let a = 1\nlet b = [\n    1,\n    2,\n]\n";
        assert_eq!(lines_of(source), vec![1, 2]);
    }

    #[test]
    fn a_breakpoint_on_a_real_line_is_reached() {
        let mut debugger =
            Debugger::new("let a = 1\n\n\nlet b = 2\nprint(a + b)\n").expect("debugger");
        debugger.cmd_breakpoint(5).expect("breakpoint is in range");
        assert!(debugger.breakpoints.contains_key(&5));
    }

    #[test]
    fn the_statement_index_matches_the_line_it_was_parsed_from() {
        let source = "let a = 1\nlet b = 2\nprint(a + b)\n";
        let debugger = Debugger::new(source).expect("debugger");
        assert_eq!(debugger.stmt_to_line(0), 1);
        assert_eq!(debugger.stmt_to_line(1), 2);
        assert_eq!(debugger.stmt_to_line(2), 3);
    }
}
