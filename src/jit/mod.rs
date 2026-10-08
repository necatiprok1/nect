//! Performance plan phase 3: type inference and native code generation.
//!
//! Pipeline:
//!   3.3 infer a type for every value a function manipulates, which decides
//!       whether the function is provably numeric
//!   3.2 lower provably-numeric functions to machine code with Cranelift,
//!       guarded by the caller's argument types, with the bytecode VM as the
//!       fallback for everything that is not provably safe
//!
//! There is no separate Nect IR: the bytecode is dense and stack-free enough
//! (see [`Fusee`]) that it lowers straight into Cranelift IR, which is itself in
//! SSA form. A generated function that trips the native recursion guard returns
//! [`BAIL`] and the VM re-runs that call on the bytecode VM, which recurses on
//! the heap instead of the host stack.

use crate::ast::{BinaryOp, UnaryOp, Value};
use crate::vm::{
    CallTarget, CompiledFunction, Fusee, ModuleEntry, Op, Program, TAG_CONST, TAG_LOCAL,
};
use cranelift_codegen::ir::condcodes::{FloatCC, IntCC};
use cranelift_codegen::ir::{AbiParam, Block, InstBuilder, MemFlagsData, Value as ClValue, types};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext, Variable};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{FuncId, Linkage, Module};
use std::collections::HashSet;

/// Most parameters a JIT-compiled function may take (the VM marshals arguments
/// by transmuting to a fixed-arity C signature).
pub const MAX_ARITY: u32 = 4;

/// Native frames one JIT-ed call chain may use before it bails to the VM.
const MAX_NATIVE_DEPTH: i64 = 4096;

/// Value returned when the depth guard trips: a quiet NaN carrying a payload
/// arithmetic never produces, so it cannot be confused with a real result.
const BAIL: u64 = 0x7FF8_0000_0000_0001;

/// Inferred type of a value (phase 3.3). Booleans are represented as `0.0`/`1.0`
/// in native code, which preserves truthiness and comparison behaviour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ty {
    Num,
    Bool,
}

#[derive(Debug, Clone)]
pub struct FunctionAnalysis {
    /// Whether the function could be compiled.
    pub eligible: bool,
    /// Why the function cannot be compiled, when it cannot.
    pub reason: Option<&'static str>,
    /// Whether it is *worth* compiling: something in the program repeats work
    /// (a loop, recursion, or a call inside a loop). See [`Analysis`].
    pub hot: bool,
    /// Inferred type per local slot (`None` = never written).
    pub slot_types: Vec<Option<Ty>>,
}

/// What native compilation can do with the module body: a *prefix* of it.
///
/// Module-level code is not a function, so it is compiled as a prefix — native
/// code runs from instruction 0 until the first point the type pass refuses,
/// then the VM resumes bytecode at [`ModuleAnalysis::resume`].
#[derive(Debug, Clone)]
pub struct ModuleAnalysis {
    /// Whether the module's numeric prefix passed the type pass.
    pub compiled: bool,
    /// Whether that prefix is worth compiling (it contains a loop).
    pub hot: bool,
    /// Module-body instruction the bytecode VM resumes at.
    pub resume: u32,
    /// Why the module body is bytecode-only, when it is.
    pub reason: Option<&'static str>,
}

/// The whole-program inference result, and the compilation policy built on it.
///
/// # Why not compile everything provable? (performance plan 3.2, "profile-guided")
///
/// Cranelift code generation costs a fixed ~40µs to initialise plus ~110µs per
/// function, measured on this codebase. A function with no loop and no recursion,
/// called once from straight-line code, runs a handful of instructions, so
/// compiling it can only make the program slower. Native code is therefore only
/// generated for code that *repeats*:
///
/// - a body containing a loop,
/// - a recursive function (self or mutual), or
/// - a function called from inside a loop body (in any function, or in the
///   module body).
///
/// A call-count threshold was the alternative, but without on-stack replacement
/// it regresses the shape that needs the JIT most: a heavy loop inside a single
/// call. Waiting for evidence cannot accelerate the first call, so any code that
/// repeats is compiled up front instead.
#[derive(Debug, Clone)]
pub struct Analysis {
    pub functions: Vec<FunctionAnalysis>,
    /// `None` when there is no module entry to compile at all.
    pub module: Option<ModuleAnalysis>,
}

impl Analysis {
    pub fn eligible(&self, index: u32) -> bool {
        self.functions
            .get(index as usize)
            .is_some_and(|f| f.eligible)
    }

    /// Whether anything at all will be compiled natively.
    pub fn any_compiled(&self) -> bool {
        self.functions.iter().any(|f| f.hot) || self.module.as_ref().is_some_and(|m| m.hot)
    }

    /// Human-readable inference report for `nect disasm`.
    pub fn describe(&self, program: &Program) -> String {
        let mut out = String::new();
        if let Some(module) = &self.module {
            out.push_str("--- module: ");
            match (module.compiled, module.hot, module.reason) {
                (true, true, _) => out.push_str(&format!(
                    "numeric prefix with a loop, native up to instruction {} (rest in bytecode)\n",
                    module.resume
                )),
                (true, false, _) => out.push_str(
                    "numeric prefix, kept in bytecode (no loop to repay native compilation)\n",
                ),
                (false, _, reason) => out.push_str(&format!(
                    "bytecode only ({})\n",
                    reason.unwrap_or("not eligible")
                )),
            }
        }
        for (index, info) in self.functions.iter().enumerate() {
            let name = program.interner.name(program.functions[index].name);
            out.push_str(&format!("--- fn {name}: "));
            match (info.eligible, info.hot) {
                (true, true) => out.push_str("numeric, jit-eligible"),
                (true, false) => out.push_str(
                    "numeric, kept in bytecode (nothing repeats to repay native compilation)",
                ),
                (false, _) => out.push_str(&format!(
                    "bytecode only ({})",
                    info.reason.unwrap_or("not eligible")
                )),
            }
            out.push('\n');
            for (slot, ty) in info.slot_types.iter().enumerate() {
                let ty = match ty {
                    Some(Ty::Num) => "number",
                    Some(Ty::Bool) => "boolean",
                    None => "never written",
                };
                out.push_str(&format!("    slot {slot}: {ty}\n"));
            }
        }
        out
    }
}

/// Infers value types and marks the functions that can be compiled.
pub fn analyze(program: &Program) -> Analysis {
    let count = program.functions.len();
    let mut functions: Vec<FunctionAnalysis> = Vec::with_capacity(count);
    let mut callees: Vec<Vec<u32>> = Vec::with_capacity(count);

    for function in &program.functions {
        match infer(function, InferMode::Function) {
            Ok((slot_types, calls)) => {
                functions.push(FunctionAnalysis {
                    eligible: true,
                    reason: None,
                    hot: false,
                    slot_types,
                });
                callees.push(calls);
            }
            Err(reason) => {
                functions.push(FunctionAnalysis {
                    eligible: false,
                    reason: Some(reason),
                    hot: false,
                    slot_types: vec![None; function.num_slots as usize],
                });
                callees.push(Vec::new());
            }
        }
    }

    // A function is only compilable if everything it calls is too: the caller's
    // generated code calls the callee natively.
    loop {
        let mut changed = false;
        for index in 0..count {
            if !functions[index].eligible {
                continue;
            }
            if callees[index]
                .iter()
                .any(|callee| !functions[*callee as usize].eligible)
            {
                functions[index].eligible = false;
                functions[index].reason = Some("calls a function that is not compiled");
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }

    // The module body is compiled as a prefix, after the functions are known so
    // a prefix that calls a function can be rejected when it is not eligible.
    let mut module = program
        .module_entry
        .as_ref()
        .map(|entry| plan_module(entry, &functions));

    // Decide what is worth compiling (see [`Analysis`]), now that the module
    // prefix's instruction range is known: a prefix is only compiled when it
    // contains a loop, in which case everything it can call natively must be
    // compiled with it.
    let closures = transitive_callees(&callees);
    let mut repeated: Vec<bool> = program
        .functions
        .iter()
        .map(|function| contains_loop(&function.instructions, function.instructions.len()))
        .collect();
    for (index, closure) in closures.iter().enumerate() {
        // A function that can reach itself is recursive, so it runs repeatedly.
        if closure.contains(&(index as u32)) {
            repeated[index] = true;
        }
    }
    for callee in loop_callsites(&program.instructions, program.instructions.len()) {
        repeated[callee as usize] = true;
    }
    for function in &program.functions {
        for callee in loop_callsites(&function.instructions, function.instructions.len()) {
            repeated[callee as usize] = true;
        }
    }

    let mut hot = vec![false; program.functions.len()];
    for (index, worth) in repeated.iter().enumerate() {
        if *worth && functions[index].eligible {
            mark_hot(index as u32, &mut hot, &closures);
        }
    }
    if let Some(entry) = &program.module_entry
        && let Some(plan) = &module
        && plan.compiled
    {
        let prefix_has_loop = contains_loop(&entry.function.instructions, plan.resume as usize);
        if prefix_has_loop {
            for callee in calls_in(&entry.function.instructions, plan.resume as usize) {
                mark_hot(callee, &mut hot, &closures);
            }
        }
        if let Some(plan) = &mut module {
            plan.hot = prefix_has_loop;
        }
    }
    for (index, compiled) in hot.iter().enumerate() {
        functions[index].hot = *compiled;
    }

    Analysis { functions, module }
}

/// Marks a function and everything it can call natively as worth compiling.
fn mark_hot(index: u32, hot: &mut [bool], closures: &[HashSet<u32>]) {
    hot[index as usize] = true;
    for callee in &closures[index as usize] {
        hot[*callee as usize] = true;
    }
}

/// Transitive callees of every function, excluding the function itself unless it
/// is (mutually) recursive.
fn transitive_callees(callees: &[Vec<u32>]) -> Vec<HashSet<u32>> {
    let mut closures: Vec<HashSet<u32>> = Vec::with_capacity(callees.len());
    for direct in callees {
        let mut seen: HashSet<u32> = HashSet::new();
        let mut work: Vec<u32> = direct.clone();
        while let Some(next) = work.pop() {
            if !seen.insert(next) {
                continue;
            }
            if let Some(more) = callees.get(next as usize) {
                work.extend(more.iter().copied());
            }
        }
        closures.push(seen);
    }
    closures
}

/// Whether `[0, limit)` contains a backward branch, i.e. a loop.
fn contains_loop(instructions: &[Op], limit: usize) -> bool {
    instructions
        .iter()
        .enumerate()
        .take(limit)
        .any(|(position, op)| {
            op.jump_target()
                .is_some_and(|target| target as usize <= position)
        })
}

/// Call targets reached from inside a loop body, i.e. calls that run once per
/// iteration.
fn loop_callsites(instructions: &[Op], limit: usize) -> Vec<u32> {
    let mut calls = Vec::new();
    for (position, op) in instructions.iter().enumerate().take(limit) {
        let Op::Call(CallTarget::Function(index), _) = op else {
            continue;
        };
        let in_loop = instructions
            .iter()
            .enumerate()
            .take(limit)
            .any(|(back, candidate)| {
                candidate
                    .jump_target()
                    .is_some_and(|target| (target as usize) <= position && position < back)
            });
        if in_loop {
            calls.push(*index);
        }
    }
    calls
}

/// Every function called by `[0, limit)`.
fn calls_in(instructions: &[Op], limit: usize) -> Vec<u32> {
    instructions
        .iter()
        .take(limit)
        .filter_map(|op| match op {
            Op::Call(CallTarget::Function(index), _) => Some(*index),
            _ => None,
        })
        .collect()
}

/// Picks the longest module prefix native code can run.
///
/// The candidate boundaries are the module's own statement starts (where a
/// branch may leave native code) and the end of the body; the longest one that
/// type-checks wins.
fn plan_module(entry: &ModuleEntry, functions: &[FunctionAnalysis]) -> ModuleAnalysis {
    let mut candidates: Vec<u32> = Vec::with_capacity(entry.statements.len() + 1);
    candidates.push(entry.halt);
    candidates.extend(
        entry
            .statements
            .iter()
            .rev()
            .copied()
            .filter(|start| *start < entry.halt),
    );
    // The longest candidate's failure is the informative one: it is the point
    // where the module's own code stops being provably numeric.
    let mut reason = None;
    for cut in candidates {
        match infer_module_prefix(&entry.function, cut as usize, functions) {
            Ok(()) => {
                return ModuleAnalysis {
                    compiled: true,
                    hot: false,
                    resume: cut,
                    reason: None,
                };
            }
            Err(failure) => reason = reason.or(Some(failure)),
        }
    }
    ModuleAnalysis {
        compiled: false,
        hot: false,
        resume: 0,
        reason: reason.or(Some("nothing to compile")),
    }
}

/// Checks that `[0, cut)` of the mirrored module body is provably numeric, ends
/// cleanly at `cut`, and only calls compiled functions.
fn infer_module_prefix(
    function: &CompiledFunction,
    cut: usize,
    functions: &[FunctionAnalysis],
) -> Result<(), &'static str> {
    let (slots, calls) = infer(function, InferMode::ModulePrefix(cut))?;
    if !hands_back(function, cut)? {
        return Err("the rest of the module is unreachable from here");
    }
    // Every slot is one mirrored global. A slot a reachable instruction never
    // assigns is one the bytecode never binds either, so writing `0.0` back for
    // it would turn "undefined variable" into a silent zero.
    if slots.iter().any(|ty| *ty != Some(Ty::Num)) {
        return Err("a module-level value is assigned only on some paths");
    }
    if calls
        .iter()
        .any(|callee| !functions[*callee as usize].eligible)
    {
        return Err("calls a function that is not compiled");
    }
    Ok(())
}

/// Whether executing instructions `[0, cut)` reaches `cut`: either some branch
/// targets it, or the last reachable instruction falls through into it. Without
/// this, native code would never hand back to the bytecode VM (and the exit
/// block would have no predecessors).
fn hands_back(function: &CompiledFunction, cut: usize) -> Result<bool, &'static str> {
    let instructions = &function.instructions;
    let reachable = reachable_instructions(instructions, cut)?;
    for (index, op) in instructions.iter().enumerate().take(cut) {
        if reachable[index] && op.jump_target() == Some(cut as u32) {
            return Ok(true);
        }
    }
    // Otherwise the prefix has to run off its end: a conditional or
    // unconditional branch there cannot fall through.
    let last = (0..cut).rev().find(|index| reachable[*index]);
    Ok(match last {
        Some(index) => !matches!(
            instructions[index],
            Op::Jump(_)
                | Op::JumpIfFalse(_)
                | Op::JumpIfTrue(_)
                | Op::JumpIfNot { .. }
                | Op::Return
                | Op::Halt
        ),
        None => false,
    })
}

/// Which slice of a function the inference is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InferMode {
    /// The whole body, which must return.
    Function,
    /// Instructions `[0, cut)` of the mirrored module body: a branch to `cut`
    /// hands control back to the bytecode VM.
    ModulePrefix(usize),
}

/// Walks a function's reachable instructions, checking that every value is
/// numeric and recording which functions it calls.
fn infer(
    function: &CompiledFunction,
    mode: InferMode,
) -> Result<(Vec<Option<Ty>>, Vec<u32>), &'static str> {
    if function.param_count > MAX_ARITY {
        return Err("too many parameters");
    }
    let instructions = &function.instructions;
    let limit = match mode {
        InferMode::Function => instructions.len(),
        InferMode::ModulePrefix(cut) => cut.min(instructions.len()),
    };
    let reachable = reachable_instructions(instructions, limit)?;
    if mode == InferMode::Function
        && !instructions
            .iter()
            .enumerate()
            .any(|(index, op)| reachable[index] && matches!(op, Op::Return))
    {
        return Err("no reachable return");
    }

    let mut slots: Vec<Option<Ty>> = vec![None; function.num_slots as usize];
    // Parameters are checked by the caller before entering native code.
    for slot in slots.iter_mut().take(function.param_count as usize) {
        *slot = Some(Ty::Num);
    }
    let mut stack: Vec<Ty> = Vec::new();
    let mut calls: Vec<u32> = Vec::new();

    for index in 0..limit {
        let op = &instructions[index];
        if !reachable[index] {
            continue;
        }
        match op {
            Op::LoadConst(constant) => match &function.constants[*constant as usize] {
                Value::Number(_) => stack.push(Ty::Num),
                Value::Boolean(_) => stack.push(Ty::Bool),
                _ => return Err("loads a non-numeric constant"),
            },
            Op::LoadLocal(slot) => match slots.get(*slot as usize).copied().flatten() {
                Some(ty) => stack.push(ty),
                None => return Err("reads a slot that is never written"),
            },
            Op::StoreLocal(slot) => {
                let ty = mirrorable_store(mode, stack.pop().ok_or("operand stack underflow")?)?;
                let slot = slots.get_mut(*slot as usize).ok_or("slot out of range")?;
                *slot = Some(ty);
            }
            // Globals can hold anything, including strings, and can be mutated
            // between calls, so they are left to the bytecode VM.
            Op::LoadGlobal(_) | Op::StoreGlobal(_) => return Err("touches a global"),
            Op::StoreKeep(target) => {
                let ty = mirrorable_store(mode, *stack.last().ok_or("operand stack underflow")?)?;
                match target.tag() {
                    TAG_LOCAL => {
                        let slot = slots
                            .get_mut(target.index() as usize)
                            .ok_or("slot out of range")?;
                        *slot = Some(ty);
                    }
                    _ => return Err("stores to a global or constant"),
                }
            }
            Op::UnaryOp(UnaryOp::Not) => {
                stack.pop().ok_or("operand stack underflow")?;
                stack.push(Ty::Bool);
            }
            Op::UnaryOp(UnaryOp::Negate) => {
                require_num(stack.pop(), "negate a non-number")?;
                stack.push(Ty::Num);
            }
            Op::BinaryOp(op) => {
                let right = stack.pop().ok_or("operand stack underflow")?;
                let left = stack.pop().ok_or("operand stack underflow")?;
                stack.push(numeric_result(*op, left, right)?);
            }
            Op::BinaryFast { op, lhs, rhs } => {
                let left = operand_ty(function, &slots, *lhs)?;
                let right = operand_ty(function, &slots, *rhs)?;
                stack.push(numeric_result(*op, left, right)?);
            }
            Op::BinaryStore { op, dst, lhs, rhs } => {
                if dst.tag() != TAG_LOCAL {
                    return Err("stores to a global or constant");
                }
                let left = operand_ty(function, &slots, *lhs)?;
                let right = operand_ty(function, &slots, *rhs)?;
                let result = mirrorable_store(mode, numeric_result(*op, left, right)?)?;
                let slot = slots
                    .get_mut(dst.index() as usize)
                    .ok_or("slot out of range")?;
                *slot = Some(result);
            }
            Op::JumpIfNot { op, lhs, rhs, .. } => {
                if !stack.is_empty() {
                    return Err("jump with a non-empty operand stack");
                }
                let left = operand_ty(function, &slots, *lhs)?;
                let right = operand_ty(function, &slots, *rhs)?;
                numeric_result(*op, left, right)?;
            }
            Op::JumpIfFalse(_) => {
                // Both numbers and booleans are truthiness-checkable.
                stack.pop().ok_or("operand stack underflow")?;
                if !stack.is_empty() {
                    return Err("jump with a non-empty operand stack");
                }
            }
            // Short-circuit operators merge control flow with values on the
            // stack, which this straight-line pass cannot follow.
            Op::JumpIfTrue(_) => return Err("uses a short-circuit merge point"),
            Op::Jump(_) => {
                if !stack.is_empty() {
                    return Err("jump with a non-empty operand stack");
                }
            }
            // Both a builtin and a declared extern are native calls; neither
            // can be inlined, so the function stays on the bytecode VM.
            Op::Call(CallTarget::Native(_), _) => return Err("calls a builtin or extern"),
            Op::Call(CallTarget::Function(index), arg_count) => {
                for _ in 0..*arg_count {
                    require_num(stack.pop(), "calls with a non-number argument")?;
                }
                calls.push(*index);
                // The callee returns a number, otherwise it would not be
                // eligible and this function would be rejected by the fixpoint.
                stack.push(Ty::Num);
            }
            Op::Return => {
                if mode != InferMode::Function {
                    // `return` outside a function is a runtime error, which
                    // native code cannot raise.
                    return Err("returns at module level");
                }
                if stack.len() != 1 {
                    return Err("return with a non-empty operand stack");
                }
                require_num(stack.pop(), "returns a non-number")?;
            }
            Op::Pop => {
                stack.pop().ok_or("operand stack underflow")?;
            }
            // Declared inside an `if`/`while` body: the slot may never have been
            // written, which native code cannot report as an error.
            Op::LoadLocalChecked { .. } => return Err("reads a conditional declaration"),
            // Arrays and maps are heap values, which native code does not speak.
            Op::MakeArray(_)
            | Op::ArrayLen
            | Op::LoadIndex
            | Op::StoreIndex
            | Op::StoreIndexOp(_) => {
                return Err("uses arrays");
            }
            Op::MakeMap(_) => {
                return Err("uses maps");
            }
            Op::IterList => {
                return Err("iterates a map");
            }
            Op::Halt => return Err("contains a module-level halt"),
        }
    }

    // Statically unreachable code was never analysed, so make sure every
    // remaining instruction is one the code generator understands.
    for index in 0..limit {
        if reachable[index] {
            continue;
        }
        if matches!(
            instructions[index],
            Op::LoadLocalChecked { .. } | Op::JumpIfTrue(_)
        ) {
            return Err("unreachable code needs the bytecode VM");
        }
    }

    Ok((slots, calls))
}

/// Reachable instructions below `limit`. A branch to exactly `limit` is the
/// module prefix's exit (analysed by the bytecode VM instead); in
/// [`InferMode::Function`] `limit` is the end of the body, so a branch there is
/// out of range.
fn reachable_instructions(instructions: &[Op], limit: usize) -> Result<Vec<bool>, &'static str> {
    let count = instructions.len();
    let mut reachable = vec![false; count];
    let mut work = vec![0usize];
    while let Some(index) = work.pop() {
        if index > limit || index >= count {
            return Err("jump past the end of the function");
        }
        if index == limit {
            // A module prefix hands control back to the bytecode VM here.
            continue;
        }
        if std::mem::replace(&mut reachable[index], true) {
            continue;
        }
        match &instructions[index] {
            Op::Return | Op::Halt => {}
            Op::Jump(target) => work.push(*target as usize),
            Op::JumpIfFalse(target) | Op::JumpIfTrue(target) => {
                work.push(*target as usize);
                work.push(index + 1);
            }
            Op::JumpIfNot { target, .. } => {
                work.push(*target as usize);
                work.push(index + 1);
            }
            _ => work.push(index + 1),
        }
    }
    Ok(reachable)
}

/// A store into a mirror slot. The VM represents booleans as `Value::Boolean`
/// while native code uses `0.0`/`1.0`, and the mirrored globals are written back
/// as numbers, so a slot that can ever hold a boolean is not compilable — even
/// when a later store turns it back into a number.
fn mirrorable_store(mode: InferMode, ty: Ty) -> Result<Ty, &'static str> {
    if mode != InferMode::Function && ty != Ty::Num {
        return Err("a module-level value may not be a boolean");
    }
    Ok(ty)
}

fn require_num(ty: Option<Ty>, reason: &'static str) -> Result<(), &'static str> {
    match ty {
        Some(Ty::Num) => Ok(()),
        _ => Err(reason),
    }
}

fn operand_ty(
    function: &CompiledFunction,
    slots: &[Option<Ty>],
    operand: Fusee,
) -> Result<Ty, &'static str> {
    match operand.tag() {
        TAG_LOCAL => slots
            .get(operand.index() as usize)
            .copied()
            .flatten()
            .ok_or("reads a slot that is never written"),
        TAG_CONST => match &function.constants[operand.index() as usize] {
            Value::Number(_) => Ok(Ty::Num),
            Value::Boolean(_) => Ok(Ty::Bool),
            _ => Err("loads a non-numeric constant"),
        },
        _ => Err("touches a global"),
    }
}

/// Result type of `left op right` when both operands are numeric.
fn numeric_result(op: BinaryOp, left: Ty, right: Ty) -> Result<Ty, &'static str> {
    match op {
        // `+` can concatenate strings in the general path, and the JIT only
        // handles numbers, so both operands must already be numbers.
        BinaryOp::Add | BinaryOp::Subtract | BinaryOp::Multiply => {
            require_num(Some(left), "arithmetic on a non-number")?;
            require_num(Some(right), "arithmetic on a non-number")?;
            Ok(Ty::Num)
        }
        // Division still produces a number, but a zero divisor has to raise the
        // same runtime error the VM does. The generated code guards the divisor
        // and bails out to the bytecode VM when it is zero, which re-runs the
        // call and reports the error from there.
        BinaryOp::Divide => {
            require_num(Some(left), "arithmetic on a non-number")?;
            require_num(Some(right), "arithmetic on a non-number")?;
            Ok(Ty::Num)
        }
        // Cranelift has no floating-point remainder instruction, and calling
        // `fmod` for it is not worth a foreign call on the rare hot path that
        // needs it: modulo stays in bytecode.
        BinaryOp::Modulo => Err("uses modulo"),
        BinaryOp::Less
        | BinaryOp::Greater
        | BinaryOp::LessEqual
        | BinaryOp::GreaterEqual
        | BinaryOp::Equal
        | BinaryOp::NotEqual => {
            require_num(Some(left), "compares a non-number")?;
            require_num(Some(right), "compares a non-number")?;
            Ok(Ty::Bool)
        }
        BinaryOp::And | BinaryOp::Or => Err("uses short-circuit logic"),
    }
}

/// A compiled native function: its code pointer and arity.
#[derive(Clone, Copy, Debug)]
pub struct JitFunction {
    pub code: *const u8,
    pub arity: u32,
}

/// A compiled native module prefix: where its code lives, how many `f64`s it
/// writes back (the mirrored globals), and the module-body instruction the
/// bytecode VM resumes at.
#[derive(Clone, Copy, Debug)]
pub struct JitModuleEntry {
    pub code: *const u8,
    pub slots: u32,
    pub resume: u32,
}

/// Bottom half of the JIT: the executable module and its compiled functions.
pub struct Jit {
    /// Owns the executable memory the compiled functions live in, so it must
    /// outlive every call made through [`JitFunction`].
    _module: JITModule,
    /// Native depth counter shared by all compiled functions.
    depth: Box<isize>,
    functions: Vec<Option<JitFunction>>,
    module: Option<JitModuleEntry>,
    analysis: Analysis,
}

impl Jit {
    pub fn analysis(&self) -> &Analysis {
        &self.analysis
    }

    pub fn function(&self, index: u32) -> Option<JitFunction> {
        self.functions.get(index as usize).copied().flatten()
    }

    /// The compiled module prefix, when the module body could be compiled.
    pub fn module_entry(&self) -> Option<JitModuleEntry> {
        self.module
    }

    /// Pointer to the shared depth counter, passed to native code as a hidden
    /// first argument.
    pub fn depth_ptr(&self) -> *mut isize {
        self.depth.as_ref() as *const isize as *mut isize
    }

    /// Compiles every eligible function. Returns `None` (and the VM keeps
    /// running bytecode) if Cranelift cannot be initialised or a function fails
    /// to compile; a partially built module is discarded rather than trusted.
    pub fn new(program: &Program) -> Option<Jit> {
        let analysis = analyze(program);
        if !analysis.any_compiled() {
            return None;
        }
        // Compilation runs arbitrary codegen; a bug there must degrade to the
        // bytecode VM, not abort the process.
        let hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let built = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            build(program, analysis.clone())
        }));
        std::panic::set_hook(hook);
        match built {
            Ok(Some(jit)) => Some(jit),
            _ => None,
        }
    }
}

/// Native call frame: `extern "C" fn(depth, args...) -> f64`.
///
/// # Safety
/// `function` must come from [`Jit::function`] and the argument count must
/// match its arity.
/// Native module entry: `extern "C" fn(depth, out) -> f64`.
///
/// # Safety
/// `entry` must come from [`Jit::module_entry`] and `out` must have exactly
/// `entry.slots` elements.
pub unsafe fn call_module(
    entry: JitModuleEntry,
    depth: *mut isize,
    out: &mut [f64],
) -> Option<f64> {
    debug_assert_eq!(out.len(), entry.slots as usize, "module prefix slot count");
    let code = entry.code;
    #[allow(clippy::missing_transmute_annotations)]
    let result = unsafe {
        std::mem::transmute::<_, extern "C" fn(*mut isize, *mut f64) -> f64>(code)(
            depth,
            out.as_mut_ptr(),
        )
    };
    if result.to_bits() == BAIL {
        None
    } else {
        Some(result)
    }
}

#[allow(clippy::missing_safety_doc)]
pub unsafe fn call_compiled(function: JitFunction, depth: *mut isize, args: &[f64]) -> Option<f64> {
    // The generated signature is `extern "C" fn(*mut isize, f64..) -> f64`; the
    // arity is part of `JitFunction` and checked against the call site.
    let code = function.code;
    #[allow(clippy::missing_transmute_annotations)]
    let result = unsafe {
        match function.arity {
            0 => std::mem::transmute::<_, extern "C" fn(*mut isize) -> f64>(code)(depth),
            1 => std::mem::transmute::<_, extern "C" fn(*mut isize, f64) -> f64>(code)(
                depth, args[0],
            ),
            2 => std::mem::transmute::<_, extern "C" fn(*mut isize, f64, f64) -> f64>(code)(
                depth, args[0], args[1],
            ),
            3 => std::mem::transmute::<_, extern "C" fn(*mut isize, f64, f64, f64) -> f64>(code)(
                depth, args[0], args[1], args[2],
            ),
            4 => std::mem::transmute::<_, extern "C" fn(*mut isize, f64, f64, f64, f64) -> f64>(
                code,
            )(depth, args[0], args[1], args[2], args[3]),
            _ => return None,
        }
    };
    if result.to_bits() == BAIL {
        None
    } else {
        Some(result)
    }
}

fn build(program: &Program, analysis: Analysis) -> Option<Jit> {
    // Compilation cost is what decides whether compiling is worth it at all (see
    // [`Analysis`]), so `NECT_JIT_TIMING=1` reports where it goes.
    let clock = std::time::Instant::now();
    let builder = JITBuilder::new(cranelift_module::default_libcall_names()).ok()?;
    let after_builder = clock.elapsed();
    let mut module = JITModule::new(builder);
    let after_module = clock.elapsed();

    let mut ids: Vec<Option<FuncId>> = vec![None; program.functions.len()];
    for (index, info) in analysis.functions.iter().enumerate() {
        if !info.hot {
            continue;
        }
        let function = &program.functions[index];
        let name = format!("nect_fn_{index}");
        let id = module
            .declare_function(&name, Linkage::Local, &signature(&module, function))
            .ok()?;
        ids[index] = Some(id);
    }

    for (index, info) in analysis.functions.iter().enumerate() {
        if !info.hot {
            continue;
        }
        // If anything at all fails, give up on the whole module: callers may
        // already reference the function, and a half-defined JIT must not run.
        compile_function(&mut module, program, index as u32, &ids).ok()?;
    }

    // The module entry is compiled last: it is the program's entry point, and
    // its prefix may call any of the functions above.
    let plan = analysis.module.as_ref().filter(|module| module.hot);
    let module_entry = match plan {
        Some(plan) => {
            let entry = program.module_entry.as_ref()?;
            let id = module
                .declare_function("nect_module", Linkage::Local, &module_signature(&module))
                .ok()?;
            match compile_body(
                &mut module,
                &entry.function,
                id,
                &ids,
                Mode::Module {
                    resume: plan.resume as usize,
                },
            ) {
                Ok(()) => Some((id, entry.function.num_slots, plan.resume)),
                Err(_) => None,
            }
        }
        None => None,
    };

    let after_codegen = clock.elapsed();
    module.finalize_definitions().ok()?;
    let after_finalize = clock.elapsed();
    if std::env::var_os("NECT_JIT_TIMING").is_some() {
        let compiled = analysis.functions.iter().filter(|info| info.hot).count();
        eprintln!(
            "jit build: {compiled} function(s): builder {:?} codegen {:?} finalize {:?} total {:?}",
            after_builder,
            after_codegen - after_module,
            after_finalize - after_codegen,
            clock.elapsed()
        );
    }

    let mut functions: Vec<Option<JitFunction>> = vec![None; program.functions.len()];
    for (index, info) in analysis.functions.iter().enumerate() {
        if !info.hot {
            continue;
        }
        if let Some(id) = ids[index] {
            functions[index] = Some(JitFunction {
                code: module.get_finalized_function(id),
                arity: program.functions[index].param_count,
            });
        }
    }

    let module_code = module_entry.map(|(id, slots, resume)| JitModuleEntry {
        code: module.get_finalized_function(id),
        slots,
        resume,
    });

    Some(Jit {
        _module: module,
        depth: Box::new(0),
        functions,
        module: module_code,
        analysis,
    })
}

/// Native signature: `(depth pointer, args...) -> f64`.
fn signature(module: &JITModule, function: &CompiledFunction) -> cranelift_codegen::ir::Signature {
    let mut signature = module.make_signature();
    signature
        .params
        .push(AbiParam::new(module.target_config().pointer_type()));
    for _ in 0..function.param_count {
        signature.params.push(AbiParam::new(types::F64));
    }
    signature.returns.push(AbiParam::new(types::F64));
    signature
}

/// Native signature of the module entry: `(depth, out) -> f64`.
fn module_signature(module: &JITModule) -> cranelift_codegen::ir::Signature {
    let mut signature = module.make_signature();
    signature
        .params
        .push(AbiParam::new(module.target_config().pointer_type()));
    signature
        .params
        .push(AbiParam::new(module.target_config().pointer_type()));
    signature.returns.push(AbiParam::new(types::F64));
    signature
}

/// What kind of body is being compiled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// A user function: parameters arrive as `f64` and the body returns a value.
    Function,
    /// A module-level prefix: no parameters, slots are mirrored globals, and
    /// reaching `resume` (by branch, or by falling off the end) writes the slots
    /// back and hands control to the bytecode VM.
    Module { resume: usize },
}

fn compile_function(
    module: &mut JITModule,
    program: &Program,
    index: u32,
    ids: &[Option<FuncId>],
) -> Result<(), String> {
    let function = &program.functions[index as usize];
    let func_id = ids[index as usize].ok_or("function was not declared")?;
    compile_body(module, function, func_id, ids, Mode::Function)
}

/// Emits machine code for one body. `ids` resolves native calls; a module entry
/// never needs it, being the program's entry point rather than a callable.
fn compile_body(
    module: &mut JITModule,
    function: &CompiledFunction,
    func_id: FuncId,
    ids: &[Option<FuncId>],
    mode: Mode,
) -> Result<(), String> {
    let instructions = &function.instructions;
    let limit = match mode {
        Mode::Function => instructions.len(),
        Mode::Module { resume } => resume.min(instructions.len()),
    };
    let reachable =
        reachable_instructions(instructions, limit).map_err(|reason| reason.to_string())?;
    let pointer_type = module.target_config().pointer_type();
    let flags = MemFlagsData::new();

    let mut context = module.make_context();
    context.func.signature = match mode {
        Mode::Function => signature(module, function),
        Mode::Module { .. } => module_signature(module),
    };
    let mut builder_context = FunctionBuilderContext::new();
    {
        let mut builder = FunctionBuilder::new(&mut context.func, &mut builder_context);
        let entry = builder.create_block();
        builder.append_block_params_for_function_params(entry);
        builder.switch_to_block(entry);

        let entry_params: Vec<ClValue> = builder.block_params(entry).to_vec();
        let depth = entry_params[0];
        // The module entry receives the buffer its mirrored globals are written
        // back through; a function's remaining parameters are its arguments.
        let out_ptr = entry_params.get(1).copied();
        let args: &[ClValue] = match mode {
            Mode::Function => &entry_params[1..],
            Mode::Module { .. } => &[],
        };

        // Locals are SSA variables. They start at 0.0 so every path has a
        // definition; the type pass only lets through reads of written slots.
        let locals: Vec<Variable> = (0..function.num_slots)
            .map(|_| builder.declare_var(types::F64))
            .collect();
        let zero = builder.ins().f64const(0.0);
        for local in &locals {
            builder.def_var(*local, zero);
        }
        for (slot, arg) in args.iter().enumerate() {
            if let Some(local) = locals.get(slot) {
                builder.def_var(*local, *arg);
            }
        }

        // Depth guard: increment, and bail out when the native chain is too
        // deep for the host stack.
        let counter = builder.ins().load(pointer_type, flags, depth, 0);
        let incremented = builder.ins().iadd_imm_u(counter, 1);
        builder.ins().store(flags, incremented, depth, 0);
        let bail = builder.create_block();
        let body = builder.create_block();
        let too_deep =
            builder
                .ins()
                .icmp_imm_s(IntCC::SignedGreaterThan, incremented, MAX_NATIVE_DEPTH);
        builder.ins().brif(too_deep, bail, &[], body, &[]);
        builder.switch_to_block(body);

        // One block per reachable jump target, so control flow can branch. The
        // module entry's exit is a block of its own: branching to it leaves
        // native code.
        let exit = builder.create_block();
        let mut blocks: Vec<Option<Block>> = vec![None; instructions.len()];
        for (position, op) in instructions.iter().enumerate() {
            if !reachable[position] {
                continue;
            }
            if let Some(target) = op.jump_target() {
                let target = target as usize;
                if matches!(mode, Mode::Module { resume } if target == resume) {
                    continue;
                }
                continue_if_out_of_range(op.jump_target().unwrap(), instructions, &reachable)?;
                if target != position && blocks[target].is_none() {
                    blocks[target] = Some(builder.create_block());
                }
            }
        }
        blocks[0] = Some(body);
        if let Mode::Module { resume } = mode {
            blocks[resume] = Some(exit);
        }

        let mut terminated: HashSet<Block> = HashSet::new();
        let mut current = body;
        let mut filled = false;
        let mut stack: Vec<ClValue> = Vec::new();

        for position in 0..limit {
            if !reachable[position] {
                continue;
            }
            let target_block = blocks[position];
            if let Some(block) = target_block
                && block != current
            {
                if !filled {
                    builder.ins().jump(block, &[]);
                    terminated.insert(current);
                }
                builder.switch_to_block(block);
                current = block;
                filled = false;
            } else if filled {
                let block = builder.create_block();
                builder.switch_to_block(block);
                current = block;
                filled = false;
            }

            match &instructions[position] {
                Op::LoadConst(constant) => {
                    let value = match &function.constants[*constant as usize] {
                        Value::Number(n) => *n,
                        Value::Boolean(b) => f64::from(*b),
                        _ => unreachable!("non-numeric constant passed the type pass"),
                    };
                    let value = builder.ins().f64const(value);
                    stack.push(value);
                }
                Op::LoadLocal(slot) => {
                    let value = builder.use_var(locals[*slot as usize]);
                    stack.push(value);
                }
                Op::StoreLocal(slot) => {
                    let value = stack.pop().ok_or("operand stack underflow")?;
                    builder.def_var(locals[*slot as usize], value);
                }
                Op::StoreKeep(target) => {
                    let value = *stack.last().ok_or("operand stack underflow")?;
                    builder.def_var(locals[target.index() as usize], value);
                }
                Op::UnaryOp(op) => {
                    let value = stack.pop().ok_or("operand stack underflow")?;
                    let result = match op {
                        UnaryOp::Negate => builder.ins().fneg(value),
                        UnaryOp::Not => {
                            let falsy = truthy(&mut builder, value);
                            boolean_value(&mut builder, falsy)
                        }
                    };
                    stack.push(result);
                }
                Op::BinaryOp(op) => {
                    let right = stack.pop().ok_or("operand stack underflow")?;
                    let left = stack.pop().ok_or("operand stack underflow")?;
                    guarded_division(
                        &mut builder,
                        &mut current,
                        &mut filled,
                        &mut terminated,
                        *op,
                        right,
                        bail,
                    );
                    let result = binary(&mut builder, *op, left, right)?;
                    stack.push(result);
                }
                Op::BinaryFast { op, lhs, rhs } => {
                    let left = read_operand(&mut builder, function, &locals, *lhs)?;
                    let right = read_operand(&mut builder, function, &locals, *rhs)?;
                    guarded_division(
                        &mut builder,
                        &mut current,
                        &mut filled,
                        &mut terminated,
                        *op,
                        right,
                        bail,
                    );
                    let result = binary(&mut builder, *op, left, right)?;
                    stack.push(result);
                }
                Op::BinaryStore { op, dst, lhs, rhs } => {
                    let left = read_operand(&mut builder, function, &locals, *lhs)?;
                    let right = read_operand(&mut builder, function, &locals, *rhs)?;
                    guarded_division(
                        &mut builder,
                        &mut current,
                        &mut filled,
                        &mut terminated,
                        *op,
                        right,
                        bail,
                    );
                    let result = binary(&mut builder, *op, left, right)?;
                    builder.def_var(locals[dst.index() as usize], result);
                }
                Op::JumpIfNot {
                    op,
                    lhs,
                    rhs,
                    target,
                } => {
                    let left = read_operand(&mut builder, function, &locals, *lhs)?;
                    let right = read_operand(&mut builder, function, &locals, *rhs)?;
                    guarded_division(
                        &mut builder,
                        &mut current,
                        &mut filled,
                        &mut terminated,
                        *op,
                        right,
                        bail,
                    );
                    // A comparison feeds the branch directly: no need to
                    // materialise a boolean and test it again.
                    let truthy = match comparison_flag(&mut builder, *op, left, right) {
                        Some(flag) => flag,
                        None => {
                            let value = binary(&mut builder, *op, left, right)?;
                            truthy(&mut builder, value)
                        }
                    };
                    let fall = block_for(&mut builder, &mut blocks, position + 1)?;
                    let target_block = block_for(&mut builder, &mut blocks, *target as usize)?;
                    builder.ins().brif(truthy, fall, &[], target_block, &[]);
                    terminated.insert(current);
                    builder.switch_to_block(fall);
                    current = fall;
                    filled = false;
                }
                Op::JumpIfFalse(target) => {
                    let condition = stack.pop().ok_or("operand stack underflow")?;
                    let truthy = truthy(&mut builder, condition);
                    let fall = block_for(&mut builder, &mut blocks, position + 1)?;
                    let target_block = block_for(&mut builder, &mut blocks, *target as usize)?;
                    builder.ins().brif(truthy, fall, &[], target_block, &[]);
                    terminated.insert(current);
                    builder.switch_to_block(fall);
                    current = fall;
                    filled = false;
                }
                Op::Jump(target) => {
                    let target_block = block_for(&mut builder, &mut blocks, *target as usize)?;
                    builder.ins().jump(target_block, &[]);
                    terminated.insert(current);
                    filled = true;
                }
                Op::Call(CallTarget::Function(callee), arg_count) => {
                    let argument_count = *arg_count as usize;
                    if stack.len() < argument_count {
                        return Err("operand stack underflow".to_string());
                    }
                    let arguments: Vec<ClValue> = stack.split_off(stack.len() - argument_count);
                    let callee_id = ids
                        .get(*callee as usize)
                        .copied()
                        .flatten()
                        .ok_or("callee is not compiled")?;
                    let reference = module.declare_func_in_func(callee_id, builder.func);
                    let mut call_args = Vec::with_capacity(arguments.len() + 1);
                    call_args.push(depth);
                    call_args.extend(arguments);
                    let call = builder.ins().call(reference, &call_args);
                    let result = builder.inst_results(call)[0];
                    // Propagate a bail-out from a nested call unchanged.
                    let bits = builder.ins().bitcast(types::I64, flags, result);
                    let bailed = builder.ins().icmp_imm_u(IntCC::Equal, bits, BAIL as i64);
                    let cont = block_for(&mut builder, &mut blocks, position + 1)?;
                    builder.ins().brif(bailed, bail, &[], cont, &[]);
                    terminated.insert(current);
                    builder.switch_to_block(cont);
                    current = cont;
                    filled = false;
                    stack.push(result);
                }
                Op::Return => {
                    if mode != Mode::Function {
                        // `return` at module level is a runtime error, which
                        // inference rejects rather than compiles.
                        return Err("module-level return is not compilable".to_string());
                    }
                    let value = stack.pop().ok_or("operand stack underflow")?;
                    let counter = builder.ins().load(pointer_type, flags, depth, 0);
                    let decremented = builder.ins().iadd_imm_s(counter, -1);
                    builder.ins().store(flags, decremented, depth, 0);
                    builder.ins().return_(&[value]);
                    terminated.insert(current);
                    filled = true;
                }
                Op::Pop => {
                    stack.pop().ok_or("operand stack underflow")?;
                }
                other => return Err(format!("instruction {other:?} is not compilable")),
            }
        }

        // Any block that never got a terminator (dead ends, and the bail block)
        // is closed off; the bail path returns the sentinel so the VM can re-run
        // the call on the bytecode VM.
        if mode == Mode::Function {
            if !filled {
                // The current block still needs a terminator; only reachable
                // with hand-written bytecode, since the type pass requires a
                // return.
                let value = builder.ins().f64const(0.0);
                builder.ins().return_(&[value]);
                terminated.insert(current);
            }
            for block in blocks.iter().flatten() {
                if !terminated.contains(block) {
                    builder.switch_to_block(*block);
                    let value = builder.ins().f64const(0.0);
                    builder.ins().return_(&[value]);
                    terminated.insert(*block);
                }
            }
        } else {
            // A module prefix that runs off the end has reached the point where
            // the bytecode VM takes over, exactly like a branch to it.
            if !filled {
                builder.ins().jump(exit, &[]);
                terminated.insert(current);
            }
            // Blocks that were never reached are dead code; leaving native code
            // from them would add a predecessor to the exit block that never
            // defines the mirrored globals.
            for block in blocks.iter().flatten() {
                if !terminated.contains(block) && *block != exit {
                    builder.switch_to_block(*block);
                    let value = builder.ins().f64const(0.0);
                    builder.ins().return_(&[value]);
                    terminated.insert(*block);
                }
            }
            // The exit block writes the mirrored globals back for the VM and
            // leaves; the depth counter goes with it.
            let out_ptr = out_ptr.ok_or("module entry has no output pointer")?;
            builder.switch_to_block(exit);
            for (slot, local) in locals.iter().enumerate() {
                let value = builder.use_var(*local);
                builder.ins().store(
                    flags,
                    value,
                    out_ptr,
                    (slot * std::mem::size_of::<f64>()) as i32,
                );
            }
            let counter = builder.ins().load(pointer_type, flags, depth, 0);
            let decremented = builder.ins().iadd_imm_s(counter, -1);
            builder.ins().store(flags, decremented, depth, 0);
            let value = builder.ins().f64const(0.0);
            builder.ins().return_(&[value]);
            terminated.insert(exit);
        }
        builder.switch_to_block(bail);
        let counter = builder.ins().load(pointer_type, flags, depth, 0);
        let decremented = builder.ins().iadd_imm_s(counter, -1);
        builder.ins().store(flags, decremented, depth, 0);
        let sentinel = builder.ins().f64const(f64::from_bits(BAIL));
        builder.ins().return_(&[sentinel]);

        builder.seal_all_blocks();
        builder.finalize(module.target_config());
    }

    module
        .define_function(func_id, &mut context)
        .map_err(|error| error.to_string())?;
    module.clear_context(&mut context);
    Ok(())
}

fn continue_if_out_of_range(
    target: u32,
    instructions: &[Op],
    reachable: &[bool],
) -> Result<(), String> {
    if target as usize >= instructions.len() || !reachable[target as usize] {
        return Err(format!("jump target {target} is not reachable"));
    }
    Ok(())
}

/// Block for `position`, created on demand.
fn block_for(
    builder: &mut FunctionBuilder,
    blocks: &mut [Option<Block>],
    position: usize,
) -> Result<Block, String> {
    if position >= blocks.len() {
        return Err("branch past the end of the function".to_string());
    }
    if let Some(block) = blocks[position] {
        return Ok(block);
    }
    let block = builder.create_block();
    blocks[position] = Some(block);
    Ok(block)
}

fn read_operand(
    builder: &mut FunctionBuilder,
    function: &CompiledFunction,
    locals: &[Variable],
    operand: Fusee,
) -> Result<ClValue, String> {
    match operand.tag() {
        TAG_LOCAL => Ok(builder.use_var(locals[operand.index() as usize])),
        TAG_CONST => match &function.constants[operand.index() as usize] {
            Value::Number(n) => Ok(builder.ins().f64const(*n)),
            Value::Boolean(b) => Ok(builder.ins().f64const(f64::from(*b))),
            _ => Err("non-numeric constant passed the type pass".to_string()),
        },
        _ => Err("global operand passed the type pass".to_string()),
    }
}

/// Emits the zero-divisor guard for `/`.
///
/// A zero divisor is a runtime error, and native code cannot raise one without
/// unwinding, so the divisor is tested and the function bails out to the
/// bytecode VM, which re-runs the call and reports `division by zero` exactly as
/// it would have. The check costs one compare and a not-taken branch per
/// division, which is why it does not defeat the purpose of compiling the
/// function in the first place.
///
/// Control flow continues in a fresh block, so the caller's notion of "the block
/// being filled" is updated in place.
fn guarded_division(
    builder: &mut FunctionBuilder,
    current: &mut Block,
    filled: &mut bool,
    terminated: &mut HashSet<Block>,
    op: BinaryOp,
    divisor: ClValue,
    bail: Block,
) {
    if op != BinaryOp::Divide {
        return;
    }
    let zero = builder.ins().f64const(0.0);
    let is_zero = builder.ins().fcmp(FloatCC::Equal, divisor, zero);
    let ok = builder.create_block();
    builder.ins().brif(is_zero, bail, &[], ok, &[]);
    terminated.insert(*current);
    builder.switch_to_block(ok);
    *current = ok;
    *filled = false;
}

/// `left op right` for numeric operands, with booleans as `0.0`/`1.0`.
fn binary(
    builder: &mut FunctionBuilder,
    op: BinaryOp,
    left: ClValue,
    right: ClValue,
) -> Result<ClValue, String> {
    let value = match op {
        BinaryOp::Add => builder.ins().fadd(left, right),
        BinaryOp::Subtract => builder.ins().fsub(left, right),
        BinaryOp::Multiply => builder.ins().fmul(left, right),
        // Guarded by [`guarded_division`] before this point.
        BinaryOp::Divide => builder.ins().fdiv(left, right),
        BinaryOp::Less
        | BinaryOp::Greater
        | BinaryOp::LessEqual
        | BinaryOp::GreaterEqual
        | BinaryOp::Equal
        | BinaryOp::NotEqual => {
            let condition = builder.ins().fcmp(float_condition(op), left, right);
            boolean_value(builder, condition)
        }
        other => return Err(format!("operator {other:?} is not compilable")),
    };
    Ok(value)
}

fn float_condition(op: BinaryOp) -> FloatCC {
    match op {
        BinaryOp::Less => FloatCC::LessThan,
        BinaryOp::Greater => FloatCC::GreaterThan,
        BinaryOp::LessEqual => FloatCC::LessThanOrEqual,
        BinaryOp::GreaterEqual => FloatCC::GreaterThanOrEqual,
        BinaryOp::Equal => FloatCC::Equal,
        BinaryOp::NotEqual => FloatCC::NotEqual,
        _ => unreachable!("not an ordering operator"),
    }
}

/// Branch flag for an ordering comparison, or `None` when the operator is not
/// a comparison (then the caller materialises a value instead).
fn comparison_flag(
    builder: &mut FunctionBuilder,
    op: BinaryOp,
    left: ClValue,
    right: ClValue,
) -> Option<ClValue> {
    match op {
        BinaryOp::Less
        | BinaryOp::Greater
        | BinaryOp::LessEqual
        | BinaryOp::GreaterEqual
        | BinaryOp::Equal
        | BinaryOp::NotEqual => Some(builder.ins().fcmp(float_condition(op), left, right)),
        _ => None,
    }
}

/// Truthiness of a number, matching `is_truthy` in the VM (`!= 0.0`).
fn truthy(builder: &mut FunctionBuilder, value: ClValue) -> ClValue {
    let zero = builder.ins().f64const(0.0);
    builder.ins().fcmp(FloatCC::NotEqual, value, zero)
}

fn boolean_value(builder: &mut FunctionBuilder, condition: ClValue) -> ClValue {
    let one = builder.ins().f64const(1.0);
    let zero = builder.ins().f64const(0.0);
    builder.ins().select(condition, one, zero)
}
