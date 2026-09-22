//! Ahead-of-time compilation to C (`nect build`).
//!
//! The bytecode VM and its JIT are how a program is *run*. This module is how a
//! program is *shipped*: it translates the same bytecode into C, hands the
//! source to the system C compiler, and leaves a standalone executable behind
//! that needs neither Nect nor Cranelift at run time.
//!
//! # What gets compiled
//!
//! Translating to C needs a type for every value at compile time, so the
//! supported subset is the one that can be pinned down statically:
//!
//! * numbers and booleans (both `double`, as in the JIT) in locals, globals,
//!   parameters, and return values;
//! * strings **inside an expression only** — a string may be concatenated,
//!   compared, and printed, but it cannot be stored in a variable or passed
//!   across a call boundary;
//! * arithmetic, comparisons, truthiness, `if`/`while`/`for`, `break`,
//!   `continue`, function calls, and `return`;
//! * the builtins `print`, `println`, `str`, `abs`, `sqrt`, `floor`, `ceil`,
//!   `round`, `min`, `max`, `pow`, `sin`, `cos`, `tan`, and `log`.
//!
//! Anything else (arrays, modulo, builtins on non-numbers) reports the reason,
//! and the program stays on `nect run`, where the JIT handles it. Functions
//! nothing calls are skipped entirely, so an unused string helper does not stop
//! a numeric program from building.
//!
//! # Behavioural parity
//!
//! A built program must print exactly what the VM prints, including the text of
//! runtime errors. Three things carry that:
//!
//! * **Number formatting.** [`C_RUNTIME`] reproduces the VM's rule (whole
//!   values without a decimal point, otherwise the shortest decimal that
//!   round-trips, `nan`/`inf` spelled out) by searching for the shortest
//!   round-tripping representation and laying it out positionally.
//! * **Typed semantics.** Booleans are kept distinct from numbers in the
//!   translation, so `true == 1` stays `false` and `print(x > 0)` still prints
//!   `true`, exactly as the VM's tagged values behave.
//! * **Guarded failures.** A zero divisor, an out-of-domain `sqrt`/`log`, or
//!   reading a variable that never got a value print the VM's message to stderr
//!   and exit 1.
//!
//! `tests/aot_tests.rs` runs a corpus through both and compares byte for byte.

use crate::ast::{BinaryOp, UnaryOp, Value};
use crate::vm::{CallTarget, Fusee, Op, Program, TAG_CONST, TAG_GLOBAL, TAG_GLOBAL_CHECKED, TAG_LOCAL};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::Write as _;

/// Type of a value in the translated C.
///
/// Numbers and booleans are both `double` (0.0 and 1.0), matching the JIT, but
/// they stay *distinct types* here because the VM's values are tagged: `true ==
/// 1` is false, and `true` prints as `true`, not `1`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Ty {
    Num,
    Bool,
    Str,
}

impl Ty {
    fn c_type(self) -> &'static str {
        match self {
            Ty::Num | Ty::Bool => "double",
            Ty::Str => "NxStr",
        }
    }

    fn describe(self) -> &'static str {
        match self {
            Ty::Num => "a number",
            Ty::Bool => "a boolean",
            Ty::Str => "a string",
        }
    }
}

/// One entry on the simulated operand stack.
#[derive(Clone)]
struct Slot {
    ty: Ty,
    /// The C expression that computes this value.
    expr: String,
    /// Whether the expression is side-effect free and cheap to repeat, which is
    /// what makes it safe to inline and to duplicate in a guard.
    simple: bool,
}

impl Slot {
    fn new(ty: Ty, expr: impl Into<String>, simple: bool) -> Self {
        Self {
            ty,
            expr: expr.into(),
            simple,
        }
    }
}

/// Translates a compiled program into C.
///
/// Returns the full C source, or a human-readable reason why the program is
/// outside the subset this backend supports. A rejection is not a failure of
/// the program: it just means it has to run on the VM.
pub fn emit_c(program: &Program) -> Result<String, String> {
    // The module body runs first and declares the globals, so it is translated
    // before the functions that read them.
    let mut module = Body::new(program, "the module body", Mode::Module, HashMap::new());
    module.run(&program.instructions, &program.constants)?;
    let globals = module.globals.clone();

    // Only functions the module body can actually reach are translated, so an
    // unused helper outside the subset does not block the build.
    let reachable = reachable_functions(program);
    let mut functions = Vec::new();
    for index in &reachable {
        let function = &program.functions[*index as usize];
        let label = format!("function '{}'", program.interner.name(function.name));
        let mut body = Body::new(program, &label, Mode::Function, globals.clone());
        body.param_count = function.param_count;
        body.run(&function.instructions, &function.constants)?;
        functions.push((*index, function.param_count, body.finish()));
    }

    let mut out = String::from(C_RUNTIME);
    out.push('\n');

    let mut symbols: Vec<u32> = globals.keys().copied().collect();
    symbols.sort_unstable();
    for symbol in &symbols {
        let ty = globals[symbol];
        if ty == Ty::Str {
            return Err("stores a string in a global the C backend cannot represent".to_string());
        }
        let _ = writeln!(out, "static {} g{};", ty.c_type(), symbol);
    }
    // A module-level `let` inside an `if`/`while` body may never run, and
    // reading its name then has to fail the way the VM fails. Every global
    // therefore carries a definedness flag, set by its store and checked by the
    // loads that cannot be proven to follow one.
    for symbol in &symbols {
        let _ = writeln!(out, "static int d{} = 0;", symbol);
    }
    if !symbols.is_empty() {
        out.push('\n');
    }

    for (index, param_count, _) in &functions {
        let params: Vec<String> = (0..*param_count).map(|slot| format!("double p{}", slot)).collect();
        let _ = writeln!(out, "static double fn{}({});", index, params.join(", "));
    }
    if !functions.is_empty() {
        out.push('\n');
        for (index, param_count, body) in &functions {
            let params: Vec<String> = (0..*param_count).map(|slot| format!("double p{}", slot)).collect();
            let _ = writeln!(out, "static double fn{}({}) {{\n{}}}", index, params.join(", "), body);
            out.push('\n');
        }
    }

    let _ = writeln!(out, "int main(void) {{\n{}}}", module.finish());
    Ok(out)
}

/// Which functions the module body can reach, directly or transitively.
fn reachable_functions(program: &Program) -> Vec<u32> {
    let mut seen: HashSet<u32> = HashSet::new();
    let mut queue: Vec<u32> = Vec::new();
    let push = |seen: &mut HashSet<u32>, queue: &mut Vec<u32>, index: u32| {
        if seen.insert(index) {
            queue.push(index);
        }
    };
    for op in program.instructions.iter() {
        if let Op::Call(CallTarget::Function(index), _) = op {
            push(&mut seen, &mut queue, *index);
        }
    }
    while let Some(index) = queue.pop() {
        let Some(function) = program.functions.get(index as usize) else {
            continue;
        };
        for op in function.instructions.iter() {
            if let Op::Call(CallTarget::Function(callee), _) = op {
                push(&mut seen, &mut queue, *callee);
            }
        }
    }
    let mut result: Vec<u32> = seen.into_iter().collect();
    result.sort_unstable();
    result
}

/// How many values an instruction takes off the operand stack and how many it
/// leaves behind.
fn stack_effect(op: &Op) -> (usize, usize) {
    match op {
        Op::LoadConst(_) | Op::LoadLocal(_) | Op::LoadLocalChecked { .. } | Op::LoadGlobal(_) => {
            (0, 1)
        }
        Op::StoreLocal(_) | Op::StoreGlobal(_) => (1, 0),
        // `StoreKeep` writes the top of the stack without consuming it.
        Op::StoreKeep(_) => (0, 0),
        Op::BinaryOp(_) => (2, 1),
        Op::UnaryOp(_) => (1, 1),
        Op::BinaryFast { .. } => (0, 1),
        Op::BinaryStore { .. } => (0, 0),
        Op::JumpIfNot { .. } | Op::Jump(_) | Op::Halt => (0, 0),
        Op::JumpIfFalse(_) | Op::JumpIfTrue(_) => (1, 0),
        Op::Call(_, arg_count) => (*arg_count as usize, 1),
        Op::Return | Op::Pop => (1, 0),
        Op::MakeArray(count) => (*count as usize, 1),
        Op::MakeMap(count) => (*count as usize * 2, 1),
        Op::IterList => (1, 1),
        Op::ArrayLen => (1, 1),
        Op::LoadIndex => (2, 1),
        Op::StoreIndex | Op::StoreIndexOp(_) => (3, 1),
    }
}

/// Where an instruction continues to.
fn successors(op: &Op, index: usize, len: usize) -> Vec<usize> {
    let next = if index + 1 < len { vec![index + 1] } else { Vec::new() };
    match op {
        Op::Jump(target) => vec![*target as usize],
        Op::JumpIfFalse(target) | Op::JumpIfTrue(target) | Op::JumpIfNot { target, .. } => {
            let mut both = vec![*target as usize];
            both.extend(next);
            both
        }
        Op::Return | Op::Halt => Vec::new(),
        _ => next,
    }
}

/// The operand-stack depth on entry to every instruction, or `None` when the
/// instruction cannot run.
///
/// This is what makes the translation sound: the compiler reaches some
/// instructions (the merge points of `&&`, `||`, and `?:`) with a value already
/// on the stack, and the emitter has to know that before it walks the code
/// linearly. A merge whose two paths disagree about the depth is rejected,
/// because no single linear walk could describe it.
fn entry_depths(instructions: &[Op], label: &str) -> Result<Vec<Option<usize>>, String> {
    let mut depths: Vec<Option<usize>> = vec![None; instructions.len()];
    if instructions.is_empty() {
        return Ok(depths);
    }
    depths[0] = Some(0);
    let mut work = vec![0usize];
    while let Some(index) = work.pop() {
        let depth = depths[index].expect("queued instructions have a depth");
        let (pops, pushes) = stack_effect(&instructions[index]);
        if depth < pops {
            return Err(format!("{label} has an unbalanced operand stack"));
        }
        let next = depth - pops + pushes;
        for successor in successors(&instructions[index], index, instructions.len()) {
            if successor >= instructions.len() {
                return Err(format!("{label} jumps outside its own code"));
            }
            match depths[successor] {
                None => {
                    depths[successor] = Some(next);
                    work.push(successor);
                }
                Some(existing) if existing != next => {
                    return Err(format!(
                        "{label} joins two paths that disagree about the operand stack (at {successor}: {existing} vs {next})"
                    ))
                }
                Some(_) => {}
            }
        }
    }
    Ok(depths)
}

/// Whether a unit is a function body or the module body.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Function,
    Module,
}

/// Where a value is written.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Target {
    Local(u32),
    Global(u32),
    /// A global the compiler could not prove is declared, which the VM checks
    /// at run time and this backend declines.
    CheckedGlobal(u32),
    Constant,
}

impl Target {
    fn from_fusee(fusee: Fusee) -> Target {
        match fusee.tag() {
            TAG_LOCAL => Target::Local(fusee.index()),
            TAG_GLOBAL => Target::Global(fusee.index()),
            TAG_GLOBAL_CHECKED => Target::CheckedGlobal(fusee.index()),
            _ => Target::Constant,
        }
    }
}

struct Body<'a> {
    program: &'a Program,
    /// Global types, learned by the module body and then shared with functions.
    globals: HashMap<u32, Ty>,
    /// Types of the local slots this unit wrote.
    slots: Vec<Option<Ty>>,
    /// Slots some instruction writes, so a read of a never-written slot is
    /// caught rather than reading uninitialised C.
    written: HashSet<u32>,
    /// Slots read through `LoadLocalChecked`, which need a runtime flag because
    /// their declaration may not have run.
    checked: HashSet<u32>,
    /// Operand-stack depth on entry to each instruction (`None` = unreachable).
    entry: Vec<Option<usize>>,
    /// Whether the previous instruction can fall into this one, which decides
    /// whether the current stack describes its inputs.
    fallthrough: Vec<bool>,
    /// The C variable that carries a partially-computed join, keyed by
    /// (instruction, stack position). Two paths reach a merge point holding a
    /// value the emitter must reconcile; both paths assign into this variable.
    canonicals: BTreeMap<(usize, usize), (Ty, String)>,
    stack: Vec<Slot>,
    out: String,
    temp: usize,
    param_count: u32,
    label: String,
    mode: Mode,
}

impl<'a> Body<'a> {
    fn new(
        program: &'a Program,
        label: &str,
        mode: Mode,
        globals: HashMap<u32, Ty>,
    ) -> Self {
        Self {
            program,
            globals,
            slots: Vec::new(),
            written: HashSet::new(),
            checked: HashSet::new(),
            entry: Vec::new(),
            fallthrough: Vec::new(),
            canonicals: BTreeMap::new(),
            stack: Vec::new(),
            out: String::new(),
            temp: 0,
            param_count: 0,
            label: label.to_string(),
            mode,
        }
    }

    /// Emits the unit and returns its declarations followed by its statements.
    fn finish(mut self) -> String {
        let mut header = String::new();
        for slot in self.param_count as usize..self.slots.len() {
            if let Some(ty) = self.slots[slot] {
                let _ = writeln!(header, "    {} s{} = 0.0;", ty.c_type(), slot);
            }
        }
        let mut flags: Vec<u32> = self.checked.iter().copied().collect();
        flags.sort_unstable();
        for slot in flags {
            let _ = writeln!(header, "    int w{} = 0;", slot);
        }
        // Join variables are declared up front: a backward jump can reach its
        // merge point before the linear walk reaches the branch that feeds it.
        for ((index, position), (ty, name)) in &self.canonicals {
            let _ = name;
            let _ = index;
            let _ = position;
            match ty {
                Ty::Str => {
                    let _ = writeln!(header, "    NxStr {} = nx_text(\"\");", name);
                }
                _ => {
                    let _ = writeln!(header, "    {} {} = 0.0;", ty.c_type(), name);
                }
            }
        }
        if header.is_empty() {
            header.push_str("    (void)0;\n");
        }
        // Declarations go at the top of the body, statements follow them.
        self.out.insert_str(0, &header);
        self.out
    }

    fn line(&mut self, text: &str) {
        let _ = writeln!(self.out, "    {}", text);
    }

    fn temp(&mut self) -> String {
        self.temp += 1;
        format!("t{}", self.temp)
    }

    fn run(&mut self, instructions: &[Op], constants: &[Value]) -> Result<(), String> {
        self.slots
            .resize(self.slot_count(instructions) as usize, None);
        for slot in 0..self.param_count {
            self.written.insert(slot);
            self.slots[slot as usize] = Some(Ty::Num);
        }
        let mut targets: HashSet<u32> = HashSet::new();
        for op in instructions.iter() {
            if let Some(target) = op.jump_target() {
                if target as usize > instructions.len() {
                    return Err(self.reason("jumps to a place outside its own code"));
                }
                targets.insert(target);
            }
            if let Op::LoadLocalChecked { slot, .. } = op {
                self.checked.insert(*slot);
            }
            if let Op::StoreLocal(slot) = op {
                self.written.insert(*slot);
            }
        }

        self.entry = entry_depths(instructions, &self.label)?;
        self.fallthrough = (0..instructions.len())
            .map(|position| {
                if position == 0 {
                    return true;
                }
                successors(&instructions[position - 1], position - 1, instructions.len())
                    .contains(&position)
            })
            .collect();

        for (position, op) in instructions.iter().enumerate() {
            let Some(depth) = self.entry[position] else {
                // Unreachable (for example the `return null` the compiler
                // appends after a body that always returns).
                continue;
            };
            // Instructions can be entered holding a value the emitter has to
            // reconcile: the stack here is whatever the previous instruction
            // left, or the join variables a branch assigned on its way in.
            // The assignments come *before* the label, so a branch that already
            // assigned the join variable skips them.
            if depth == 0 {
                self.stack.clear();
            } else if self.fallthrough[position] {
                self.materialise_join(position)?;
            } else {
                self.adopt_join(position, depth)?;
            }
            if targets.contains(&(position as u32)) {
                let _ = writeln!(self.out, "L{}: ;", position);
            }
            if self.stack.len() != depth {
                return Err(self.reason("could not follow its operand stack"));
            }
            self.instruction(op, constants)?;
        }
        Ok(())
    }

    /// Takes the join variables of `index` as the current stack, for an
    /// instruction reachable only by branches.
    fn adopt_join(&mut self, index: usize, depth: usize) -> Result<(), String> {
        for position in 0..depth {
            let Some((ty, name)) = self.canonicals.get(&(index, position)) else {
                return Err(self.reason("could not follow its operand stack"));
            };
            let (ty, name) = (*ty, name.clone());
            if self.stack.len() <= position {
                self.stack.push(Slot::new(ty, name, true));
            } else {
                self.stack[position] = Slot::new(ty, name, true);
            }
        }
        Ok(())
    }

    /// Assigns the current stack into the join variables of `index` and adopts
    /// those variables as the stack, so both paths that meet there agree.
    fn materialise_join(&mut self, index: usize) -> Result<(), String> {
        let depth = self.entry.get(index).copied().flatten().unwrap_or(0);
        if depth == 0 {
            return Ok(());
        }
        if depth > self.stack.len() {
            return Err(self.reason("has an unbalanced operand stack at a join"));
        }
        for position in 0..depth {
            let value = self.stack[position].clone();
            let name = match self.canonicals.get(&(index, position)) {
                Some((expected, name)) => {
                    if *expected != value.ty {
                        return Err(
                            self.reason("joins two paths that disagree about a value's kind")
                        );
                    }
                    name.clone()
                }
                None => {
                    let name = format!("m{}_{}", index, position);
                    self.canonicals
                        .insert((index, position), (value.ty, name.clone()));
                    name
                }
            };
            self.line(&format!("{} = {};", name, value.expr));
            self.stack[position] = Slot::new(value.ty, name, true);
        }
        Ok(())
    }

    fn slot_count(&self, instructions: &[Op]) -> u32 {
        // Slots are numbered densely by the compiler, so the highest one
        // mentioned bounds the frame. Parameters are slots too.
        let mut count = self.param_count;
        for op in instructions {
            let note = |slot: u32, count: &mut u32| {
                if slot + 1 > *count {
                    *count = slot + 1;
                }
            };
            match op {
                Op::LoadLocal(slot) | Op::StoreLocal(slot) | Op::LoadLocalChecked { slot, .. } => {
                    note(*slot, &mut count)
                }
                Op::StoreKeep(fusee) if fusee.tag() == TAG_LOCAL => {
                    note(fusee.index(), &mut count)
                }
                Op::BinaryStore { dst, .. } if dst.tag() == TAG_LOCAL => {
                    note(dst.index(), &mut count)
                }
                _ => {}
            }
        }
        if self.mode == Mode::Module {
            count.max(self.program.module_slots)
        } else {
            count
        }
    }

    fn reason(&self, text: &str) -> String {
        format!("{} {}", self.label, text)
    }

    fn push(&mut self, ty: Ty, expr: impl Into<String>, simple: bool) {
        self.stack.push(Slot::new(ty, expr, simple));
    }

    fn pop(&mut self, what: &str) -> Result<Slot, String> {
        match self.stack.pop() {
            Some(slot) => Ok(slot),
            None => Err(self.reason(&format!("has an unbalanced operand stack at {what}"))),
        }
    }

    /// Resolves a fused operand (a local slot, a global, or a constant).
    fn operand(&mut self, operand: Fusee, constants: &[Value]) -> Result<Slot, String> {
        match operand.tag() {
            TAG_LOCAL => {
                let slot = operand.index();
                if !self.written.contains(&slot) {
                    return Err(self.reason("reads a variable nothing assigns"));
                }
                let ty = self
                    .slots
                    .get(slot as usize)
                    .copied()
                    .flatten()
                    .ok_or_else(|| self.reason("reads a variable whose kind it cannot tell"))?;
                Ok(Slot::new(ty, self.slot_expr(slot), true))
            }
            TAG_GLOBAL => {
                let symbol = operand.index();
                match self.globals.get(&symbol).copied() {
                    Some(ty) => Ok(Slot::new(ty, format!("g{}", symbol), true)),
                    None => {
                        let name = self.program.interner.name(symbol).to_string();
                        let message = if crate::builtins::NAMES.contains(&name.as_str()) {
                            format!("cannot use '{}' as a value (it is a function)", name)
                        } else {
                            format!("undefined variable '{}'", name)
                        };
                        Ok(Slot::new(
                            Ty::Num,
                            format!("(nx_fail({}), 0.0)", c_string(&message)),
                            false,
                        ))
                    }
                }
            }
            TAG_CONST => match constants.get(operand.index() as usize) {
                Some(Value::Number(n)) => Ok(Slot::new(Ty::Num, c_double(*n), true)),
                Some(Value::Boolean(b)) => Ok(Slot::new(Ty::Bool, c_bool(*b), true)),
                Some(Value::String(s)) => {
                    let text = self.string_literal(s);
                    Ok(Slot::new(Ty::Str, text, true))
                }
                _ => Err(self.reason("reads a constant the C backend cannot represent")),
            },
            TAG_GLOBAL_CHECKED => Err(self.reason(
                "assigns to a name the compiler could not prove is declared",
            )),
            _ => Err(self.reason("assigns to a constant")),
        }
    }

    fn slot_expr(&self, slot: u32) -> String {
        if slot < self.param_count {
            format!("p{}", slot)
        } else {
            format!("s{}", slot)
        }
    }

    /// Materialises a string constant into a temporary of its own, so releasing
    /// it is always safe.
    fn string_literal(&mut self, text: &str) -> String {
        let name = self.temp();
        self.line(&format!("NxStr {} = nx_text({});", name, c_string(text)));
        name
    }

    fn store(&mut self, target: Target, value: Slot) -> Result<(), String> {
        match target {
            Target::Local(slot) => {
                if slot as usize >= self.slots.len() {
                    self.slots.resize(slot as usize + 1, None);
                }
                if value.ty == Ty::Str {
                    return Err(self.reason(
                        "stores a string in a variable, which the C backend cannot represent",
                    ));
                }
                match self.slots[slot as usize] {
                    Some(existing) if existing != value.ty => {
                        return Err(self.reason("stores two kinds of value in one variable"));
                    }
                    _ => self.slots[slot as usize] = Some(value.ty),
                }
                self.written.insert(slot);
                if self.checked.contains(&slot) {
                    self.line(&format!("w{} = 1;", slot));
                }
                self.line(&format!("{} = {};", self.slot_expr(slot), value.expr));
            }
            Target::Global(symbol) => {
                if value.ty == Ty::Str {
                    return Err(self.reason(
                        "stores a string in a global, which the C backend cannot represent",
                    ));
                }
                match self.globals.get(&symbol).copied() {
                    Some(existing) if existing != value.ty => {
                        return Err(self.reason("stores two kinds of value in one global"));
                    }
                    Some(_) => {}
                    None if self.mode == Mode::Module => {
                        self.globals.insert(symbol, value.ty);
                    }
                    None => {
                        return Err(self.reason(
                            "assigns a name the compiler could not prove is declared",
                        ));
                    }
                }
                self.line(&format!("g{} = {};", symbol, value.expr));
                self.line(&format!("d{} = 1;", symbol));
            }
            Target::CheckedGlobal(symbol) => {
                // The compiler could not prove this name is declared, so the VM
                // fails here when it is not. Failing the same way keeps the
                // built program's message identical.
                let name = self.program.interner.name(symbol).to_string();
                self.line(&format!(
                    "nx_fail({});",
                    c_string(&format!("undefined variable '{}'", name))
                ));
            }
            Target::Constant => return Err(self.reason("assigns to a constant")),
        }
        Ok(())
    }

    fn instruction(&mut self, op: &Op, constants: &[Value]) -> Result<(), String> {
        match op {
            Op::LoadConst(index) => {
                let value = constants
                    .get(*index as usize)
                    .ok_or_else(|| self.reason("reads a constant that does not exist"))?;
                match value {
                    Value::Number(n) => self.push(Ty::Num, c_double(*n), true),
                    Value::Boolean(b) => self.push(Ty::Bool, c_bool(*b), true),
                    Value::String(s) => {
                        let text = self.string_literal(s);
                        self.push(Ty::Str, text, true);
                    }
                    Value::Null => {
                        // A reachable `null` means a function can fall off its
                        // end without returning, which the VM turns into a null
                        // result and (usually) a later type error.
                        return Err(self.reason(
                            "can finish without a value where one is needed (the VM returns null there)",
                        ));
                    }
                    _ => {
                        return Err(self.reason(
                            "uses a value the C backend cannot represent (arrays or functions)",
                        ))
                    }
                }
            }
            Op::LoadLocal(slot) => {
                if !self.written.contains(slot) {
                    return Err(self.reason("reads a variable nothing assigns"));
                }
                let ty = self
                    .slots
                    .get(*slot as usize)
                    .copied()
                    .flatten()
                    .ok_or_else(|| self.reason("reads a variable whose kind it cannot tell"))?;
                self.push(ty, self.slot_expr(*slot), true);
            }
            Op::LoadLocalChecked { slot, name } => {
                let ty = self
                    .slots
                    .get(*slot as usize)
                    .copied()
                    .flatten()
                    .ok_or_else(|| self.reason("reads a variable whose kind it cannot tell"))?;
                self.line(&format!(
                    "if (!w{}) {{ nx_fail({}); }}",
                    slot,
                    c_string(&format!("undefined variable '{}'", self.program.interner.name(*name)))
                ));
                self.push(ty, self.slot_expr(*slot), true);
            }
            Op::StoreLocal(slot) => {
                let value = self.pop("a local store")?;
                self.store(Target::Local(*slot), value)?;
            }
            Op::StoreKeep(target) => {
                let value = self
                    .stack
                    .last()
                    .cloned()
                    .ok_or_else(|| self.reason("has an unbalanced operand stack at a store"))?;
                self.store(Target::from_fusee(*target), value)?;
            }
            Op::LoadGlobal(symbol) => match self.globals.get(symbol).copied() {
                Some(ty) => {
                    let name = self.program.interner.name(*symbol).to_string();
                    let message = if crate::builtins::NAMES.contains(&name.as_str()) {
                        format!("cannot use '{}' as a value (it is a function)", name)
                    } else {
                        format!("undefined variable '{}'", name)
                    };
                    self.line(&format!(
                        "if (!d{}) {{ nx_fail({}); }}",
                        symbol,
                        c_string(&message)
                    ));
                    self.push(ty, format!("g{}", symbol), true);
                }
                // Reading a name that was never given a value is a runtime error
                // in the VM, so it is one here too (with the same wording).
                None => {
                    let name = self.program.interner.name(*symbol).to_string();
                    let message = if crate::builtins::NAMES.contains(&name.as_str()) {
                        format!("cannot use '{}' as a value (it is a function)", name)
                    } else {
                        format!("undefined variable '{}'", name)
                    };
                    self.push(
                        Ty::Num,
                        format!("(nx_fail({}), 0.0)", c_string(&message)),
                        false,
                    );
                }
            },
            Op::StoreGlobal(symbol) => {
                let value = self.pop("a global store")?;
                self.store(Target::Global(*symbol), value)?;
            }
            Op::BinaryOp(operator) => {
                let right = self.pop("an operator")?;
                let left = self.pop("an operator")?;
                self.binary(*operator, left, right)?;
            }
            Op::BinaryFast { op: operator, lhs, rhs } => {
                let left = self.operand(*lhs, constants)?;
                let right = self.operand(*rhs, constants)?;
                self.binary(*operator, left, right)?;
            }
            Op::BinaryStore { op: operator, dst, lhs, rhs } => {
                let left = self.operand(*lhs, constants)?;
                let right = self.operand(*rhs, constants)?;
                let before = self.stack.len();
                self.binary(*operator, left, right)?;
                let value = self.pop("a fused store")?;
                debug_assert_eq!(self.stack.len(), before);
                self.store(Target::from_fusee(*dst), value)?;
            }
            Op::UnaryOp(operator) => {
                let value = self.pop("a unary operator")?;
                self.unary(*operator, value)?;
            }
            Op::JumpIfNot { op: operator, lhs, rhs, target } => {
                let left = self.operand(*lhs, constants)?;
                let right = self.operand(*rhs, constants)?;
                let condition = self.condition(*operator, left, right)?;
                self.materialise_join(*target as usize)?;
                self.line(&format!("if (!({})) goto L{};", condition, target));
            }
            Op::JumpIfFalse(target) => {
                let value = self.pop("a branch")?;
                self.materialise_join(*target as usize)?;
                match value.ty {
                    Ty::Str => {} // a string is always truthy
                    _ => self.line(&format!("if (({}) == 0.0) goto L{};", value.expr, target)),
                }
            }
            Op::JumpIfTrue(target) => {
                let value = self.pop("a branch")?;
                self.materialise_join(*target as usize)?;
                match value.ty {
                    Ty::Str => self.line(&format!("goto L{};", target)),
                    _ => self.line(&format!("if (({}) != 0.0) goto L{};", value.expr, target)),
                }
            }
            Op::Jump(target) => {
                self.materialise_join(*target as usize)?;
                self.line(&format!("goto L{};", target));
            }
            Op::Call(target, arg_count) => self.call(target, *arg_count as usize)?,
            Op::Return => {
                if self.mode != Mode::Function {
                    return Err(self.reason("returns from module level"));
                }
                let value = self.pop("a return")?;
                match value.ty {
                    Ty::Str => return Err(self.reason("returns a string, which the C backend cannot represent")),
                    _ => self.line(&format!("return {};", value.expr)),
                }
            }
            Op::Pop => {
                let value = self.pop("a discarded expression")?;
                if value.ty == Ty::Str && value.simple {
                    self.line(&format!("nx_release(&{});", value.expr));
                } else if is_join_variable(&value.expr) {
                    // A join variable that is only ever discarded would otherwise
                    // trip `-Wunused-but-set-variable` in the generated C.
                    self.line(&format!("(void){};", value.expr));
                }
            }
            Op::Halt => self.line("return 0;"),
            other => {
                return Err(self.reason(&format!(
                    "uses {} which the C backend cannot translate",
                    describe_instruction(other)
                )))
            }
        }
        Ok(())
    }

    /// Emits `left op right` and pushes the result.
    fn binary(&mut self, op: BinaryOp, left: Slot, right: Slot) -> Result<(), String> {
        // Equality between different kinds of value is a definite `false` in the
        // VM (its values are tagged), so it does not need runtime code.
        if matches!(op, BinaryOp::Equal | BinaryOp::NotEqual) && left.ty != right.ty {
            let result = if op == BinaryOp::Equal { "0.0" } else { "1.0" };
            self.push(Ty::Bool, result, true);
            return Ok(());
        }

        match op {
            BinaryOp::Add if left.ty == Ty::Str && right.ty == Ty::Str => {
                let name = self.temp();
                self.line(&format!(
                    "NxStr {} = nx_concat({}, {});",
                    name,
                    self.owned(&left),
                    self.owned(&right)
                ));
                self.push(Ty::Str, name, true);
            }
            BinaryOp::Add if left.ty == Ty::Str || right.ty == Ty::Str => {
                return Err(self.reason(
                    "applies '+' to a string and something that is not a string",
                ));
            }
            BinaryOp::Add
            | BinaryOp::Subtract
            | BinaryOp::Multiply
            | BinaryOp::Divide
            | BinaryOp::Modulo => {
                self.number(&left, op)?;
                self.number(&right, op)?;
                let symbol = match op {
                    BinaryOp::Add => "+",
                    BinaryOp::Subtract => "-",
                    BinaryOp::Multiply => "*",
                    BinaryOp::Divide => "/",
                    _ => "fmod",
                };
                match op {
                    // The VM reports a zero divisor at run time; so does this.
                    BinaryOp::Divide | BinaryOp::Modulo => {
                        let divisor = self.inline_or_temp(&right);
                        let message = if op == BinaryOp::Divide {
                            "division by zero"
                        } else {
                            "modulo by zero"
                        };
                        self.line(&format!(
                            "if (({}) == 0.0) {{ nx_fail({}); }}",
                            divisor,
                            c_string(message)
                        ));
                        let expression = if op == BinaryOp::Divide {
                            format!("(({}) / ({}))", left.expr, divisor)
                        } else {
                            // Rust's `%` on f64 is fmod, so this matches exactly.
                            format!("fmod({}, {})", left.expr, divisor)
                        };
                        let _ = symbol;
                        self.push(Ty::Num, expression, false);
                    }
                    _ => self.push(
                        Ty::Num,
                        format!("(({}) {} ({}))", left.expr, symbol, right.expr),
                        left.simple && right.simple,
                    ),
                }
            }
            BinaryOp::Less
            | BinaryOp::Greater
            | BinaryOp::LessEqual
            | BinaryOp::GreaterEqual => {
                let condition = self.ordering(op, left, right)?;
                self.push(Ty::Bool, format!("(({}) ? 1.0 : 0.0)", condition), false);
            }
            BinaryOp::Equal | BinaryOp::NotEqual => {
                if left.ty == Ty::Str {
                    let name = self.temp();
                    let symbol = if op == BinaryOp::Equal { "==" } else { "!=" };
                    self.line(&format!(
                        "double {} = (strcmp({}.data, {}.data) {} 0) ? 1.0 : 0.0;",
                        name,
                        self.owned(&left),
                        self.owned(&right),
                        symbol
                    ));
                    self.push(Ty::Bool, name, true);
                } else {
                    let symbol = if op == BinaryOp::Equal { "==" } else { "!=" };
                    self.push(
                        Ty::Bool,
                        format!("((({}) {} ({})) ? 1.0 : 0.0)", left.expr, symbol, right.expr),
                        left.simple && right.simple,
                    );
                }
            }
            BinaryOp::And | BinaryOp::Or => {
                return Err(self.reason(
                    "uses && or ||, whose merge point the C backend does not translate",
                ))
            }
        }
        Ok(())
    }

    fn unary(&mut self, op: UnaryOp, value: Slot) -> Result<(), String> {
        match op {
            UnaryOp::Negate => match value.ty {
                Ty::Num => self.push(Ty::Num, format!("-({})", value.expr), value.simple),
                other => return Err(self.reason(&format!("negates {}", other.describe()))),
            },
            UnaryOp::Not => {
                let result = match value.ty {
                    Ty::Str => "0.0".to_string(),
                    _ => format!("((({}) == 0.0) ? 1.0 : 0.0)", value.expr),
                };
                self.push(Ty::Bool, result, value.simple);
            }
        }
        Ok(())
    }

    /// A C condition for a comparison, used by `JumpIfNot`.
    fn condition(&self, op: BinaryOp, left: Slot, right: Slot) -> Result<String, String> {
        match op {
            BinaryOp::Equal | BinaryOp::NotEqual if left.ty != right.ty => {
                // Different kinds of value are never equal in the VM.
                Ok(if op == BinaryOp::Equal { "1 == 0" } else { "1 == 1" }.to_string())
            }
            BinaryOp::Equal | BinaryOp::NotEqual if left.ty == Ty::Str => Err(self.reason(
                "compares strings in a fused branch, which the C backend does not translate",
            )),
            BinaryOp::Equal | BinaryOp::NotEqual => {
                let symbol = if op == BinaryOp::Equal { "==" } else { "!=" };
                Ok(format!("({}) {} ({})", left.expr, symbol, right.expr))
            }
            _ => self.ordering(op, left, right),
        }
    }

    /// A C condition for an ordering comparison.
    fn ordering(&self, op: BinaryOp, left: Slot, right: Slot) -> Result<String, String> {
        let symbol = match op {
            BinaryOp::Less => "<",
            BinaryOp::Greater => ">",
            BinaryOp::LessEqual => "<=",
            BinaryOp::GreaterEqual => ">=",
            _ => return Err(self.reason("uses a logic operator where a comparison belongs")),
        };
        match (left.ty, right.ty) {
            (Ty::Num, Ty::Num) => Ok(format!("({}) {} ({})", left.expr, symbol, right.expr)),
            (Ty::Str, Ty::Str) => Err(self.reason(
                "compares strings by ordering, which the C backend does not translate",
            )),
            (a, b) => Err(self.reason(&format!(
                "compares {} with {}",
                a.describe(),
                b.describe()
            ))),
        }
    }

    fn number(&self, slot: &Slot, op: BinaryOp) -> Result<(), String> {
        match slot.ty {
            Ty::Num => Ok(()),
            other => Err(self.reason(&format!(
                "applies {} to {}",
                operator_symbol(op),
                other.describe()
            ))),
        }
    }

    /// Returns an expression that can be evaluated more than once, giving a
    /// complex divisor a temporary so it is computed exactly once.
    fn inline_or_temp(&mut self, slot: &Slot) -> String {
        if slot.simple {
            return slot.expr.clone();
        }
        let name = self.temp();
        self.line(&format!("double {} = {};", name, slot.expr));
        name
    }

    /// The expression to pass to a consuming string operation.
    fn owned(&self, slot: &Slot) -> String {
        slot.expr.clone()
    }

    fn call(&mut self, target: &CallTarget, arg_count: usize) -> Result<(), String> {
        let mut arguments: Vec<Slot> = Vec::with_capacity(arg_count);
        for _ in 0..arg_count {
            arguments.push(self.pop("a call")?);
        }
        arguments.reverse();

        match target {
            CallTarget::Function(index) => {
                for argument in &arguments {
                    if argument.ty != Ty::Num {
                        return Err(self.reason(&format!(
                            "passes {} to a function, which the C backend expects to be a number",
                            argument.ty.describe()
                        )));
                    }
                }
                let rendered: Vec<String> = arguments.iter().map(|slot| slot.expr.clone()).collect();
                let name = self.temp();
                // Materialised immediately, so calls keep the bytecode's
                // evaluation order.
                self.line(&format!("double {} = fn{}({});", name, index, rendered.join(", ")));
                self.push(Ty::Num, name, true);
            }
            CallTarget::Native(symbol) => {
                let name = self.program.interner.name(*symbol).to_string();
                self.native(&name, arguments)?;
            }
        }
        Ok(())
    }

    fn native(&mut self, name: &str, arguments: Vec<Slot>) -> Result<(), String> {
        match name {
            "print" | "println" => {
                for (index, argument) in arguments.iter().enumerate() {
                    if index > 0 {
                        self.line("nx_space();");
                    }
                    match argument.ty {
                        Ty::Str => {
                            self.line(&format!("nx_print_str(&{});", argument.expr));
                            self.line(&format!("nx_release(&{});", argument.expr));
                        }
                        Ty::Bool => self.line(&format!("nx_print_bool({});", argument.expr)),
                        _ => self.line(&format!("nx_print_double({});", argument.expr)),
                    }
                }
                self.line("nx_newline();");
                self.push(Ty::Num, "0.0", true);
            }
            "str" => {
                match arguments.first() {
                    Some(argument) if argument.ty == Ty::Str => {
                        let value = argument.expr.clone();
                        self.push(Ty::Str, value, argument.simple);
                    }
                    Some(argument) if argument.ty == Ty::Bool => {
                        let value = format!("nx_bool_text({})", argument.expr);
                        self.push(Ty::Str, value, false);
                    }
                    Some(argument) => {
                        let value = format!("nx_from_double({})", argument.expr);
                        self.push(Ty::Str, value, false);
                    }
                    None => {
                        let value = self.string_literal("");
                        self.push(Ty::Str, value, true);
                    }
                }
            }
            "abs" | "sqrt" | "floor" | "ceil" | "round" | "sin" | "cos" | "tan" | "log" => {
                let argument = self.numeric_argument(name, &arguments)?;
                self.push(Ty::Num, format!("nx_{}({})", name, argument), false);
            }
            "pow" => {
                if arguments.len() != 2 {
                    return Err(self.reason("calls pow() without exactly two numbers"));
                }
                let left = self.numeric_argument(name, &arguments[0..1])?;
                let right = self.numeric_argument(name, &arguments[1..2])?;
                self.push(Ty::Num, format!("pow({}, {})", left, right), false);
            }
            "min" | "max" => {
                if arguments.is_empty() {
                    return Err(self.reason(&format!("calls {}() with nothing to compare", name)));
                }
                // Folded pairwise with the VM's loop, so NaN behaves the same.
                let mut folded = self.numeric_argument(name, &arguments[0..1])?;
                for argument in &arguments[1..] {
                    let next = self.numeric_argument(name, std::slice::from_ref(argument))?;
                    folded = format!("nx_{}2({}, {})", name, folded, next);
                }
                self.push(Ty::Num, folded, false);
            }
            other => {
                return Err(self.reason(&format!(
                    "calls {}(), which the C backend does not translate",
                    other
                )))
            }
        };
        Ok(())
    }

    fn numeric_argument(&self, name: &str, arguments: &[Slot]) -> Result<String, String> {
        match arguments.first() {
            Some(argument) if argument.ty == Ty::Num => Ok(argument.expr.clone()),
            Some(argument) => Err(self.reason(&format!(
                "calls {}() with {}, which the C backend cannot type",
                name,
                argument.ty.describe()
            ))),
            None => Err(self.reason(&format!("calls {}() without an argument", name))),
        }
    }
}

/// Whether an expression is one of the join variables the emitter creates.
fn is_join_variable(expression: &str) -> bool {
    let Some(rest) = expression.strip_prefix('m') else {
        return false;
    };
    let Some((index, position)) = rest.split_once('_') else {
        return false;
    };
    !index.is_empty()
        && !position.is_empty()
        && index.bytes().all(|b| b.is_ascii_digit())
        && position.bytes().all(|b| b.is_ascii_digit())
}

fn c_bool(value: bool) -> &'static str {
    if value {
        "1.0"
    } else {
        "0.0"
    }
}

fn operator_symbol(op: BinaryOp) -> &'static str {
    match op {
        BinaryOp::Add => "+",
        BinaryOp::Subtract => "-",
        BinaryOp::Multiply => "*",
        BinaryOp::Divide => "/",
        BinaryOp::Modulo => "%",
        BinaryOp::Less => "<",
        BinaryOp::Greater => ">",
        BinaryOp::LessEqual => "<=",
        BinaryOp::GreaterEqual => ">=",
        BinaryOp::Equal => "==",
        BinaryOp::NotEqual => "!=",
        BinaryOp::And => "&&",
        BinaryOp::Or => "||",
    }
}

fn describe_instruction(op: &Op) -> &'static str {
    match op {
        Op::MakeArray(_) => "an array literal",
        Op::MakeMap(_) => "a map literal",
        Op::IterList => "map iteration",
        Op::ArrayLen => "an array length",
        Op::LoadIndex => "indexing",
        Op::StoreIndex | Op::StoreIndexOp(_) => "indexed assignment",
        Op::LoadLocalChecked { .. } => "a conditional declaration",
        _ => "an instruction",
    }
}

/// A number as a C literal that round-trips to the same double.
fn c_double(value: f64) -> String {
    if value.is_nan() {
        return "(0.0 / 0.0)".to_string();
    }
    if value.is_infinite() {
        return if value > 0.0 {
            "(1.0 / 0.0)".to_string()
        } else {
            "(-1.0 / 0.0)".to_string()
        };
    }
    // `{:?}` is Rust's shortest round-tripping form, which C parses back
    // exactly.
    let text = format!("{:?}", value);
    if text.contains('.') || text.contains('e') || text.contains('E') {
        text
    } else {
        format!("{}.0", text)
    }
}

/// A Rust string as a C literal, preserving its UTF-8 bytes.
fn c_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

/// The C runtime every generated program links against.
///
/// Deliberately small: value formatting that matches the VM character for
/// character, string building for `str(...)` concatenation, guarded maths with
/// the VM's error wording, and the failure exit path.
pub const C_RUNTIME: &str = r#"/* Generated by `nect build`. */
#include <math.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

/* A program uses only the helpers it needs, so the rest of this runtime is
   deliberately left unused: compiling it with -Wall stays quiet. */
#if defined(__GNUC__) || defined(__clang__)
#define NX_UNUSED __attribute__((unused))
#else
#define NX_UNUSED
#endif

/* Portable strdup: MSVC uses _strdup, POSIX uses strdup */
#if defined(_MSC_VER)
#define nx_strdup _strdup
#else
#define nx_strdup strdup
#endif

typedef struct {
    char *data;
    int owned;
} NxStr;

NX_UNUSED static void nx_fail(const char *message) {
    fprintf(stderr, "error: %s\n", message);
    exit(1);
}

/* Formats a double exactly the way the VM prints it: whole values without a
   decimal point, otherwise the shortest decimal that round-trips, and nan/inf
   spelled out. The shortest round-tripping representation is found by widening
   the significant digits, then laid out positionally (Rust's `{}` never uses
   exponent notation). */
NX_UNUSED static void nx_format_double(char *out, size_t cap, double value) {
    if (isnan(value)) { snprintf(out, cap, "nan"); return; }
    if (isinf(value)) { snprintf(out, cap, value > 0 ? "inf" : "-inf"); return; }
    if (value == 0.0) { snprintf(out, cap, "0"); return; }

    char scientific[32];
    for (int digits = 1; digits <= 17; digits++) {
        snprintf(scientific, sizeof scientific, "%.*e", digits - 1, value);
        if (strtod(scientific, NULL) == value) break;
    }

    const char *cursor = scientific;
    int negative = 0;
    if (*cursor == '-') { negative = 1; cursor++; }
    char digits[24];
    int count = 0;
    while (*cursor != 'e' && *cursor != 'E' && *cursor != '\0') {
        if (*cursor >= '0' && *cursor <= '9') digits[count++] = *cursor;
        cursor++;
    }
    int exponent = 0;
    if (*cursor == 'e' || *cursor == 'E') exponent = (int)strtol(cursor + 1, NULL, 10);
    int integer_digits = exponent + 1;

    char buffer[400];
    size_t at = 0;
    if (negative) buffer[at++] = '-';
    if (integer_digits <= 0) {
        buffer[at++] = '0';
        buffer[at++] = '.';
        for (int i = 0; i < -integer_digits; i++) buffer[at++] = '0';
        for (int i = 0; i < count; i++) buffer[at++] = digits[i];
    } else if (integer_digits >= count) {
        for (int i = 0; i < count; i++) buffer[at++] = digits[i];
        for (int i = 0; i < integer_digits - count; i++) buffer[at++] = '0';
    } else {
        for (int i = 0; i < integer_digits; i++) buffer[at++] = digits[i];
        buffer[at++] = '.';
        for (int i = integer_digits; i < count; i++) buffer[at++] = digits[i];
    }
    buffer[at] = '\0';
    snprintf(out, cap, "%s", buffer);
}

NX_UNUSED static NxStr nx_text(const char *literal) {
    NxStr text;
    text.data = (char *)literal;
    text.owned = 0;
    return text;
}

NX_UNUSED static void nx_release(NxStr *text) {
    if (text->owned) {
        free(text->data);
        text->data = (char *)"";
        text->owned = 0;
    }
}

NX_UNUSED static NxStr nx_from_double(double value) {
    char buffer[400];
    nx_format_double(buffer, sizeof buffer, value);
    NxStr text;
    text.data = nx_strdup(buffer);
    text.owned = 1;
    return text;
}

NX_UNUSED static NxStr nx_bool_text(double value) {
    return nx_text(value != 0.0 ? "true" : "false");
}

NX_UNUSED static NxStr nx_concat(NxStr left, NxStr right) {
    size_t left_len = strlen(left.data);
    size_t right_len = strlen(right.data);
    char *joined = (char *)malloc(left_len + right_len + 1);
    if (joined == NULL) nx_fail("out of memory building a string");
    memcpy(joined, left.data, left_len);
    memcpy(joined + left_len, right.data, right_len + 1);
    /* Each operand is consumed by the concatenation, so this frees the
       temporaries it copied from. */
    nx_release(&left);
    nx_release(&right);
    NxStr text;
    text.data = joined;
    text.owned = 1;
    return text;
}

NX_UNUSED static void nx_print_str(const NxStr *text) { fputs(text->data, stdout); }
NX_UNUSED static void nx_print_double(double value) {
    char buffer[400];
    nx_format_double(buffer, sizeof buffer, value);
    fputs(buffer, stdout);
}
NX_UNUSED static void nx_print_bool(double value) { fputs(value != 0.0 ? "true" : "false", stdout); }
NX_UNUSED static void nx_space(void) { fputc(' ', stdout); }
NX_UNUSED static void nx_newline(void) { fputc('\n', stdout); }

/* Domain errors keep the VM's wording, which prints the offending value. */
NX_UNUSED static void nx_fail_value(const char *prefix, double value) {
    char buffer[400];
    nx_format_double(buffer, sizeof buffer, value);
    fprintf(stderr, "error: %s%s\n", prefix, buffer);
    exit(1);
}

NX_UNUSED static double nx_sqrt(double value) {
    if (value < 0.0) nx_fail_value("sqrt() is not defined for ", value);
    return sqrt(value);
}

NX_UNUSED static double nx_log(double value) {
    if (value <= 0.0) nx_fail_value("log() is not defined for ", value);
    return log(value);
}

NX_UNUSED static double nx_abs(double value) { return fabs(value); }
NX_UNUSED static double nx_floor(double value) { return floor(value); }
NX_UNUSED static double nx_ceil(double value) { return ceil(value); }
NX_UNUSED static double nx_round(double value) { return round(value); }
NX_UNUSED static double nx_sin(double value) { return sin(value); }
NX_UNUSED static double nx_cos(double value) { return cos(value); }
NX_UNUSED static double nx_tan(double value) { return tan(value); }

/* The VM keeps its running best and replaces it only on a strict comparison, so
   NaN never wins. */
NX_UNUSED static double nx_min2(double best, double candidate) { return candidate < best ? candidate : best; }
NX_UNUSED static double nx_max2(double best, double candidate) { return candidate > best ? candidate : best; }
"#;
