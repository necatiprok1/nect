# Security model

What Nect protects, what it deliberately does not, and where the boundaries are.
This document describes the implementation as it is; where something is not
protected, that is stated rather than left implied.

## The short version

Nect is a general-purpose language with a general-purpose standard library. A
Nect program that calls `read_file`, `write_file`, `http_get`, `open_url`, or
`exec`-shaped built-ins can do what those APIs allow, with the same privileges as
the user who ran it. **Nect does not sandbox programs.** There is no capability
system, no permission prompt, and no filesystem or network restriction at
runtime.

That is a design position, not an oversight. Nect is meant to be a small,
fast, understandable language; a sandbox that appears to constrain a program but
can be escaped is worse than an honest "this runs with your privileges". If you
need isolation, run the program in a container, a VM, or under a restricted user
— and treat the program as untrusted code, which it is.

The rest of this document covers the places where a *smaller* guarantee does
hold, and the checks that are actually implemented.

## Package supply chain

### Integrity is verified

Every download is checked against a SHA-256 checksum before it is unpacked, and
the cache is verified on every access. A corrupted or tampered archive is
reported, not retried into existence: installing it anyway would defeat the
purpose of checking.

A checksum proves the bytes match what the registry said they should be. It does
not prove those bytes are benign. Integrity is not authorship — see signing
below.

### Nothing runs implicitly

`nect pkg install` downloads, verifies, and unpacks. It does **not** execute
anything from the package. Only an explicit `nect pkg run <name>` runs a script,
and the exact text is written to stderr before the shell sees it.

This is the single most important property of the package system, and it is
tested: installing an untrusted package cannot execute code on this machine.

The consequence is that `nect pkg run <name>` executes repository-controlled text
with your privileges. The command is echoed first so it is visible in a log, but
it is still your decision to invoke it. Read the script first if you did not
write the package.

### Auditing

`nect pkg audit` checks a resolved dependency set against a vulnerability
database. It is a check on *known* advisories, so a clean audit means "nothing
known", not "nothing wrong".

### What is not implemented

- **Signing.** There is no signature on a package, a tarball, or a manifest. A
  checksum in a lockfile proves the archive has not changed since the lockfile
  was written; it does not prove who produced it, and a lockfile from an
  untrusted source can pin a checksum for a malicious archive. Signing needs
  server-side key management, so it is not implemented rather than implemented
  weakly.
- **Ownership transfer.** A package's ownership is not modelled, so there is no
  "this name was transferred, the new owner is different" signal.

## Filesystem

`read_file` and `write_file` take whatever path the program gives them, resolved
against the working directory for `nect run -`. There is no allow-list, no jail,
and no path canonicalisation that would stop `../../etc/passwd`.

Symlinks are followed. A program that reads a path you did not intend it to read
will succeed.

## Network

`http_get`, `http_post`, `http_request`, `http_listen`, and the rest of the HTTP
group open sockets with no restriction on host, port, or address family. A
program can reach anything the machine can reach, including services bound to
localhost that the user can reach but did not intend to expose.

There is no TLS. `https://` URLs are not supported by the client built-ins, and
nothing verifies a server certificate. **Do not send a secret over these
built-ins.** If you need transport security, put it in front of the process —
a reverse proxy, or a tunnel.

`open_url` hands a URL to the platform's launcher (`open`, `xdg-open`, `cmd`).
The launcher decides what happens next, so a hostile URL may open an application
of the system's choosing. It spawns detached and returns immediately.

## Foreign function interface

FFI is the sharpest edge in the language, and it is deliberately blunt.

- An `extern` block loads a shared library by path and calls symbols in it with
  a declared signature. **A wrong declaration is undefined behaviour**, not an
  error: the C function will be called with the wrong argument types or count and
  the process may corrupt memory. Nect checks the *Nect-side* types and arity; it
  cannot check the C function's actual signature.
- The signature must be declared truthfully. `extern "m" { fn sin(number) -> number; }`
  works because that is `libm`'s signature.
- At most four arguments, and only `number`, `string`, `bool`, and `void`. Maps
  and arrays are rejected by the JIT and the C backend, so a program using `extern`
  runs on the VM.
- The library handle is leaked deliberately, so a loaded library stays mapped for
  the process lifetime. That is required for the function pointers to remain
  callable, and it means a library cannot be unloaded.

Treat an `extern` block in code you did not write as arbitrary native code
execution.

## Native compilation

The JIT compiles a function to machine code with Cranelift, and the C backend
compiles generated C. Neither is reachable from untrusted input in a way that
produces a wrong answer: the JIT compiles only functions it has *proved* are
numeric and side-effect-free, and anything it cannot prove stays on the bytecode
VM. A bailout in native code re-runs on the VM rather than continuing.

The relevant guarantee is behavioural, and it is enforced rather than asserted:
`tests/differential_tests.rs` runs every program in the suite — plus generated
ones — through the interpreter, the VM, and the VM with native compilation, and
requires identical output. A native-compilation bug fails the test suite, not a
user's program.

## Resource limits

There are no resource limits. A program can allocate until the process is killed
by the operating system, recurse until the stack guard fires, or loop forever.
`range()` and `repeat()` cap their element count at 10 million so a single
accidental call cannot request an absurd allocation, but that is a guard rail,
not a limit.

Deep recursion is a specific case: the interpreter recurses on the host stack and
will overflow it, while the VM recurses on the heap and will instead hit its own
guard. That difference is documented in `docs/reference.md` §13 and pinned by a
test.

## Untrusted input

There is no `eval`, and the language has no reflection, so a program cannot be
made to execute input as code. The nearest equivalents are:

- `import "path"` splices a file's *source* into the program. An imported file is
  code, not data. A module that arrives from an untrusted source is arbitrary
  code, exactly as an `extern` block is.
- `json_decode` parses data. It does not evaluate anything.

The web helpers are the parts most likely to touch untrusted input, and they are
written for it:

- `http_match_route` percent-decodes captured parameters and **rejects a
  malformed escape** rather than treating `%zz` as literal text. A path that
  smuggles a raw `%` past a check that only inspects decoded values is the
  failure this prevents.
- `http_cookie` percent-encodes the cookie value, so a token containing `;`
  cannot terminate the attribute list and introduce an attribute of its own.
- `http_validate` collects every problem in a body rather than the first, and
  treats an explicit `null` as absent, so a client is not told a field is
  present when it has no value.
- Unknown cookie attribute names are an error. A typo in `httpOnly` would
  otherwise leave a session cookie readable from JavaScript.

## What the runtime refuses

These are enforced in code and covered by tests:

- A call to a built-in name always resolves to the built-in, whatever the
  program also declares. `fn len(x) { return 99 }` is accepted and then has no
  effect: `len("abcd")` is `4`, identically on both engines. So a built-in's
  meaning cannot be changed by a declaration, and a file cannot mean something
  different from an identical-looking one. The cost is that such a declaration is
  silently inert rather than an error, which is why `nect lint` reports it
  (`shadowed_builtin`). Reserved words `and`, `or`, and `not` are operators and
  cannot be used as names at all.
- An FFI declaration with the wrong arity or a type the runtime cannot marshal
  is an error before the call is attempted, not a crash.
- A missing shared library or symbol fails at startup with a message naming what
  was missing, rather than at the call site.
- `nect build` never runs a partially-valid translation. A program outside the
  translatable subset is rejected with a reason, and the program stays on the VM.

## Debugger

`nect debug` and `nect dap` execute the program. A breakpoint is not a
security boundary, and neither front end adds one.

The DAP server redirects the debuggee's output to stderr because stdout carries
the protocol — a `print` landing in the message stream would desynchronise every
frame after it. That is a protocol-correctness measure, not a secrecy measure:
program output still goes to the console in plain text.

## Reporting a vulnerability

Report it privately rather than in a public issue, with the program that
triggers it and what you expected instead. The most useful reports name which
component is involved: `nect run`, `nect build`, the JIT, the FFI layer, or the
package manager. `nect disasm` output identifies whether a problem is in the VM,
the JIT, or the C backend.
