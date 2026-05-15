# Agent Instructions

Read and follow `PLAN.md` before implementation, review, or planning work in
this repository.

## User priorities

The user values these points above everything else:

- The project ends when `tsdown`-bundleable TypeScript/JavaScript Node.js code
  can compile to standalone Go and match Node.js LTS observable behavior.
- This is not a Fastify, Hono, Express, NestJS, Vite, or framework-specific
  converter. Frameworks and packages are validation corpus only.
- `go build` passing is not enough. Generated Go must match Node behavior:
  stdout, stderr, exit code, JSON/library result, env, argv, cwd, filesystem
  side effects, async order, and observed error shape.
- Never hardcode just to make tests or compilation pass. No corpus-name,
  package-name, fixture-name, path, test-id, known probe, or precomputed-answer
  branches in compiler, runtime, or codegen.
- Corpus tests are evidence for generic JS/Node semantics, not templates for
  special-case code generation.
- Unsupported semantics must fail closed with deterministic diagnostics before
  codegen. Silent wrong Go output is worse than a clear failure.
- Generated Go must not embed, shell out to, or depend on Node.js, V8,
  Node-API, N-API, native addons, or package-specific helper binaries.
- Rust owns parse/analyze/module resolution/executable IR/semantic
  lowering/codegen/diagnostics/fail-closed policy. TypeScript/JavaScript owns
  CLI/config/tsdown orchestration/corpus gates/UX. Go owns generated target and
  runtime helper only.
- Backend provider architecture matters. Go must be plugged in through the
  backend provider interface/adapter pattern, not wired as compiler shape.
- IR and runtime contracts must stay backend-neutral. Go emitter must not own JS
  or Node semantics policy.
- Commit by functional axis. Do not split into tiny test-case commits, and do
  not make one commit per corpus case unless required for reviewability.

`PLAN.md` is the source of truth for:

- TS -> Go full parity goal.
- Framework-agnostic compiler/runtime policy.
- Real Node corpus parity gate.
- Functional-axis commit policy.
- 100-point acceptance criteria.

Do not duplicate or reinterpret the plan here. If the parity target, corpus, or
commit policy changes, update `PLAN.md` first and keep this file as forwarding
only.
