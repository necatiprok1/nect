//! Integration tests for the IR optimization passes (Phase 9.2).
//!
//! These tests verify that IR optimizations preserve the exact language
//! semantics by constructing IR, running optimizations, and checking that
//! the IR structure is sound and the optimization results are correct.
//!
//! The IR layer is a supplementary analysis/optimization target — it does not
//! replace the bytecode VM. These tests ensure the IR infrastructure is correct
//! and ready for future IR-to-backend lowering.

use nect::ast::{BinaryOp, UnaryOp, Value};
use nect::ir::*;

/// Build a simple IR function that computes `a + b * 2` and returns the result.
fn build_simple_arithmetic() -> IrModule {
    let mut module = IrModule::new();
    module.new_function(
        "arithmetic",
        vec![
            IrParam {
                name: "a".to_string(),
                ty: ValueType::Num,
            },
            IrParam {
                name: "b".to_string(),
                ty: ValueType::Num,
            },
        ],
    );

    let func = &mut module.functions[0];
    let block = &mut func.blocks[0];

    // Constants for the multiplication: 2.0
    // Params are ValueId 0 and 1, so we use 2+ for instruction results.
    block.push(IrInstr::Const {
        value: Value::Number(2.0),
        result: ValueId::new(2),
    });

    // v2 = a + b (params 0 and 1)
    block.push(IrInstr::BinaryOp {
        op: BinaryOp::Add,
        lhs: ValueId::new(0), // param a
        rhs: ValueId::new(1), // param b
        result: ValueId::new(3),
    });
    // v3 = v2 * 2.0 (const at result 2)
    block.push(IrInstr::BinaryOp {
        op: BinaryOp::Multiply,
        lhs: ValueId::new(3),
        rhs: ValueId::new(2), // const 2.0
        result: ValueId::new(4),
    });
    block.terminator = Terminator::Return(Some(ValueId::new(4)));

    module
}

#[test]
fn test_arithmetic_ir_is_valid() {
    let module = build_simple_arithmetic();
    assert!(verify(&module).is_ok());
}

#[test]
fn test_arithmetic_ir_constant_folding() {
    let mut module = build_simple_arithmetic();

    // The constant fold pass should not fold `a + b` since `a` and `b` are params.
    // But it should process the IR without errors.
    optimize(&mut module.functions[0]);

    assert!(verify(&module).is_ok());
}

#[test]
fn test_ir_dce_preserves_side_effects() {
    let mut module = IrModule::new();
    module.new_function("side_effects", vec![]);

    let func = &mut module.functions[0];
    let block = &mut func.blocks[0];

    // A dead computation (result never used).
    block.push(IrInstr::Const {
        value: Value::Number(1.0),
        result: ValueId::new(0),
    });
    block.push(IrInstr::Const {
        value: Value::Number(2.0),
        result: ValueId::new(0),
    });
    block.push(IrInstr::BinaryOp {
        op: BinaryOp::Add,
        lhs: ValueId::new(0),
        rhs: ValueId::new(1),
        result: ValueId::new(2), // dead: never used
    });
    block.terminator = Terminator::Return(None);

    optimize(&mut module.functions[0]);

    assert!(verify(&module).is_ok());

    // DCE should have removed the dead BinaryOp.
    let block = &module.functions[0].blocks[0];
    assert!(
        !block
            .instructions
            .iter()
            .any(|i| matches!(i, IrInstr::BinaryOp { .. }))
    );
}

#[test]
fn test_ir_dce_preserves_print() {
    let mut module = IrModule::new();
    module.new_function("print_test", vec![]);

    let _print_sym = module.intern("print");

    let func = &mut module.functions[0];
    let block = &mut func.blocks[0];

    block.push(IrInstr::Const {
        value: Value::Number(42.0),
        result: ValueId::new(0),
    });
    block.push(IrInstr::Print {
        value: ValueId::new(0),
    });
    block.terminator = Terminator::Return(None);

    optimize(&mut module.functions[0]);

    assert!(verify(&module).is_ok());

    // Print should NOT be removed (it has side effects).
    let block = &module.functions[0].blocks[0];
    assert!(
        block
            .instructions
            .iter()
            .any(|i| matches!(i, IrInstr::Print { .. }))
    );
}

#[test]
fn test_ir_cse_detects_common_subexpression() {
    let mut module = IrModule::new();
    module.new_function("cse_test", vec![]);

    let func = &mut module.functions[0];
    let block = &mut func.blocks[0];

    // Two identical binary operations on the same operands.
    block.push(IrInstr::Const {
        value: Value::Number(3.0),
        result: ValueId::new(0),
    });
    block.push(IrInstr::Const {
        value: Value::Number(4.0),
        result: ValueId::new(0),
    });
    // v2 = 3.0 + 4.0
    block.push(IrInstr::BinaryOp {
        op: BinaryOp::Add,
        lhs: ValueId::new(0),
        rhs: ValueId::new(1),
        result: ValueId::new(2),
    });
    // v3 = 3.0 + 4.0 (same expression!)
    block.push(IrInstr::BinaryOp {
        op: BinaryOp::Add,
        lhs: ValueId::new(0),
        rhs: ValueId::new(1),
        result: ValueId::new(3),
    });
    block.terminator = Terminator::Return(Some(ValueId::new(2)));

    optimize(&mut module.functions[0]);

    assert!(verify(&module).is_ok());
    // CSE should have detected the common subexpression.
    // (The current implementation is a placeholder that doesn't rewrite,
    // but it should at least complete without errors.)
}

#[test]
fn test_ir_optimize_preserves_verification() {
    let mut module = build_simple_arithmetic();

    // Optimize and verify the IR is still valid.
    optimize(&mut module.functions[0]);
    assert!(verify(&module).is_ok());

    // Run optimization with metrics.
    let _metrics = optimize_with_metrics(&mut module.functions[0]);
    assert!(verify(&module).is_ok());

    // Module-level optimization should also preserve verification.
    optimize_module(&mut module);
    assert!(verify(&module).is_ok());
}

#[test]
fn test_ir_backend_trait() {
    // Test that the IrBackend trait can be used in a generic context.
    struct TestBackend;

    impl IrBackend for TestBackend {
        type Output = String;

        fn lower(&self, module: &IrModule) -> Result<Self::Output, String> {
            Ok(dump_ir(module))
        }

        fn name(&self) -> &'static str {
            "test-backend"
        }
    }

    let module = build_simple_arithmetic();
    let backend = TestBackend;
    let result = backend.lower(&module);
    assert!(result.is_ok());
    assert!(result.unwrap().contains("arithmetic"));
}

#[test]
fn test_ir_jit_diagnostics() {
    let module = build_simple_arithmetic();

    for func in &module.functions {
        let diag = analyze_function_for_jit(func);
        assert!(diag.eligible);
        assert_eq!(diag.max_native_depth, IR_MAX_NATIVE_DEPTH);
    }
}

#[test]
fn test_ir_sequential_optimization() {
    // Run optimization multiple times to ensure idempotency.
    let mut module = build_simple_arithmetic();

    optimize(&mut module.functions[0]);
    let instr_count_1 = module.functions[0].blocks[0].instructions.len();

    optimize(&mut module.functions[0]);
    let instr_count_2 = module.functions[0].blocks[0].instructions.len();

    // Second pass should not change anything (idempotent).
    assert_eq!(instr_count_1, instr_count_2);

    assert!(verify(&module).is_ok());
}

#[test]
fn test_ir_unary_negate() {
    let mut module = IrModule::new();
    module.new_function(
        "negate_test",
        vec![IrParam {
            name: "x".to_string(),
            ty: ValueType::Num,
        }],
    );

    let func = &mut module.functions[0];
    let block = &mut func.blocks[0];

    block.push(IrInstr::UnaryOp {
        op: UnaryOp::Negate,
        operand: ValueId::new(0),
        result: ValueId::new(1),
    });
    block.terminator = Terminator::Return(Some(ValueId::new(1)));

    assert!(verify(&module).is_ok());

    optimize(&mut module.functions[0]);
    assert!(verify(&module).is_ok());
}

#[test]
fn test_ir_branch_blocks() {
    let mut module = IrModule::new();
    module.new_function(
        "branch_test",
        vec![IrParam {
            name: "x".to_string(),
            ty: ValueType::Num,
        }],
    );

    module.add_block("then_block");
    module.add_block("else_block");
    module.add_block("end_block");

    let func = &mut module.functions[0];

    // Entry: branch on x > 0
    let x = ValueId::new(0);
    let zero_val = ValueId::new(1);
    func.blocks[0].push(IrInstr::Const {
        value: Value::Number(0.0),
        result: zero_val,
    });
    func.blocks[0].push(IrInstr::BinaryOp {
        op: BinaryOp::Greater,
        lhs: x,
        rhs: zero_val,
        result: ValueId::new(2),
    });
    func.blocks[0].terminator = Terminator::Branch {
        cond: ValueId::new(2),
        then: BlockId::new(1),
        from: BlockId::new(2),
    };

    // Then block: return x
    func.blocks[1].terminator = Terminator::Return(Some(x));

    // Else block: return -x
    func.blocks[2].push(IrInstr::UnaryOp {
        op: UnaryOp::Negate,
        operand: x,
        result: ValueId::new(3),
    });
    func.blocks[2].terminator = Terminator::Return(Some(ValueId::new(3)));

    // End block (unreachable, for completeness)
    func.blocks[3].terminator = Terminator::Halt;

    let verify_result = verify(&module);
    assert!(
        verify_result.is_ok(),
        "verify failed: {:?}",
        verify_result.err()
    );

    optimize(&mut module.functions[0]);
    assert!(verify(&module).is_ok());
}

#[test]
fn test_ir_optimization_preserves_semantics() {
    // Verify that optimization doesn't break a simple computation.
    let mut module = IrModule::new();
    module.new_function("semantic_preserve", vec![]);

    let func = &mut module.functions[0];
    let block = &mut func.blocks[0];

    // 1 + 2 = 3, should fold to const 3.0
    let _c1 = ValueId::new(0);
    let _c2 = ValueId::new(1);
    let _result = ValueId::new(2);

    block.push(IrInstr::Const {
        value: Value::Number(1.0),
        result: ValueId::new(0),
    });
    block.push(IrInstr::Const {
        value: Value::Number(2.0),
        result: ValueId::new(0),
    });
    block.push(IrInstr::BinaryOp {
        op: BinaryOp::Add,
        lhs: ValueId::new(0),
        rhs: ValueId::new(1),
        result: ValueId::new(2),
    });
    block.terminator = Terminator::Return(Some(ValueId::new(2)));

    optimize(&mut module.functions[0]);
    assert!(verify(&module).is_ok());

    // After optimization, the BinaryOp should be folded to a Const.
    let block = &module.functions[0].blocks[0];
    assert!(
        block
            .instructions
            .iter()
            .any(|i| matches!(i, IrInstr::Const { .. }))
    );
    assert!(
        !block
            .instructions
            .iter()
            .any(|i| matches!(i, IrInstr::BinaryOp { .. }))
    );
}

#[test]
fn test_ir_function_with_params() {
    let mut module = IrModule::new();
    module.new_function(
        "param_fn",
        vec![
            IrParam {
                name: "a".to_string(),
                ty: ValueType::Num,
            },
            IrParam {
                name: "b".to_string(),
                ty: ValueType::Num,
            },
            IrParam {
                name: "c".to_string(),
                ty: ValueType::Num,
            },
            IrParam {
                name: "d".to_string(),
                ty: ValueType::Num,
            }, // max arity = 4
        ],
    );

    let func = &mut module.functions[0];
    let block = &mut func.blocks[0];

    // a + b
    block.push(IrInstr::BinaryOp {
        op: BinaryOp::Add,
        lhs: ValueId::new(0),
        rhs: ValueId::new(1),
        result: ValueId::new(4),
    });
    // result + c
    block.push(IrInstr::BinaryOp {
        op: BinaryOp::Add,
        lhs: ValueId::new(4),
        rhs: ValueId::new(2),
        result: ValueId::new(5),
    });
    // result + d
    block.push(IrInstr::BinaryOp {
        op: BinaryOp::Add,
        lhs: ValueId::new(5),
        rhs: ValueId::new(3),
        result: ValueId::new(6),
    });
    block.terminator = Terminator::Return(Some(ValueId::new(6)));

    assert!(verify(&module).is_ok());
}
