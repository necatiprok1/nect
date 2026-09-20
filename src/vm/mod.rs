//! Bytecode compiler + VM.
//!
//! Performance plan implementation notes:
//!   Phase 1
//!     1.1 identifiers are interned into dense `Symbol` ids (no string hashing at runtime)
//!     1.2 locals live in one flat `Vec<Value>` arena indexed by `frame base + slot`
//!     1.3 opcodes are `Copy`, so dispatch never clones a `String`; calls don't
//!         allocate an argument `Vec`
//!     1.4 there is no runtime scope-chain search at all: variables are resolved
//!         to a slot (or a global id) at compile time
//!   Phase 2
//!     2.1 constant folding and dead code elimination after `return`
//!     2.2 fused three-address opcodes (`BinaryFast`, `BinaryStore`,
//!         `JumpIfNot`) with a numeric fast path that skips type dispatch
use crate::ast::*;
use crate::builtins::{
    apply_binary, apply_unary, get_index, is_truthy, numeric_binary, set_index, Map, RuntimeError,
};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

/// Built-in functions. The VM registers these and the compiler resolves calls
/// to them at compile time.
pub use crate::builtins::NAMES as NATIVE_NAMES;

/// Interned identifier: an index into [`Interner`]'s name table.
pub type Symbol = u32;

/// Compile-time string interner. Every identifier gets a dense `Symbol`, so the
/// VM compares and resolves names with a single `u32`.
#[derive(Debug, Default, Clone)]
pub struct Interner {
    names: Vec<String>,
    ids: HashMap<String, Symbol>,
}

impl Interner {
    pub fn new() -> Self {
        Self::default()
    }

    /// A program always interns the built-in names, so the VM can index its
    /// native table by `Symbol` without mutating the interner while running.
    fn with_natives() -> Self {
        let mut interner = Self::new();
        for name in NATIVE_NAMES {
            interner.intern(name);
        }
        interner
    }

    pub fn intern(&mut self, name: &str) -> Symbol {
        if let Some(id) = self.ids.get(name) {
            return *id;
        }
        let id = self.names.len() as Symbol;
        self.names.push(name.to_string());
        self.ids.insert(name.to_string(), id);
        id
    }

    pub fn resolve(&self, name: &str) -> Option<Symbol> {
        self.ids.get(name).copied()
    }

    pub fn name(&self, id: Symbol) -> &str {
        &self.names[id as usize]
    }

    pub fn len(&self) -> usize {
        self.names.len()
    }

    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }
}

/// How many local slots a frame tracks for "was this written yet?". Slots past
/// this are treated as initialized; the compiler never emits a checked load for
/// them, so only very large functions fall back to the old null behaviour.
const MAX_TRACKED_SLOTS: u32 = 64;

/// A resolved local: its slot, and whether the declaration sits inside an
/// `if`/`while` body (so a read may see an unwritten slot).
#[derive(Debug, Clone, Copy)]
struct Local {
    slot: u32,
    conditional: bool,
}

/// An operand a fused instruction reads directly: a local slot, a global id, or
/// a constant. Packed into one `u32` (2-bit tag + 30-bit index) so `Op` stays
/// small enough to copy cheaply on every dispatch.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fusee(u32);

const FUSEE_TAG_SHIFT: u32 = 30;
const FUSEE_INDEX_MASK: u32 = (1 << FUSEE_TAG_SHIFT) - 1;
pub const TAG_LOCAL: u32 = 0;
pub const TAG_GLOBAL: u32 = 1;
pub const TAG_CONST: u32 = 2;
/// A global the compiler could not prove is declared, so storing to it must be
/// checked at runtime (and report an undefined variable). Globals declared by a
/// module-level `let` use [`TAG_GLOBAL`] and skip the check.
pub const TAG_GLOBAL_CHECKED: u32 = 3;

impl Fusee {
    fn new(tag: u32, index: u32) -> Self {
        debug_assert!(index <= FUSEE_INDEX_MASK, "operand index too large");
        Self((tag << FUSEE_TAG_SHIFT) | (index & FUSEE_INDEX_MASK))
    }

    fn local(slot: u32) -> Self {
        Self::new(TAG_LOCAL, slot)
    }

    fn global(symbol: Symbol) -> Self {
        Self::new(TAG_GLOBAL, symbol)
    }

    fn global_checked(symbol: Symbol) -> Self {
        Self::new(TAG_GLOBAL_CHECKED, symbol)
    }

    fn constant(index: u32) -> Self {
        Self::new(TAG_CONST, index)
    }

    pub fn tag(self) -> u32 {
        self.0 >> FUSEE_TAG_SHIFT
    }

    pub fn index(self) -> u32 {
        self.0 & FUSEE_INDEX_MASK
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Op {
    LoadConst(u32),
    /// Push `locals[base + slot]`.
    LoadLocal(u32),
    /// Push `locals[base + slot]`, erroring when it was never written. Only
    /// emitted for declarations inside `if`/`while` bodies.
    LoadLocalChecked {
        slot: u32,
        name: Symbol,
    },
    /// Pop into `locals[base + slot]`.
    StoreLocal(u32),
    /// Push `globals[id]`, erroring when the global was never bound.
    LoadGlobal(Symbol),
    /// Pop into `globals[id]`.
    StoreGlobal(Symbol),
    /// Store the top of the stack without consuming it, because assignment
    /// evaluates to the assigned value.
    StoreKeep(Fusee),
    BinaryOp(BinaryOp),
    UnaryOp(UnaryOp),
    /// Push `lhs op rhs` with no stack traffic between the operands.
    BinaryFast {
        op: BinaryOp,
        lhs: Fusee,
        rhs: Fusee,
    },
    /// `dst = lhs op rhs`.
    BinaryStore {
        op: BinaryOp,
        dst: Fusee,
        lhs: Fusee,
        rhs: Fusee,
    },
    /// Branch to `target` when `lhs op rhs` is falsy.
    JumpIfNot {
        op: BinaryOp,
        lhs: Fusee,
        rhs: Fusee,
        target: u32,
    },
    Call(CallTarget, u32),
    Return,
    Jump(u32),
    JumpIfFalse(u32),
    JumpIfTrue(u32),
    Pop,
    Halt,
    /// Pop n values, push a new array containing them (in source order).
    MakeArray(u32),
    /// Pop n key/value pairs (value then key, pairs in source order), push a
    /// new map. Any pair whose key is not a string/number/boolean fails here,
    /// so the runtime error surfaces identically to the interpreter's.
    MakeMap(u32),
    /// Pop an iterable, push an array to iterate: the elements of an array,
    /// or the keys of a map in insertion order. Normalizes the for-loop's
    /// "visit each item" so the numeric index lowering below is unchanged.
    IterList,
    /// Pop array, push its length as a number.
    ArrayLen,
    /// Pop index then target, push `target[index]` (an array element or a
    /// one-character string).
    LoadIndex,
    /// Pop value, index, target; store value at index, then push the value.
    StoreIndex,
    /// `target[index] op= rhs`: pop rhs, index, target; apply `op` to the
    /// current element and store the result, then push it. The element and the
    /// index are each evaluated once, which is why this is an opcode rather
    /// than an expansion into load/binary/store.
    StoreIndexOp(BinaryOp),
}

impl Op {
    /// The instruction a control-transfer branches to, if any.
    pub fn jump_target(&self) -> Option<u32> {
        match self {
            Op::Jump(target)
            | Op::JumpIfFalse(target)
            | Op::JumpIfTrue(target)
            | Op::JumpIfNot { target, .. } => Some(*target),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CallTarget {
    /// A built-in, addressed by the interned name.
    Native(Symbol),
    /// A user function, addressed by index into `Program::functions`.
    Function(u32),
}

/// Placeholder jump target, patched once the destination is known.
const PATCH: u32 = u32::MAX;

#[derive(Clone)]
pub struct CompiledFunction {
    pub name: Symbol,
    pub param_count: u32,
    /// Number of local slots the frame needs (parameters use slots `0..param_count`).
    pub num_slots: u32,
    pub instructions: Rc<Vec<Op>>,
    pub constants: Rc<Vec<Value>>,
}

impl CompiledFunction {
    /// Stand-in for a function that has been named but not compiled yet, so
    /// forward calls can be resolved while the program is still being built.
    fn placeholder(name: Symbol) -> Self {
        Self {
            name,
            param_count: 0,
            num_slots: 0,
            instructions: Rc::new(vec![Op::LoadConst(0), Op::Return]),
            constants: Rc::new(vec![Value::Null]),
        }
    }
}

/// Module-level code in a shape native code can run (performance plan phase
/// 3.2, "JIT top-level code").
///
/// Module-level `let`s are globals, and native code only speaks `f64`, so the
/// compiler also emits a *mirror* of the module body in which every
/// definitely-declared global is a frame slot. [`ModuleEntry::globals`] maps
/// those slots back to symbols, so the VM can write the values a native run
/// produced into its global table when it hands control back to bytecode.
///
/// Instructions the mirror cannot represent (module-frame locals, and globals
/// the compiler could not prove are declared) are poisoned with [`Op::Halt`],
/// which inference rejects, so native code never runs past a point where the
/// mirror and the bytecode would disagree.
#[derive(Clone)]
pub struct ModuleEntry {
    /// The mirrored body. Its slots are the mirrored globals, in order.
    pub function: CompiledFunction,
    /// Mirror slot -> global symbol.
    pub globals: Vec<Symbol>,
    /// Module-body instruction index where each module-level statement starts.
    pub statements: Vec<u32>,
    /// Index of the module body's `Halt`.
    pub halt: u32,
}

pub struct Program {
    pub instructions: Rc<Vec<Op>>,
    pub constants: Rc<Vec<Value>>,
    /// Indexed by [`CallTarget::Function`].
    pub functions: Vec<CompiledFunction>,
    /// Slots needed by module-level block scopes (the module's own frame).
    pub module_slots: u32,
    /// The module body mirrored for native compilation; `None` when there is
    /// nothing to mirror.
    pub module_entry: Option<ModuleEntry>,
    pub interner: Rc<Interner>,
}

impl Program {
    /// Renders the compiled bytecode (used by `nect disasm` and for debugging
    /// the compiler, the fusion pass, and JIT eligibility).
    pub fn disassemble(&self) -> String {
        let mut out = format!(
            "=== module: {} slot(s), {} instruction(s) ===\n",
            self.module_slots,
            self.instructions.len()
        );
        self.write_body(&mut out, &self.instructions, &self.constants);
        for (index, function) in self.functions.iter().enumerate() {
            out.push_str(&format!(
                "\n=== fn {} (index {index}, {} param(s), {} slot(s), {} instruction(s)) ===\n",
                self.interner.name(function.name),
                function.param_count,
                function.num_slots,
                function.instructions.len()
            ));
            self.write_body(&mut out, &function.instructions, &function.constants);
        }
        out
    }

    fn write_body(&self, out: &mut String, instructions: &[Op], constants: &[Value]) {
        for (position, op) in instructions.iter().enumerate() {
            out.push_str(&format!(
                "  {position:>4}  {}\n",
                self.format_op(op, constants)
            ));
        }
    }

    fn format_op(&self, op: &Op, constants: &[Value]) -> String {
        match op {
            Op::LoadConst(index) => format!(
                "load.const   {index} ({})",
                crate::interpreter::format_value(&constants[*index as usize])
            ),
            Op::LoadLocal(slot) => format!("load.local   slot{slot}"),
            Op::LoadLocalChecked { slot, name } => format!(
                "load.local?  slot{slot}   ; {}",
                self.interner.name(*name)
            ),
            Op::StoreLocal(slot) => format!("store.local  slot{slot}"),
            Op::LoadGlobal(symbol) => format!(
                "load.global  #{}   ; {}",
                symbol,
                self.interner.name(*symbol)
            ),
            Op::StoreGlobal(symbol) => format!(
                "store.global #{}   ; {}",
                symbol,
                self.interner.name(*symbol)
            ),
            Op::StoreKeep(target) => {
                format!("store.keep   {}", self.format_fusee(target, constants))
            }
            Op::BinaryOp(operator) => format!("binary       {operator:?}"),
            Op::UnaryOp(operator) => format!("unary        {operator:?}"),
            Op::BinaryFast { op, lhs, rhs } => format!(
                "fast         {} {} {}",
                self.format_fusee(lhs, constants),
                format_operator(*op),
                self.format_fusee(rhs, constants)
            ),
            Op::BinaryStore {
                op,
                dst,
                lhs,
                rhs,
            } => format!(
                "store        {} = {} {} {}",
                self.format_fusee(dst, constants),
                self.format_fusee(lhs, constants),
                format_operator(*op),
                self.format_fusee(rhs, constants)
            ),
            Op::JumpIfNot {
                op,
                lhs,
                rhs,
                target,
            } => format!(
                "branch       unless {} {} {} -> {target}",
                self.format_fusee(lhs, constants),
                format_operator(*op),
                self.format_fusee(rhs, constants)
            ),
            Op::Call(CallTarget::Native(symbol), argc) => format!(
                "call         {} (builtin) {argc}",
                self.interner.name(*symbol)
            ),
            Op::Call(CallTarget::Function(index), argc) => format!(
                "call         fn{index} {} {argc}",
                self.interner.name(self.functions[*index as usize].name)
            ),
            Op::Return => "return".to_string(),
            Op::Jump(target) => format!("jump         {target}"),
            Op::JumpIfFalse(target) => format!("jump.false   {target}"),
            Op::JumpIfTrue(target) => format!("jump.true    {target}"),
            Op::Pop => "pop".to_string(),
            Op::Halt => "halt".to_string(),
            Op::MakeArray(n) => format!("make.array   {n}"),
            Op::MakeMap(n) => format!("make.map     {n}"),
            Op::IterList => "iter.list".to_string(),
            Op::ArrayLen => "array.len".to_string(),
            Op::LoadIndex => "load.index".to_string(),
            Op::StoreIndex => "store.index".to_string(),
            Op::StoreIndexOp(op) => format!("store.index[{:?}]", op),
        }
    }

    fn format_fusee(&self, operand: &Fusee, constants: &[Value]) -> String {
        match operand.tag() {
            TAG_LOCAL => format!("slot{}", operand.index()),
            TAG_GLOBAL => format!("#{}({})", operand.index(), self.interner.name(operand.index())),
            TAG_GLOBAL_CHECKED => format!(
                "#?{}({})",
                operand.index(),
                self.interner.name(operand.index())
            ),
            _ => format!(
                "const{} ({})",
                operand.index(),
                crate::interpreter::format_value(&constants[operand.index() as usize])
            ),
        }
    }
}

fn format_operator(op: BinaryOp) -> &'static str {
    match op {
        BinaryOp::Add => "+",
        BinaryOp::Subtract => "-",
        BinaryOp::Multiply => "*",
        BinaryOp::Divide => "/",
        BinaryOp::Modulo => "%",
        BinaryOp::Equal => "==",
        BinaryOp::NotEqual => "!=",
        BinaryOp::Less => "<",
        BinaryOp::Greater => ">",
        BinaryOp::LessEqual => "<=",
        BinaryOp::GreaterEqual => ">=",
        BinaryOp::And => "&&",
        BinaryOp::Or => "||",
    }
}

/// Strips redundant grouping so `(x)` still counts as a simple operand.
fn without_grouping(expr: &Expr) -> &Expr {
    let mut expr = expr;
    while let Expr::Grouping(inner) = expr {
        expr = inner;
    }
    expr
}

/// Combines the per-operand mirror decisions of one instruction.
fn merge(a: Mirror, b: Mirror) -> Mirror {
    match (a, b) {
        (Mirror::Poison, _) | (_, Mirror::Poison) => Mirror::Poison,
        (Mirror::Copy, _) | (_, Mirror::Copy) => Mirror::Copy,
        _ => Mirror::Rewrite,
    }
}

/// Rewrites a module-body instruction's definitely-declared globals into mirror
/// slot accesses.
fn mirror_op(mut op: Op, slot_of: &mut impl FnMut(Symbol) -> u32) -> Op {
    fn operand(operand: Fusee, slot_of: &mut impl FnMut(Symbol) -> u32) -> Fusee {
        if operand.tag() == TAG_GLOBAL {
            Fusee::local(slot_of(operand.index()))
        } else {
            // Constants (and anything else) are already mirror-compatible.
            operand
        }
    }
    match &mut op {
        Op::LoadGlobal(symbol) => return Op::LoadLocal(slot_of(*symbol)),
        Op::StoreGlobal(symbol) => return Op::StoreLocal(slot_of(*symbol)),
        Op::StoreKeep(target) => *target = operand(*target, slot_of),
        Op::BinaryFast { lhs, rhs, .. } | Op::JumpIfNot { lhs, rhs, .. } => {
            *lhs = operand(*lhs, slot_of);
            *rhs = operand(*rhs, slot_of);
        }
        Op::BinaryStore { dst, lhs, rhs, .. } => {
            *dst = operand(*dst, slot_of);
            *lhs = operand(*lhs, slot_of);
            *rhs = operand(*rhs, slot_of);
        }
        _ => {}
    }
    op
}

pub struct Compiler {
    instructions: Vec<Op>,
    constants: Vec<Value>,
    functions: Vec<CompiledFunction>,
    function_index: HashMap<String, u32>,
    interner: Interner,
    /// Compile-time scope stack mapping a name to its local slot.
    scopes: Vec<HashMap<Symbol, Local>>,
    /// Module-level `let`s seen so far, and whether they always execute (a
    /// declaration inside an `if`/`while` body may never run).
    declared_globals: HashMap<Symbol, bool>,
    next_slot: u32,
    /// How many `if`/`while` bodies enclose the current point, used to spot
    /// declarations that may never execute.
    conditional_depth: u32,
    /// Module-level `let`s become globals; everything else is a local slot.
    is_module: bool,
    /// Per-instruction mirror decision for the module body (see [`Mirror`]).
    mirror: Vec<Mirror>,
    /// Module-body instruction index where each module-level statement starts.
    module_statements: Vec<u32>,
    /// Set while compiling the module's own top-level statement list.
    record_statements: bool,
    /// Enclosing loops, innermost last. `break` and `continue` compile into
    /// jumps to the patch lists of the innermost entry.
    loops: Vec<LoopContext>,
}

/// Where the `break` and `continue` inside one loop must jump.
#[derive(Default)]
struct LoopContext {
    /// Where `continue` resumes: the next condition check for `while`, the
    /// index increment for `for` (patched once that code has been emitted).
    continue_target: Option<u32>,
    /// Jump instructions whose target is the loop's exit.
    breaks: Vec<usize>,
    /// `continue` jumps emitted before `continue_target` was known (`for`).
    continues: Vec<usize>,
}

/// How one module-body instruction is mirrored into [`ModuleEntry`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mirror {
    /// No state the mirror renumbers: copy the instruction unchanged.
    Copy,
    /// Every global it touches is proven declared, so it becomes a slot access.
    Rewrite,
    /// Touches a module-frame local, whose slot numbering the mirror reuses for
    /// globals; the instruction is replaced by [`Op::Halt`] so inference
    /// refuses to compile past it.
    Poison,
}

impl Compiler {
    pub fn compile(stmts: &[Stmt]) -> Result<Program, RuntimeError> {
        let mut compiler = Compiler::new();
        // Pass 1: reserve an index per function so calls can be resolved
        // regardless of definition order (including recursion).
        compiler.hoist_functions(stmts);
        // Pass 2: emit code.
        compiler.record_statements = true;
        compiler.compile_stmts(stmts)?;
        compiler.record_statements = false;
        let halt = compiler.emit(Op::Halt) as u32;
        let constants = Rc::new(std::mem::take(&mut compiler.constants));
        // Built before the module body is moved out, since the mirror is derived
        // from it.
        let module_entry = compiler.build_module_entry(halt, Rc::clone(&constants));
        let instructions = Rc::new(std::mem::take(&mut compiler.instructions));
        Ok(Program {
            instructions,
            constants,
            functions: std::mem::take(&mut compiler.functions),
            module_slots: compiler.next_slot,
            module_entry,
            interner: Rc::new(compiler.interner),
        })
    }

    fn new() -> Self {
        Self {
            instructions: Vec::new(),
            constants: Vec::new(),
            functions: Vec::new(),
            function_index: HashMap::new(),
            interner: Interner::with_natives(),
            scopes: vec![HashMap::new()],
            declared_globals: HashMap::new(),
            next_slot: 0,
            conditional_depth: 0,
            is_module: true,
            mirror: Vec::new(),
            module_statements: Vec::new(),
            record_statements: false,
            loops: Vec::new(),
        }
    }

    fn emit(&mut self, op: Op) -> usize {
        let idx = self.instructions.len();
        debug_assert_eq!(self.mirror.len(), idx, "mirror flags out of step");
        let mirror = self.mirror_decision(&op);
        self.instructions.push(op);
        self.mirror.push(mirror);
        idx
    }

    /// Whether one module-body instruction can be mirrored into native code.
    fn mirror_decision(&self, op: &Op) -> Mirror {
        if !self.is_module {
            return Mirror::Copy;
        }
        let declared = |symbol: Symbol| self.declared_globals.get(&symbol).copied() == Some(true);
        let operand = |operand: &Fusee| match operand.tag() {
            // Module-frame locals would collide with the mirror's slot numbers.
            TAG_LOCAL => Mirror::Poison,
            TAG_GLOBAL if declared(operand.index()) => Mirror::Rewrite,
            // Unproven globals stay globals, which inference rejects.
            TAG_GLOBAL | TAG_GLOBAL_CHECKED => Mirror::Copy,
            // Constants are unaffected by mirroring.
            _ => Mirror::Rewrite,
        };
        match op {
            Op::LoadGlobal(symbol) | Op::StoreGlobal(symbol) => {
                if declared(*symbol) {
                    Mirror::Rewrite
                } else {
                    Mirror::Copy
                }
            }
            Op::LoadLocal(_) | Op::StoreLocal(_) | Op::LoadLocalChecked { .. } => Mirror::Poison,
            Op::StoreKeep(target) => operand(target),
            Op::BinaryFast { lhs, rhs, .. } | Op::JumpIfNot { lhs, rhs, .. } => {
                merge(operand(lhs), operand(rhs))
            }
            Op::BinaryStore { dst, lhs, rhs, .. } => {
                merge(merge(operand(dst), operand(lhs)), operand(rhs))
            }
            _ => Mirror::Copy,
        }
    }

    /// Mirrors the module body into a form the JIT can compile.
    fn build_module_entry(&mut self, halt: u32, constants: Rc<Vec<Value>>) -> Option<ModuleEntry> {
        if self.instructions.is_empty() {
            return None;
        }
        let decisions = std::mem::take(&mut self.mirror);
        let mut globals: Vec<Symbol> = Vec::new();
        let mut slot_of: HashMap<Symbol, u32> = HashMap::new();
        let mut instructions: Vec<Op> = Vec::with_capacity(self.instructions.len());
        for (index, op) in self.instructions.iter().enumerate() {
            let decision = decisions.get(index).copied().unwrap_or(Mirror::Copy);
            let mut op = *op;
            if decision == Mirror::Rewrite {
                op = mirror_op(op, &mut |symbol: Symbol| {
                    *slot_of.entry(symbol).or_insert_with(|| {
                        globals.push(symbol);
                        (globals.len() - 1) as u32
                    })
                });
            } else if decision == Mirror::Poison {
                op = Op::Halt;
            }
            instructions.push(op);
        }
        Some(ModuleEntry {
            function: CompiledFunction {
                name: self.interner.intern("__module"),
                param_count: 0,
                num_slots: globals.len() as u32,
                instructions: Rc::new(instructions),
                constants,
            },
            globals,
            statements: std::mem::take(&mut self.module_statements),
            halt,
        })
    }

    /// Interns `value` and returns its index, reusing an identical constant.
    fn constant_index(&mut self, value: &Value) -> u32 {
        match self.constants.iter().position(|c| *c == *value) {
            Some(i) => i as u32,
            None => {
                self.constants.push(value.clone());
                (self.constants.len() - 1) as u32
            }
        }
    }

    fn emit_load_const(&mut self, value: &Value) {
        let idx = self.constant_index(value);
        self.emit(Op::LoadConst(idx));
    }

    fn patch_jump(&mut self, pos: usize) {
        let target = self.instructions.len() as u32;
        self.patch_jump_to(pos, target);
    }

    /// Patches a jump emitted earlier to branch to `target`.
    fn patch_jump_to(&mut self, pos: usize, target: u32) {
        match &mut self.instructions[pos] {
            Op::Jump(t)
            | Op::JumpIfFalse(t)
            | Op::JumpIfTrue(t)
            | Op::JumpIfNot { target: t, .. } => *t = target,
            _ => unreachable!("only jumps can be patched"),
        }
    }

    fn hoist_functions(&mut self, stmts: &[Stmt]) {
        for stmt in stmts {
            match stmt {
                Stmt::Function { name, .. } => {
                    self.function_index_of(name);
                }
                Stmt::Block(body) => self.hoist_functions(body),
                Stmt::If {
                    then_branch,
                    else_branch,
                    ..
                } => {
                    self.hoist_functions(then_branch);
                    self.hoist_functions(else_branch);
                }
                Stmt::While { body, .. } => self.hoist_functions(body),
                _ => {}
            }
        }
    }

    fn function_index_of(&mut self, name: &str) -> u32 {
        if let Some(idx) = self.function_index.get(name) {
            return *idx;
        }
        let symbol = self.interner.intern(name);
        let idx = self.functions.len() as u32;
        self.functions.push(CompiledFunction::placeholder(symbol));
        self.function_index.insert(name.to_string(), idx);
        idx
    }

    fn lookup_local(&self, symbol: Symbol) -> Option<Local> {
        for scope in self.scopes.iter().rev() {
            if let Some(local) = scope.get(&symbol) {
                return Some(*local);
            }
        }
        None
    }

    /// Resolves an assignment target to the local slot or global it names.
    fn store_target(&mut self, name: &str) -> Fusee {
        let symbol = self.interner.intern(name);
        match self.lookup_local(symbol) {
            Some(local) => Fusee::local(local.slot),
            // Storing to a global the compiler never saw declared (or one that
            // may not have run yet) needs a runtime check; the check is skipped
            // on the hot path for globals that are definitely declared.
            None => match self.declared_globals.get(&symbol) {
                Some(true) => Fusee::global(symbol),
                _ => Fusee::global_checked(symbol),
            },
        }
    }

    /// A simple operand (local, global, or constant) that a fused instruction
    /// can address without pushing anything onto the operand stack.
    fn fusee_of(&mut self, expr: &Expr) -> Option<Fusee> {
        match without_grouping(expr) {
            Expr::Literal(value) => Some(Fusee::constant(self.constant_index(value))),
            Expr::Variable(name) => {
                let symbol = self.interner.intern(name);
                match self.lookup_local(symbol) {
                    // A conditionally declared slot must go through the checked
                    // load path, so it is not a fusion candidate.
                    Some(local) if local.conditional => None,
                    Some(local) => Some(Fusee::local(local.slot)),
                    None => Some(Fusee::global(symbol)),
                }
            }
            _ => None,
        }
    }

    /// Emits a branch taken when `cond` is falsy, returning the jump position.
    fn emit_jump_if_false(&mut self, cond: &Expr) -> Result<usize, RuntimeError> {
        if let Expr::Binary { left, op, right } = without_grouping(cond)
            && let (Some(lhs), Some(rhs)) = (self.fusee_of(left), self.fusee_of(right))
        {
            let pos = self.instructions.len();
            self.emit(Op::JumpIfNot {
                op: *op,
                lhs,
                rhs,
                target: PATCH,
            });
            return Ok(pos);
        }
        self.compile_expr(cond)?;
        Ok(self.emit(Op::JumpIfFalse(PATCH)))
    }

    /// `&&` / `||` short-circuit exactly like the interpreter: the right operand
    /// is only evaluated when the left one does not already decide the result,
    /// and both operators produce a boolean.
    fn compile_logical(
        &mut self,
        left: &Expr,
        op: BinaryOp,
        right: &Expr,
    ) -> Result<(), RuntimeError> {
        // `&&` jumps to the false result, `||` to the true result.
        let short_circuit = match op {
            BinaryOp::And => self.emit_jump_if_false(left)?,
            _ => {
                self.compile_expr(left)?;
                self.emit(Op::JumpIfTrue(PATCH))
            }
        };
        self.compile_expr(right)?;
        let second_jump = match op {
            BinaryOp::And => self.emit(Op::JumpIfFalse(PATCH)),
            _ => self.emit(Op::JumpIfTrue(PATCH)),
        };
        // Neither side decided: push the other truth value.
        let decided = match op {
            BinaryOp::And => Value::Boolean(true),
            _ => Value::Boolean(false),
        };
        self.emit_load_const(&decided);
        let end_jump = self.emit(Op::Jump(PATCH));
        self.patch_jump(short_circuit);
        self.patch_jump(second_jump);
        let decided = match op {
            BinaryOp::And => Value::Boolean(false),
            _ => Value::Boolean(true),
        };
        self.emit_load_const(&decided);
        self.patch_jump(end_jump);
        Ok(())
    }

    /// Compiles a statement list, dropping statements that cannot be reached
    /// because an earlier statement always returns.
    fn compile_stmts(&mut self, stmts: &[Stmt]) -> Result<(), RuntimeError> {
        // Statement boundaries are the only points native module code may hand
        // control back to the bytecode VM, so they are recorded for the JIT.
        let record = std::mem::replace(&mut self.record_statements, false);
        for stmt in stmts {
            if record {
                self.module_statements.push(self.instructions.len() as u32);
            }
            self.compile_stmt(stmt)?;
            if matches!(stmt, Stmt::Return(_)) {
                break;
            }
        }
        self.record_statements = record;
        Ok(())
    }

    fn compile_stmt(&mut self, stmt: &Stmt) -> Result<(), RuntimeError> {
        match stmt {
            Stmt::Expression(expr) => {
                // `dst = lhs op rhs` as a whole statement becomes one opcode.
                if let Expr::Assign { name, value } = expr
                    && let Expr::Binary { left, op, right } = without_grouping(value)
                    && let (Some(lhs), Some(rhs)) =
                        (self.fusee_of(left), self.fusee_of(right))
                {
                    let dst = self.store_target(name);
                    self.emit(Op::BinaryStore {
                        op: *op,
                        dst,
                        lhs,
                        rhs,
                    });
                    return Ok(());
                }
                self.compile_expr(expr)?;
                self.emit(Op::Pop);
            }
            Stmt::Let { name, value } => {
                self.compile_expr(value)?;
                let symbol = self.interner.intern(name);
                if self.is_module && self.scopes.len() == 1 {
                    self.declared_globals
                        .insert(symbol, self.conditional_depth == 0);
                    self.emit(Op::StoreGlobal(symbol));
                } else {
                    // Fresh slot per declaration: shadowing falls out for free
                    // because slots are never reused.
                    let slot = self.next_slot;
                    self.next_slot += 1;
                    // A declaration inside an `if`/`while` body may never run,
                    // so reads of it need a runtime written-check.
                    let conditional =
                        self.conditional_depth > 0 && slot < MAX_TRACKED_SLOTS;
                    self.scopes
                        .last_mut()
                        .unwrap()
                        .insert(symbol, Local { slot, conditional });
                    self.emit(Op::StoreLocal(slot));
                }
            }
            Stmt::Block(stmts) => {
                // Only a bare block introduces a scope; `if`/`while` bodies do
                // not, matching the original interpreter semantics.
                self.scopes.push(HashMap::new());
                self.compile_stmts(stmts)?;
                self.scopes.pop();
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                let else_jump = self.emit_jump_if_false(condition)?;
                self.conditional_depth += 1;
                self.compile_stmts(then_branch)?;
                if !else_branch.is_empty() {
                    let end_jump = self.emit(Op::Jump(PATCH));
                    self.patch_jump(else_jump);
                    self.compile_stmts(else_branch)?;
                    self.patch_jump(end_jump);
                } else {
                    self.patch_jump(else_jump);
                }
                self.conditional_depth -= 1;
            }
            Stmt::While { condition, body } => {
                let loop_start = self.instructions.len() as u32;
                let exit_jump = self.emit_jump_if_false(condition)?;
                // `continue` re-tests the condition, which is the loop start.
                self.loops.push(LoopContext {
                    continue_target: Some(loop_start),
                    ..LoopContext::default()
                });
                self.conditional_depth += 1;
                self.compile_stmts(body)?;
                self.conditional_depth -= 1;
                let context = self.loops.pop().expect("loop context");
                for jump in context.continues {
                    self.patch_jump_to(jump, loop_start);
                }
                self.emit(Op::Jump(loop_start));
                self.patch_jump(exit_jump);
                for jump in context.breaks {
                    self.patch_jump(jump);
                }
            }
            Stmt::Break => {
                let jump = self.emit(Op::Jump(PATCH));
                match self.loops.last_mut() {
                    Some(context) => context.breaks.push(jump),
                    None => {
                        // Reported here rather than at runtime, because the
                        // branch target only exists while a loop is open.
                        self.instructions.pop();
                        return Err(RuntimeError::new("'break' outside of a loop"));
                    }
                }
            }
            Stmt::Continue => {
                match self.loops.last() {
                    Some(context) => match context.continue_target {
                        Some(target) => {
                            self.emit(Op::Jump(target));
                        }
                        None => {
                            // `for`: the increment is emitted after the body.
                            let jump = self.emit(Op::Jump(PATCH));
                            self.loops
                                .last_mut()
                                .expect("loop context")
                                .continues
                                .push(jump);
                        }
                    },
                    None => {
                        return Err(RuntimeError::new("'continue' outside of a loop"));
                    }
                }
            }
            Stmt::For {
                var_name,
                iterable,
                body,
            } => {
                let symbol = self.interner.intern(var_name);
                let array_slot = self.next_slot;
                self.next_slot += 1;
                let len_slot = self.next_slot;
                self.next_slot += 1;
                let idx_slot = self.next_slot;
                self.next_slot += 1;
                let x_slot = self.next_slot;
                self.next_slot += 1;
                self.scopes
                    .last_mut()
                    .unwrap()
                    .insert(symbol, Local { slot: x_slot, conditional: false });
                // items = iterable (arrays iterate elements; maps iterate keys)
                self.compile_expr(iterable)?;
                self.emit(Op::IterList);
                self.emit(Op::StoreLocal(array_slot));
                // len = items.len()
                self.emit(Op::LoadLocal(array_slot));
                self.emit(Op::ArrayLen);
                self.emit(Op::StoreLocal(len_slot));
                // idx = 0
                self.emit_load_const(&Value::Number(0.0));
                self.emit(Op::StoreLocal(idx_slot));
                let loop_start = self.instructions.len() as u32;
                // if !(idx < len) goto end. The branch reads both slots
                // directly, so pushing them first would leave two values on
                // the operand stack every iteration (a leak, and it stops the
                // C backend from translating the loop).
                let exit = self.emit(Op::JumpIfNot {
                    op: BinaryOp::Less,
                    lhs: Fusee::local(idx_slot),
                    rhs: Fusee::local(len_slot),
                    target: PATCH,
                });
                // x = array[idx]
                self.emit(Op::LoadLocal(array_slot));
                self.emit(Op::LoadLocal(idx_slot));
                self.emit(Op::LoadIndex);
                self.emit(Op::StoreLocal(x_slot));
                // body — `continue` lands on the increment below.
                self.loops.push(LoopContext::default());
                self.compile_stmts(body)?;
                let continue_target = self.instructions.len() as u32;
                let context = self.loops.pop().expect("loop context");
                for jump in context.continues {
                    self.patch_jump_to(jump, continue_target);
                }
                // idx++
                self.emit(Op::LoadLocal(idx_slot));
                self.emit_load_const(&Value::Number(1.0));
                self.emit(Op::BinaryOp(BinaryOp::Add));
                self.emit(Op::StoreLocal(idx_slot));
                // goto loop_start
                self.emit(Op::Jump(loop_start));
                self.patch_jump(exit);
                for jump in context.breaks {
                    self.patch_jump(jump);
                }
            }
            Stmt::Function { name, params, body } => {
                self.compile_function(name, params, body)?;
            }
            Stmt::Return(expr) => {
                match expr {
                    Some(e) => self.compile_expr(e)?,
                    None => self.emit_load_const(&Value::Null),
                }
                self.emit(Op::Return);
            }
        }
        Ok(())
    }

    fn compile_function(
        &mut self,
        name: &str,
        params: &[String],
        body: &[Stmt],
    ) -> Result<(), RuntimeError> {
        let index = self.function_index_of(name) as usize;
        let name_symbol = self.interner.intern(name);

        // Swap in a fresh compilation state for the function body.
        let outer_instructions = std::mem::take(&mut self.instructions);
        let outer_constants = std::mem::take(&mut self.constants);
        let outer_scopes = std::mem::take(&mut self.scopes);
        let outer_slots = std::mem::replace(&mut self.next_slot, 0);
        let outer_conditional = std::mem::replace(&mut self.conditional_depth, 0);
        // A function body never breaks out of a loop that encloses its
        // definition, so the enclosing loop context ends at the body boundary.
        let outer_loops = std::mem::take(&mut self.loops);
        let was_module = std::mem::replace(&mut self.is_module, false);
        let outer_mirror = std::mem::take(&mut self.mirror);
        let outer_record = std::mem::replace(&mut self.record_statements, false);

        // Parameters occupy slots 0..param_count positionally, so the call
        // prologue can copy arguments straight into the frame.
        let mut scope = HashMap::new();
        let mut slot = 0;
        for param in params {
            let symbol = self.interner.intern(param);
            // A repeated parameter name resolves to its last slot, so the last
            // argument wins, as before.
            scope.insert(
                symbol,
                Local {
                    slot,
                    conditional: false,
                },
            );
            slot += 1;
        }
        self.scopes = vec![scope];
        self.next_slot = slot;

        self.compile_stmts(body)?;
        self.emit_load_const(&Value::Null);
        self.emit(Op::Return);

        let function = CompiledFunction {
            name: name_symbol,
            param_count: params.len() as u32,
            num_slots: self.next_slot,
            instructions: Rc::new(std::mem::take(&mut self.instructions)),
            constants: Rc::new(std::mem::take(&mut self.constants)),
        };
        self.functions[index] = function;

        self.instructions = outer_instructions;
        self.constants = outer_constants;
        self.scopes = outer_scopes;
        self.next_slot = outer_slots;
        self.conditional_depth = outer_conditional;
        self.is_module = was_module;
        self.mirror = outer_mirror;
        self.record_statements = outer_record;
        self.loops = outer_loops;
        Ok(())
    }

    fn compile_expr(&mut self, expr: &Expr) -> Result<(), RuntimeError> {
        match expr {
            Expr::Literal(v) => {
                self.emit_load_const(v);
            }
            Expr::Variable(name) => {
                let symbol = self.interner.intern(name);
                match self.lookup_local(symbol) {
                    // Declared inside an `if`/`while` body: the slot may never
                    // have been written, which must stay an error.
                    Some(local) if local.conditional => {
                        self.emit(Op::LoadLocalChecked {
                            slot: local.slot,
                            name: symbol,
                        });
                    }
                    Some(local) => {
                        self.emit(Op::LoadLocal(local.slot));
                    }
                    None => {
                        self.emit(Op::LoadGlobal(symbol));
                    }
                }
            }
            Expr::Assign { name, value } => {
                // Assignment evaluates to the assigned value, so the result
                // stays on the operand stack for the enclosing expression.
                self.compile_expr(value)?;
                let target = self.store_target(name);
                self.emit(Op::StoreKeep(target));
            }
            Expr::Binary { left, op, right } => {
                if matches!(op, BinaryOp::And | BinaryOp::Or) {
                    self.compile_logical(left, *op, right)?;
                    return Ok(());
                }
                // 2.1 constant folding.
                // Fold only when the runtime would succeed, so error cases
                // (division by zero, type mismatches) still fail at runtime with
                // the original message.
                if let (Expr::Literal(l), Expr::Literal(r)) =
                    (without_grouping(left), without_grouping(right))
                    && let Ok(folded) = apply_binary(l.clone(), *op, r.clone())
                {
                    self.emit_load_const(&folded);
                    return Ok(());
                }
                // 2.2 specialized three-address form.
                if let (Some(lhs), Some(rhs)) = (self.fusee_of(left), self.fusee_of(right)) {
                    self.emit(Op::BinaryFast { op: *op, lhs, rhs });
                    return Ok(());
                }
                self.compile_expr(left)?;
                self.compile_expr(right)?;
                self.emit(Op::BinaryOp(*op));
            }
            Expr::Unary { op, operand } => {
                if let Expr::Literal(v) = without_grouping(operand)
                    && let Ok(folded) = apply_unary(*op, v.clone())
                {
                    self.emit_load_const(&folded);
                    return Ok(());
                }
                self.compile_expr(operand)?;
                self.emit(Op::UnaryOp(*op));
            }
            Expr::Conditional {
                condition,
                then_expr,
                else_expr,
            } => {
                // Both branches leave exactly one value on the stack.
                let else_jump = self.emit_jump_if_false(condition)?;
                self.compile_expr(then_expr)?;
                let end_jump = self.emit(Op::Jump(PATCH));
                self.patch_jump(else_jump);
                self.compile_expr(else_expr)?;
                self.patch_jump(end_jump);
            }
            Expr::Grouping(e) => {
                self.compile_expr(e)?;
            }
            Expr::Array(elements) => {
                for element in elements {
                    self.compile_expr(element)?;
                }
                self.emit(Op::MakeArray(elements.len() as u32));
            }
            Expr::Map(entries) => {
                // Push value then key so the executor can pop key first.
                for (key_expr, value_expr) in entries {
                    self.compile_expr(key_expr)?;
                    self.compile_expr(value_expr)?;
                }
                self.emit(Op::MakeMap(entries.len() as u32));
            }
            Expr::GetIndex { array, index } => {
                self.compile_expr(array)?;
                self.compile_expr(index)?;
                self.emit(Op::LoadIndex);
            }
            Expr::SetIndex {
                array,
                index,
                op,
                value,
            } => {
                self.compile_expr(array)?;
                self.compile_expr(index)?;
                self.compile_expr(value)?;
                match op {
                    Some(op) => {
                        self.emit(Op::StoreIndexOp(*op));
                    }
                    None => {
                        self.emit(Op::StoreIndex);
                    }
                }
            }
            Expr::Call { callee, args } => {
                for arg in args {
                    self.compile_expr(arg)?;
                }
                let Expr::Variable(name) = &**callee else {
                    return Err(RuntimeError::new(
                        "calling non-variable expressions is not yet supported",
                    ));
                };
                let symbol = self.interner.intern(name);
                let target = if NATIVE_NAMES.contains(&name.as_str()) {
                    CallTarget::Native(symbol)
                } else if let Some(index) = self.function_index.get(name) {
                    CallTarget::Function(*index)
                } else {
                    return Err(RuntimeError::new(&format!("undefined function '{}'", name)));
                };
                self.emit(Op::Call(target, args.len() as u32));
            }
        }
        Ok(())
    }
}

type NativeFn = Rc<dyn Fn(&[Value]) -> Result<Value, RuntimeError>>;

/// A builtin call. The implementation lives in [`crate::builtins`], which the
/// reference interpreter calls too, so the two engines cannot drift apart.
fn native_fn(name: &'static str) -> NativeFn {
    Rc::new(move |args| crate::builtins::call(name, args))
}

pub struct VM {
    /// The value stack. Each frame owns the contiguous slice
    /// `[base, locals_end)` for its local slots and pushes operands above
    /// `locals_end`. Because a call's arguments are the topmost operands, they
    /// are already sitting in slots `0..arg_count` of the callee's frame, so a
    /// call never copies arguments.
    stack: Vec<Value>,
    frames: Vec<Frame>,
    functions: Vec<CompiledFunction>,
    /// Indexed by `Symbol`.
    natives: Vec<Option<NativeFn>>,
    /// Indexed by `Symbol`.
    globals: Vec<Option<Value>>,
    interner: Rc<Interner>,
    ip: usize,
    instructions: Rc<Vec<Op>>,
    constants: Rc<Vec<Value>>,
    /// Slot 0 of the running frame inside `stack`.
    base: usize,
    /// One past the last local slot of the running frame: where its operands
    /// start. Guarded by `debug_assert!`s so a miscompiled operand stack is
    /// caught by the test suite instead of silently overwriting locals.
    locals_end: usize,
    /// Bit per local slot: written yet? Only consulted by
    /// [`Op::LoadLocalChecked`].
    defined: u64,
    /// Natively compiled functions (performance plan phase 3.2), when Cranelift
    /// initialised and every eligible function compiled.
    jit: Option<crate::jit::Jit>,
    /// Set once native code bails out (its recursion guard tripped or it
    /// returned a value the JIT cannot represent); the rest of the run stays on
    /// the bytecode VM.
    jit_disabled: bool,
    /// Global symbol of each slot of the native module prefix, in slot order.
    /// Empty when there is no native module entry.
    module_globals: Vec<Symbol>,
}

struct Frame {
    ip: usize,
    instructions: Rc<Vec<Op>>,
    constants: Rc<Vec<Value>>,
    base: usize,
    locals_end: usize,
    defined: u64,
}

impl VM {
    pub fn new(program: Program) -> Self {
        // Compiled first, while the whole `Program` is still available.
        // `NECT_NO_JIT=1` keeps the bytecode VM, for A/B measurements.
        let jit = if std::env::var_os("NECT_NO_JIT").is_some() {
            None
        } else {
            crate::jit::Jit::new(&program)
        };

        let Program {
            instructions,
            constants,
            functions,
            module_slots,
            module_entry,
            interner,
        } = program;

        let mut natives: Vec<Option<NativeFn>> = (0..interner.len()).map(|_| None).collect();
        for name in NATIVE_NAMES {
            if let Some(symbol) = interner.resolve(name) {
                natives[symbol as usize] = Some(native_fn(name));
            }
        }
        let globals: Vec<Option<Value>> = (0..interner.len()).map(|_| None).collect();

        Self {
            stack: vec![Value::Null; module_slots as usize],
            frames: Vec::new(),
            functions,
            natives,
            globals,
            interner,
            ip: 0,
            instructions,
            constants,
            base: 0,
            locals_end: module_slots as usize,
            defined: 0,
            jit,
            jit_disabled: false,
            module_globals: module_entry.map(|entry| entry.globals).unwrap_or_default(),
        }
    }

    pub fn run(&mut self) -> Result<(), RuntimeError> {
        self.run_module_entry();
        loop {
            if self.ip >= self.instructions.len() {
                return Ok(());
            }
            // `Op` is `Copy`, so dispatch never clones an operand.
            let op = self.instructions[self.ip];
            self.ip += 1;
            match op {
                Op::LoadConst(idx) => {
                    let val = self.constants[idx as usize].clone();
                    self.stack.push(val);
                }
                Op::LoadLocal(slot) => {
                    let val = self.stack[self.base + slot as usize].clone();
                    self.stack.push(val);
                }
                Op::LoadLocalChecked { slot, name } => {
                    if !self.is_defined(slot) {
                        return Err(RuntimeError::new(&format!(
                            "undefined variable '{}'",
                            self.interner.name(name)
                        )));
                    }
                    let val = self.stack[self.base + slot as usize].clone();
                    self.stack.push(val);
                }
                Op::StoreLocal(slot) => {
                    debug_assert!(self.has_operands(1), "operand underflow in StoreLocal");
                    let val = self.pop_operand();
                    self.stack[self.base + slot as usize] = val;
                    self.mark_defined(slot);
                }
                Op::LoadGlobal(symbol) => match &self.globals[symbol as usize] {
                    Some(val) => {
                        let val = val.clone();
                        self.stack.push(val);
                    }
                    // A builtin used where a value is expected gets the same
                    // message the interpreter gives, rather than "undefined".
                    None if self.natives[symbol as usize].is_some() => {
                        return Err(RuntimeError::new(&format!(
                            "cannot use '{}' as a value (it is a function)",
                            self.interner.name(symbol)
                        )));
                    }
                    None => {
                        return Err(RuntimeError::new(&format!(
                            "undefined variable '{}'",
                            self.interner.name(symbol)
                        )));
                    }
                },
                Op::StoreGlobal(symbol) => {
                    debug_assert!(self.has_operands(1), "operand underflow in StoreGlobal");
                    let val = self.pop_operand();
                    self.globals[symbol as usize] = Some(val);
                }
                Op::StoreKeep(target) => {
                    let val = self.stack.last().cloned().unwrap_or(Value::Null);
                    self.store(target, val)?;
                }
                Op::BinaryOp(binop) => {
                    debug_assert!(self.has_operands(2), "operand underflow in BinaryOp");
                    let r = self.pop_operand();
                    let l = self.pop_operand();
                    let result = apply_binary(l, binop, r)?;
                    self.stack.push(result);
                }
                Op::UnaryOp(unaryop) => {
                    debug_assert!(self.has_operands(1), "operand underflow in UnaryOp");
                    let v = self.pop_operand();
                    let result = apply_unary(unaryop, v)?;
                    self.stack.push(result);
                }
                Op::BinaryFast { op, lhs, rhs } => {
                    let result = self.binary_fast(op, lhs, rhs)?;
                    self.stack.push(result);
                }
                Op::BinaryStore { op, dst, lhs, rhs } => {
                    let result = self.binary_fast(op, lhs, rhs)?;
                    self.store(dst, result)?;
                }
                Op::JumpIfNot {
                    op,
                    lhs,
                    rhs,
                    target,
                } => {
                    let result = self.binary_fast(op, lhs, rhs)?;
                    if !is_truthy(&result) {
                        self.ip = target as usize;
                    }
                }
                Op::Call(target, arg_count) => {
                    self.call(target, arg_count as usize)?;
                }
                Op::Return => {
                    let val = self.pop_operand();
                    if self.frames.is_empty() {
                        return Err(RuntimeError::new("'return' outside of a function"));
                    }
                    // Truncating to the callee's base releases its locals and any
                    // leftover operands in one step.
                    self.stack.truncate(self.base);
                    let frame = self.frames.pop().unwrap();
                    self.ip = frame.ip;
                    self.instructions = frame.instructions;
                    self.constants = frame.constants;
                    self.base = frame.base;
                    self.locals_end = frame.locals_end;
                    self.defined = frame.defined;
                    self.stack.push(val);
                }
                Op::Jump(target) => {
                    self.ip = target as usize;
                }
                Op::JumpIfFalse(target) => {
                    debug_assert!(self.has_operands(1), "operand underflow in JumpIfFalse");
                    let cond = self.pop_operand();
                    if !is_truthy(&cond) {
                        self.ip = target as usize;
                    }
                }
                Op::JumpIfTrue(target) => {
                    debug_assert!(self.has_operands(1), "operand underflow in JumpIfTrue");
                    let cond = self.pop_operand();
                    if is_truthy(&cond) {
                        self.ip = target as usize;
                    }
                }
                Op::Pop => {
                    // Only ever removes a statement result, never a local.
                    if self.has_operands(1) {
                        self.stack.pop();
                    }
                }
                Op::Halt => return Ok(()),
                Op::MakeArray(count) => {
                    debug_assert!(self.has_operands(count as usize), "operand underflow in MakeArray");
                    let mut elements: Vec<Value> = Vec::with_capacity(count as usize);
                    for _ in 0..count {
                        elements.push(self.pop_operand());
                    }
                    elements.reverse();
                    self.stack.push(Value::Array(Rc::new(RefCell::new(elements))));
                }
                Op::IterList => {
                    debug_assert!(self.has_operands(1), "operand underflow in IterList");
                    let iterable = self.pop_operand();
                    let items = match iterable {
                        Value::Array(elements) => elements.borrow().clone(),
                        Value::Map(map) => map.borrow().keys(),
                        _ => {
                            return Err(RuntimeError::new(
                                "for loop requires an array or map",
                            ))
                        }
                    };
                    self.stack.push(Value::Array(Rc::new(RefCell::new(items))));
                }
                Op::MakeMap(count) => {
                    debug_assert!(self.has_operands(count as usize * 2), "operand underflow in MakeMap");
                    let mut map = Map::new();
                    // Pairs went in value-then-key, forward order; pop them
                    // back-to-front and insert forward to keep insertion order.
                    let mut pairs: Vec<(Value, Value)> = Vec::with_capacity(count as usize);
                    for _ in 0..count {
                        let value = self.pop_operand();
                        let key = self.pop_operand();
                        pairs.push((key, value));
                    }
                    for (key, value) in pairs.into_iter().rev() {
                        map.insert(key, value)?;
                    }
                    self.stack.push(Value::Map(Rc::new(RefCell::new(map))));
                }
                Op::ArrayLen => {
                    debug_assert!(self.has_operands(1), "operand underflow in ArrayLen");
                    let val = self.pop_operand();
                    let len = match &val {
                        Value::Array(arr) => arr.borrow().len() as f64,
                        Value::Map(map) => map.borrow().len() as f64,
                        // `for` requires the same thing the interpreter does.
                        _ => return Err(RuntimeError::new("for loop requires an array or map")),
                    };
                    self.stack.push(Value::Number(len));
                }
                Op::LoadIndex => {
                    debug_assert!(self.has_operands(2), "operand underflow in LoadIndex");
                    let index = self.pop_operand();
                    let target = self.pop_operand();
                    let result = get_index(&target, &index)?;
                    self.stack.push(result);
                }
                Op::StoreIndex => {
                    debug_assert!(self.has_operands(3), "operand underflow in StoreIndex");
                    let value = self.pop_operand();
                    let index = self.pop_operand();
                    let target = self.pop_operand();
                    // Assignment is an expression, so the stored value is the
                    // result; `Pop` discards it in statement position.
                    let stored = set_index(&target, &index, None, value)?;
                    self.stack.push(stored);
                }
                Op::StoreIndexOp(op) => {
                    debug_assert!(self.has_operands(3), "operand underflow in StoreIndexOp");
                    let rhs = self.pop_operand();
                    let index = self.pop_operand();
                    let target = self.pop_operand();
                    let stored = set_index(&target, &index, Some(op), rhs)?;
                    self.stack.push(stored);
                }
            }
        }
    }

    fn call(&mut self, target: CallTarget, arg_count: usize) -> Result<(), RuntimeError> {
        let arg_start = self.stack.len().saturating_sub(arg_count);
        match target {
            CallTarget::Native(symbol) => {
                let native = self.natives[symbol as usize].clone();
                let native = match native {
                    Some(f) => f,
                    None => {
                        return Err(RuntimeError::new(&format!(
                            "undefined function '{}'",
                            self.interner.name(symbol)
                        )));
                    }
                };
                let result = native(&self.stack[arg_start..])?;
                self.stack.truncate(arg_start);
                self.stack.push(result);
            }
            CallTarget::Function(index) => {
                // Phase 3.2: run natively when this function was compiled and the
                // arguments are all numbers, which is exactly what the type pass
                // proved the generated code needs.
                if let Some(compiled) = self.native_candidate(index)
                    && arg_count == compiled.arity as usize
                    && self.stack[arg_start..]
                        .iter()
                        .all(|value| matches!(value, Value::Number(_)))
                {
                    let mut args = [0.0f64; crate::jit::MAX_ARITY as usize];
                    for (slot, value) in self.stack[arg_start..].iter().enumerate() {
                        if let Value::Number(number) = value {
                            args[slot] = *number;
                        }
                    }
                    let depth = self.jit.as_ref().unwrap().depth_ptr();
                    let result =
                        unsafe { crate::jit::call_compiled(compiled, depth, &args[..arg_count]) };
                    match result {
                        Some(number) => {
                            self.stack.truncate(arg_start);
                            self.stack.push(Value::Number(number));
                            return Ok(());
                        }
                        // The native depth guard tripped: re-run this call on the
                        // bytecode VM, which recurses on the heap. The rest of the
                        // run stays in bytecode so the two don't ping-pong.
                        None => self.jit_disabled = true,
                    }
                }
                let function = &self.functions[index as usize];
                if arg_count != function.param_count as usize {
                    return Err(RuntimeError::new(&format!(
                        "function '{}' expects {} argument(s), got {}",
                        self.interner.name(function.name),
                        function.param_count,
                        arg_count
                    )));
                }
                let instructions = function.instructions.clone();
                let constants = function.constants.clone();
                let num_slots = function.num_slots as usize;

                // The arguments are the topmost operands, so the callee's frame
                // starts exactly where they begin: slots 0..arg_count already
                // hold the parameters and no copy is needed. Only the callee's
                // remaining locals are appended.
                debug_assert!(arg_start >= self.locals_end, "call overwrote caller locals");
                debug_assert!(num_slots >= arg_count, "frame too small for its arguments");
                self.stack.resize(arg_start + num_slots, Value::Null);

                self.frames.push(Frame {
                    ip: self.ip,
                    instructions: std::mem::replace(&mut self.instructions, instructions),
                    constants: std::mem::replace(&mut self.constants, constants),
                    base: std::mem::replace(&mut self.base, arg_start),
                    locals_end: std::mem::replace(&mut self.locals_end, arg_start + num_slots),
                    defined: std::mem::replace(&mut self.defined, 0),
                });
                self.ip = 0;
            }
        }
        Ok(())
    }

    /// Runs the module-level numeric prefix natively (performance plan phase
    /// 3.2, "JIT top-level code") and resumes bytecode where it stopped.
    ///
    /// Native module code cannot produce output — inference refuses to compile
    /// anything that calls a builtin or touches a non-number — so re-running the
    /// module body in bytecode after a bail-out repeats no observable effect.
    fn run_module_entry(&mut self) {
        if self.ip != 0 || !self.frames.is_empty() || self.jit_disabled {
            return;
        }
        let Some(jit) = self.jit.as_ref() else {
            return;
        };
        let Some(entry) = jit.module_entry() else {
            return;
        };
        let mut out = vec![0.0f64; self.module_globals.len()];
        debug_assert_eq!(out.len(), entry.slots as usize, "module mirror slots");
        let depth = jit.depth_ptr();
        match unsafe { crate::jit::call_module(entry, depth, &mut out) } {
            Some(_) => {
                for (slot, symbol) in self.module_globals.iter().enumerate() {
                    self.globals[*symbol as usize] = Some(Value::Number(out[slot]));
                }
                self.ip = entry.resume as usize;
            }
            // The depth guard tripped somewhere in the native call chain: redo
            // the module body from the top on the bytecode VM, which recurses on
            // the heap instead of the host stack.
            None => {
                self.jit_disabled = true;
                self.ip = 0;
            }
        }
    }

    /// The compiled version of `index`, when native execution is still allowed.
    fn native_candidate(&self, index: u32) -> Option<crate::jit::JitFunction> {
        if self.jit_disabled {
            return None;
        }
        self.jit.as_ref()?.function(index)
    }

    /// Whether the frame still has `count` operands above its locals.
    fn has_operands(&self, count: usize) -> bool {
        self.stack.len() >= self.locals_end + count
    }

    /// Pops an operand, leaving locals untouched when the operand area is empty
    /// (only reachable from a `return` at module level, which is an error).
    fn pop_operand(&mut self) -> Value {
        if self.has_operands(1) {
            self.stack.pop().unwrap_or(Value::Null)
        } else {
            Value::Null
        }
    }

    fn store(&mut self, target: Fusee, value: Value) -> Result<(), RuntimeError> {
        match target.tag() {
            TAG_LOCAL => {
                self.stack[self.base + target.index() as usize] = value;
                self.mark_defined(target.index());
                Ok(())
            }
            TAG_GLOBAL => {
                self.globals[target.index() as usize] = Some(value);
                Ok(())
            }
            TAG_GLOBAL_CHECKED => {
                // Matches the interpreter: assigning to a name that was never
                // declared (and is not a builtin) is an error rather than an
                // implicit definition, so typos surface.
                let index = target.index() as usize;
                if self.globals[index].is_none() && self.natives[index].is_none() {
                    return Err(RuntimeError::new(&format!(
                        "undefined variable '{}'",
                        self.interner.name(target.index())
                    )));
                }
                self.globals[index] = Some(value);
                Ok(())
            }
            _ => Err(RuntimeError::new("cannot assign to a constant")),
        }
    }

    /// Whether a slot has been written this frame. Untracked (very large)
    /// frames and slots always count as written.
    fn is_defined(&self, slot: u32) -> bool {
        slot >= MAX_TRACKED_SLOTS || self.defined & (1u64 << slot) != 0
    }

    fn mark_defined(&mut self, slot: u32) {
        if slot < MAX_TRACKED_SLOTS {
            self.defined |= 1u64 << slot;
        }
    }

    /// Reads an operand as a number, without cloning, when it holds one.
    fn number_of(&self, operand: Fusee) -> Option<f64> {
        let value = match operand.tag() {
            TAG_LOCAL => &self.stack[self.base + operand.index() as usize],
            TAG_GLOBAL | TAG_GLOBAL_CHECKED => self.globals[operand.index() as usize].as_ref()?,
            _ => &self.constants[operand.index() as usize],
        };
        match value {
            Value::Number(n) => Some(*n),
            _ => None,
        }
    }

    /// Reads an operand, erroring for globals that were never bound.
    fn read_operand(&self, operand: Fusee) -> Result<Value, RuntimeError> {
        match operand.tag() {
            TAG_LOCAL => Ok(self.stack[self.base + operand.index() as usize].clone()),
            TAG_GLOBAL | TAG_GLOBAL_CHECKED => {
                let index = operand.index() as usize;
                match &self.globals[index] {
                    Some(value) => Ok(value.clone()),
                    None => Err(RuntimeError::new(&format!(
                        "undefined variable '{}'",
                        self.interner.name(operand.index())
                    ))),
                }
            }
            _ => Ok(self.constants[operand.index() as usize].clone()),
        }
    }

    /// `lhs op rhs` with a numeric fast path (2.2) that skips the general
    /// type-dispatch, falling back for strings, errors, and undefined globals.
    fn binary_fast(&self, op: BinaryOp, lhs: Fusee, rhs: Fusee) -> Result<Value, RuntimeError> {
        if let (Some(a), Some(b)) = (self.number_of(lhs), self.number_of(rhs))
            && let Some(result) = numeric_binary(op, a, b)
        {
            return Ok(result);
        }
        let l = self.read_operand(lhs)?;
        let r = self.read_operand(rhs)?;
        apply_binary(l, op, r)
    }
}

// Value semantics (`apply_binary`, `apply_unary`, `numeric_binary`,
// `is_truthy`, `type_name`) and the builtin library live in `crate::builtins`,
// shared with the reference interpreter so the two engines cannot drift.
