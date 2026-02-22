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

Documentation and gate execution follow this fixed sequence:

`M5 -> M1 -> M2 -> M3 -> M4`

- `M5`: compiler-mode direction lock and contract freeze
- `M1`: canonical compile-success gate (`pnpm run gate:m1`)
- `M2`: generated runtime reachability acceptance
- `M3`: deterministic runtime behavior/perf guard extensions
- `M4`: architecture guardrails, triage/release discipline, and DoD closure policy

The sequence is locked for planning/reporting consistency and should be used in issue/PR text, roadmap updates, and release evidence.

## Current route-extraction grammar (reference boundary)

The current extractor supports deterministic route-extraction patterns that are stable for compiler-mode builds.
These examples describe the present reference boundary, not permanent framework-coupled scope.

- `fastify.<method>("/literal", handlerRef)` where method is `get|post|put|delete|patch`
- `fastify.route({ ... })` with inline object literal:
  - `method`: string (`"GET"`) or non-empty array (`["PUT", "PATCH"]`)
  - `url` or `path`: string literal
  - `handler`: named handler reference
- `fastify.register(...)` with:
  - inline callback plugin, or
  - named local plugin reference
  - optional `prefix` composition across nested plugins
- named handlers (including common member references) that the analyzer can resolve

Supported example:

```ts
import Fastify from "fastify";

const app = Fastify();

async function listUsers(req, reply) {
  return [{ id: "u1" }];
}

function replaceThing(req, reply) {
  reply.send({ ok: true });
}

app.get("/users", listUsers);
app.route({
  method: ["PUT", "PATCH"],
  url: "/things/:id",
  handler: replaceThing,
});
```

## Unsupported route patterns + diagnostics codes

When unsupported patterns are found, `tsgodown` emits warnings with deterministic codes.

Common unsupported patterns:

- dynamic/non-literal route paths
- inline/anonymous route handlers where named references are required
- `fastify.route(...)` object shapes that are not directly analyzable
- unresolved plugin references
- conditional route registration (`if (...) { fastify.get(...) }`)

Example (unsupported):

```ts
import Fastify from "fastify";

const app = Fastify();

const dynamicPath = "/users/" + process.env.VERSION;
app.get(dynamicPath, async (req, reply) => {
  reply.send({ ok: true });
});

if (process.env.EXPERIMENTAL === "1") {
  app.get("/exp", expHandler);
}
```

Diagnostics you will see:

- `ANALYZER_UNSUPPORTED_DYNAMIC_PATH`
- `ANALYZER_UNSUPPORTED_INLINE_HANDLER`
- `ANALYZER_UNSUPPORTED_ROUTE_OBJECT_SHAPE`
- `ANALYZER_UNSUPPORTED_ROUTE_OBJECT_METHOD`
- `ANALYZER_UNRESOLVED_PLUGIN`
- `ANALYZER_UNSUPPORTED_REGISTER_CALLBACK`
- `ANALYZER_UNSUPPORTED_CONDITIONAL_ROUTE`

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

Official-style Hono workspace sample is available at:

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
