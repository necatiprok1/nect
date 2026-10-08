PHASE 1
~~✓ Lexer~~
~~✓ Parser~~
~~✓ AST~~
~~✓ Type System~~
~~✓ Compiler~~
~~✓ Runtime~~
~~✓ .nct~~
~~✓ CLI~~

PHASE 2
~~✓ Standard Library~~
~~✓ Error system~~
~~✓ Testing~~
~~✓ Benchmark suite~~
~~✓ Documentation~~

PHASE 3
~~✓ Package Manager~~
~~✓ Package Registry~~
~~✓ Dependency system~~

PHASE 4
~~✓ LSP~~
~~✓ VS Code extension~~
~~✓ Formatter~~
~~✓ Linter~~
~~✓ Debugger~~

PHASE 5
~~✓ Native compilation~~
~~✓ Cross compilation~~
~~✓ Optimizer~~
~~✓ Parallelism~~
~~✓ Async runtime~~

PHASE 6
~~✓ Web framework (HTTP server/client, routing, middleware)~~
~~✓ AI/ML libraries (tensor ops, neural nets, common algorithms)~~
~~✓ Database libraries (SQLite bindings, query builder)~~
~~✓ GUI framework (native widgets, event loop, layout)~~

PHASE 7
~~✓ Stable 1.0~~
  - ~~✓ Compiler: crash-free, clear errors, stable type system, proper memory management, stable runtime, cross-platform build~~
  - ~~✓ Standard Library: io, fs, net, json, math, collections, time~~
  - ~~✓ Tooling: nect build, nect run, nect test, nect fmt, nect lint, nect debug~~
  - ~~✓ Package system: nect add, nect remove, nect install, dependency resolution~~
~~✓ Documentation website~~
  - Site in docs/site/ (index.html, tutorial.html, cookbook.html, reference.html, style.css, build.py)
~~✓ GitHub releases~~
~~✓ Automated CI/CD~~
   - GitHub Actions: build, test, clippy, fmt check on Linux + macOS

PHASE 10
~~✓ Runtime Memory Model~~
- ~~✓ Document value representation (Rc<RefCell<T>>, primitives)~~
- ~~✓ Document stack vs heap, ownership, temporary lifetimes~~
- ~~✓ Document out-of-memory behavior~~
- ~~✓ docs/memory.md~~

~~✓ Allocation Optimization~~
- ~~✓ Audit allocations in hot runtime paths (builtins.rs, vm/mod.rs, interpreter)~~
- ~~✓ Remove avoidable allocations: string indexing, print, concat, join, array/map formatting~~
- ~~✓ Allocation-aware benchmarks (benches/mem/*.nct)~~
- ~~✓ Memory regression tests (tests/mem_tests.rs, 22 tests)~~

~~✓ Runtime Diagnostics~~
- ~~✓ nect mem-profile command~~
- ~~✓ Runtime statistics (instruction count, call counts, peak stack depth)~~
- ~~✓ --stack-trace flag and NECT_STACK_TRACE=1 for crash diagnostics~~
- ~~✓ NECT_VM_STATS=1 environment variable~~

PHASE 11
~~✓ C FFI~~
- ~~✓ extern declaration syntax: `extern "library" { fn name(..) -> type }`~~
- ~~✓ libloading integration for shared library loading~~
- ~~✓ Type marshaling: number↔f64, string↔*const c_char, bool↔bool, void↔()~~
- ~~✓ FFI tests (16 integration tests in tests/ffi_tests.rs)~~
- ~~✓ VM and interpreter parity for FFI~~
- ~~✓ Error handling: missing library, missing symbol, wrong args, wrong type~~