use crate::ast::Stmt;
use crate::interpreter::Interpreter;
use crate::lexer::Lexer;
use crate::parser::Parser;
use crate::builtins::format_value;
use std::collections::HashMap;
use std::io::{self, Write};
use std::cell::RefCell;
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
    source_lines: Vec<String>,
    stmts: Vec<Stmt>,
    stmt_lines: Vec<usize>,  // Line number for each statement
    breakpoints: HashMap<usize, Breakpoint>,
    current_stmt: usize,
    in_debug: bool,
    at_breakpoint: bool,  // True when stopped at a breakpoint, ready to run current_stmt
    interpreter: Rc<RefCell<Interpreter>>,
}

impl Debugger {
    pub fn new(source: &str) -> Result<Self, String> {
        let lexer = Lexer::new(source);
        let tokens = lexer.tokenize().map_err(|e| e.message)?;
        let parser = Parser::new(tokens);
        let stmts = parser.parse().map_err(|e| e.message)?;
        
        let source_lines: Vec<String> = source.lines().map(|s| s.to_string()).collect();
        
        // Compute line number for each statement
        let stmt_lines = Self::compute_stmt_lines(&stmts, &source_lines);
        
        let interpreter = Rc::new(RefCell::new(Interpreter::new()));

        Ok(Self {
            source_lines,
            stmts,
            stmt_lines,
            breakpoints: HashMap::new(),
            current_stmt: 0,
            in_debug: false,
            at_breakpoint: false,
            interpreter,
        })
    }
    
    fn compute_stmt_lines(stmts: &[Stmt], _source_lines: &[String]) -> Vec<usize> {
        let mut lines = Vec::new();
        for (i, _stmt) in stmts.iter().enumerate() {
            // Best effort: use statement index + 1 as line number
            // This works well for simple one-statement-per-line code
            lines.push(i + 1);
        }
        lines
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
        let bytes_read = io::stdin().read_line(&mut input).map_err(|e| e.to_string())?;
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
                    self.interpreter.borrow_mut().run(&[stmt.clone()]).map_err(|e| e.message)?;
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
            
            if let Some(bp) = self.breakpoints.get(&line) {
                if bp.enabled {
                    println!("\nBreakpoint hit at line {}", line);
                    self.show_current_location();
                    self.in_debug = true;
                    self.at_breakpoint = true;
                    return Ok(());
                }
            }
            
            if let Some(stmt) = self.stmts.get(self.current_stmt) {
                self.interpreter.borrow_mut().run(&[stmt.clone()]).map_err(|e| e.message)?;
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
            self.interpreter.borrow_mut().run(&[stmt.clone()]).map_err(|e| e.message)?;
        }
        
        self.current_stmt += 1;
        
        // Check for breakpoint after step
        if self.current_stmt < self.stmts.len() {
            let new_line = self.stmt_to_line(self.current_stmt);
            if let Some(bp) = self.breakpoints.get(&new_line) {
                if bp.enabled {
                    println!("\nBreakpoint hit at line {}", new_line);
                    self.show_current_location();
                    self.in_debug = true;
                    self.at_breakpoint = true;
                    return Ok(());
                }
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
        self.stmt_lines.get(stmt_idx).copied().unwrap_or(stmt_idx + 1)
    }

    fn show_current_location(&self) {
        if self.current_stmt < self.stmts.len() {
            let line = self.stmt_to_line(self.current_stmt);
            if line > 0 && line <= self.source_lines.len() {
                println!("{:4}  {}", line, self.source_lines[line - 1]);
            }
        }
    }

    fn cmd_breakpoint(&mut self, line: usize) -> Result<(), String> {
        if line == 0 || line > self.source_lines.len() {
            return Err(format!("Invalid line number: {} (1-{})", line, self.source_lines.len()));
        }
        self.breakpoints.entry(line).or_insert(Breakpoint {
            line,
            enabled: true,
            hit_count: 0,
        });
        println!("Breakpoint set at line {}", line);
        Ok(())
    }

    fn cmd_remove_breakpoint(&mut self, line: usize) -> Result<(), String> {
        if self.breakpoints.remove(&line).is_some() {
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
    debug_source(&source)
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