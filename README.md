# tsgodown

## Primary goal (direction lock)

`tsgodown` is a **compiler-mode pipeline for tsdown-bundleable
TypeScript/JavaScript Node.js code**.

The primary goal is fixed as:

1. **Everything `tsdown` can bundle is in scope**
   - the compiler input is the `tsdown` bundle plus sourcemap, `.d.ts`, and
     package metadata
   - applications, CLIs, libraries, frameworks, build tools, compilers, ORMs,
     and servers are all target workloads
   - Express, NestJS, Vite, Rollup, Webpack, Next, Nuxt, Astro, ESLint,
     Prettier, Babel, TypeScript, GraphQL, Apollo, Socket.IO, and ORM packages
     are validation corpus, not special cases
2. **Rust owns compiler semantics**
   - Rust parses/analyzes JavaScript and TypeScript artifacts, resolves modules,
     lowers executable IR, enforces diagnostics, and emits backend output
   - TypeScript/JavaScript in this repo owns CLI/config/tsdown orchestration,
     corpus gates, and UX only
3. **Go output is standalone**
   - generated output must compile as a normal Go project/binary using the Go
     toolchain
   - generated Go must not embed or shell out to Node, V8, Node-API, N-API, or
     native addon fallback paths
4. **Node.js 26 observable parity**
   - target behavior is Node.js 26 parity for every supported language/runtime
     capability
   - observable parity means matching stdout, stderr, exit code, JSON/library
     results, env, argv, cwd, filesystem side effects, async order, and observed
     error shape
   - unsupported or blocked surfaces must fail closed with deterministic
     diagnostics instead of silently generating wrong Go
5. **Backend-neutral compiler architecture**
   - IR and runtime contracts must not encode Go-specific concepts
   - Go is the first backend implementation, not the shape of the compiler
   - future Rust/C++/other backends should plug into the same backend interface

This repository remains intentionally strict: when code is outside the
implemented semantic envelope or cannot be extracted deterministically, the
compiler must emit explicit diagnostics and fail closed instead of silently
guessing.

Core execution path guardrail: framework-name branching/adapters are disallowed in `packages/core/src`, `packages/pipeline/src`, and `packages/cli/src/commands` (check with `pnpm run guard:core-path`).

Canonical compiler input contract (framework-agnostic): `tsdown` bundled JS +
sourcemap + `.d.ts` + package metadata are the source of truth for analysis and
Go emission. Framework fixtures are validation samples, not compiler-mode scope
boundaries.

## Current status

The long-term target is **all tsdown-bundleable Node.js 26 code**. Current
implementation is not there yet.

Current green phase:

- 10 real Node utility/library corpus entries are vendored.
- Each has 100 Vitest vectors.
- Node execution, generated Go build/run, and Node/Go vector parity are green
  for that phase.

Current gaps:

- Node.js 26 full API coverage ledger is planned but not complete.
- Large package/application corpus is planned but not implemented.
- Backend-neutral interface and Go backend plugin split need hardening.
- Some runtime semantics still live inside Go emitter templates and must move
  behind runtime contracts.
- Route/Fastify-era docs and fixtures are legacy validation samples, not final
  product scope.

Next target corpus tiers:

- `test-corpus/node-real/`: 10 utility/library corpus entries, 100 vectors each.
- `test-corpus/node-large/`: planned 20 framework/tooling/application corpus
  entries, 100 vectors each.
- Every corpus must prove the same thing: original Node.js 26 behavior equals
  generated standalone Go behavior, with no corpus-specific compiler hacks.

## Milestone lock (execution sequence)

Documentation and gate execution follow this fixed sequence:

`M5 -> M1 -> M2 -> M3 -> M4`

- `M5`: compiler-mode direction lock and contract freeze
- `M1`: canonical compile-success gate (`pnpm run gate:m1`)
- `M2`: generated runtime reachability acceptance
- `M3`: deterministic runtime behavior/perf guard extensions
- `M4`: architecture guardrails, triage/release discipline, and DoD closure policy

The sequence is locked for planning/reporting consistency and should be used in issue/PR text, roadmap updates, and release evidence.

## Legacy route-extraction grammar (current implementation snapshot)

The current extractor still supports deterministic route-extraction patterns
from the earlier service-focused milestone. These examples describe current
implementation residue, not the product boundary.

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
