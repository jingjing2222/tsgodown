# tsgodown

## Primary goal (direction lock)

`tsgodown` is a **compiler-mode pipeline** for TypeScript services.

The primary goal is fixed as:

1. **tsdown bundling as compiler input**
   - input artifact is the tsdown bundle plus `d.ts` and sourcemap metadata
2. **AST + sourcemap + `d.ts`-driven IR/Go generation**
   - analysis and lowering are driven by syntax + symbol/type surface + source mapping provenance
3. **`go build` output**
   - generated output must compile as a normal Go project/binary using the Go toolchain
4. **100% behavioral coverage (declared semantic envelope)**
   - defined as: full behavioral match within the declared semantic envelope, proven by semantics-parity testing (TS runtime vs Go runtime), not by claiming universal JS/TS coverage

This repository remains intentionally strict: when code is outside the declared semantic envelope or cannot be extracted deterministically, the compiler must emit explicit diagnostics and fail closed instead of silently guessing.

Core execution path guardrail: framework-name branching/adapters are disallowed in `packages/core/src`, `packages/pipeline/src`, and `packages/cli/src/commands` (check with `pnpm run guard:core-path`).

Canonical compiler input contract (framework-agnostic): `tsdown` bundled JS + sourcemap + `d.ts` are the source of truth for analysis and Go emission. Framework fixtures are validation samples, not compiler-mode scope boundaries.

## Milestone lock (execution sequence)

Documentation and gate execution follow roadmap issue `#117` as source of truth.
Current fixed sequence:

`M0 -> M1 -> M2 -> M3 -> M4 -> M5`

- `M0`: roadmap/docs/gates source-of-truth alignment
- `M1`: canonical compile-success gate (`pnpm run gate:m1`)
- `M2`: generated runtime reachability acceptance
- `M3`: deterministic runtime behavior/perf guard extensions
- `M4`: coverage ratchet + capability-based expansion controls
- `M5`: production readiness and release discipline

The sequence is locked for planning/reporting consistency and should be used in issue/PR text, roadmap updates, and release evidence.
Roadmap drift reconciliation log: `docs/operations/ROADMAP_DRIFT_RECONCILIATION.md`.

## JS -> Go syntax coverage roadmap (non-backend-first)

This project is not limited to backend/router translation.
The roadmap target is broad JavaScript/TypeScript semantics coverage to Go, with explicit host-boundary exceptions.

Normative checklist source:

- issue `#117` roadmap checklist
- `docs/specs/JS_GO_SYNTAX_COVERAGE_ROADMAP.md`

Principle:

- default direction is "make currently unsupported syntax work"
- unsupported decisions must be temporary and evidence-backed
- long-term target is "all language-level syntax/semantics covered"
- explicit non-targets are host-bound operations that cannot be made runtime-equivalent in pure Go compiler output (for example direct browser DOM control)

Current status and unsupported details are tracked by capability/syntax IDs, never by framework names.

## Quickstart

```bash
pnpm install --frozen-lockfile
pnpm run build

# Optional but recommended for local Rust-engine runs
cargo build -p engine-core
export TSGODOWN_RUST_ENGINE_BIN="$(pwd)/scripts/rust-engine-launcher.sh"
export TSGODOWN_ENGINE_CORE_BIN="$(pwd)/target/debug/engine-core"

cd examples/generic-simple-cli  # default neutral reference fixture
pnpm install
pnpm run build:go
```

Output:

- `examples/generic-simple-cli/dist-go/main.go`

Scaffold-oriented compatibility sample (Fastify-shaped) is available at:

- `examples/fastify-scaffold-real/src/app.ts`
- `examples/fastify-scaffold-real/src/routes/*`

Framework-agnostic simple CLI workspace sample is available at:

- `examples/generic-simple-cli/src/index.ts`

Compatibility-track framework samples (optional references, non-default) are available at:

- `examples/COMPAT_TRACK.md`
- `examples/fastify-scaffold-real/src/app.ts`
- `examples/hono-scaffold-real/src/index.ts`

## CLI behavior

- `tsgodown` (no subcommand) runs the compiler build flow (`build`).
- Supported subcommands: `build`, `check`, `report`, `stages`.
- The removed `compiler` transitional command is no longer accepted.

## Runtime contracts (404/405/Allow)

Generated Go runtime keeps HTTP mismatch behavior deterministic:

- **404 Not Found**: no route matched by path
- **405 Method Not Allowed**: path matched but HTTP method is unsupported
- **Allow header on 405**: includes allowed methods for the matched path (comma-separated)

This contract is verified in emitter/CLI tests and in smoke workflows.

## Diagnostics/Fix guide pointers

When build output is incomplete, start here:

- Failure triage playbook: [`docs/operations/FAILURE_TRIAGE_PLAYBOOK.md`](docs/operations/FAILURE_TRIAGE_PLAYBOOK.md)
- M1 release gate and acceptance criteria: [`docs/specs/M1_RELEASE_GATE.md`](docs/specs/M1_RELEASE_GATE.md)
- Testing strategy and boundaries: [`docs/specs/TESTING_STRATEGY.md`](docs/specs/TESTING_STRATEGY.md)
- Canonical compiler-mode spec lock (supported subset / out-of-scope / fail-closed): [`docs/specs/COMPILER_MODE_CONTRACTS.md`](docs/specs/COMPILER_MODE_CONTRACTS.md)
- JS<->Go syntax coverage roadmap mirror: [`docs/specs/JS_GO_SYNTAX_COVERAGE_ROADMAP.md`](docs/specs/JS_GO_SYNTAX_COVERAGE_ROADMAP.md)

Additional project docs:

- Architecture overview: [`docs/architecture/OVERVIEW.md`](docs/architecture/OVERVIEW.md)
- IR spec: [`docs/specs/IR_SPEC.md`](docs/specs/IR_SPEC.md)
- Capability matrix: [`docs/specs/CAPABILITY_MATRIX.md`](docs/specs/CAPABILITY_MATRIX.md)
- Artifact schema: [`docs/specs/ARTIFACT_SCHEMA.md`](docs/specs/ARTIFACT_SCHEMA.md)

## Development commands

- `pnpm run lint`
- `pnpm run format:check`
- `pnpm run test:tdd`
- `pnpm run perf:baseline`
- `pnpm run docs:scaffold:sync`
- `./scripts/smoke-m1.sh`

## License

MIT
