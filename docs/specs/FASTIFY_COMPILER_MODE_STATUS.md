# Fastify Compiler-Mode Status (v2 rollout)

This document is the user-facing status page for Fastify v2 support in `tsgodown`.

## Status policy

Support claims are valid only when all three are true:

1. Behavior is inside the declared compiler-mode subset.
2. Contracts/specs include that behavior explicitly.
3. Required local/CI gates pass for the current branch.

Canonical policy source: `docs/specs/COMPILER_MODE_CONTRACTS.md`.

## Fastify v2 support matrix

| Surface | Usable now | Not yet (must fail closed) | Contract / proof source |
|---|---|---|---|
| Route extraction | `fastify.<method>("/literal", handlerRef)` for `get/post/put/delete/patch`; analyzable `fastify.route({...})` with literal `url/path`, supported `method`, resolvable handler ref | Dynamic paths, unsupported route object shapes/method encodings, non-resolvable route handlers | `README.md`, `docs/specs/DIAGNOSTICS.md`, analyzer/emitter contract tests |
| Plugin graph | `fastify.register(...)` inline callback or named local plugin reference with deterministic `prefix` composition | Unresolved plugin refs, unsupported register callback patterns, conditional route registration | `README.md`, analyzer contract tests |
| Generated runtime contracts | Deterministic 404/405/`Allow` behavior for scaffolded router paths | Full Fastify runtime equivalence beyond subset | `docs/specs/SEMANTIC_PARITY_CONTRACT.md`, emitter + CLI tests, `scripts/smoke-m1.sh` |
| Out-of-subset handling | Deterministic diagnostics and compilation stop | Permissive fallback / partial silent success | `docs/specs/COMPILER_MODE_CONTRACTS.md`, `docs/specs/DIAGNOSTICS.md` |

## v2 rollout note

Fastify v2 support is a **phased subset rollout**. "v2 supported" means "supported subset usable now," not blanket framework-level compatibility.

Expansion rule: a capability moves from "not yet" to "usable now" only after:

- spec/contracts are updated,
- deterministic diagnostics boundaries are defined,
- differential/parity obligations are added where relevant,
- and required gates remain green.

## Current required gates (local and CI-aligned)

From repo root, run all of the following:

1. `pnpm install --frozen-lockfile`
2. `pnpm run lint`
3. `pnpm run format:check`
4. `pnpm run build`
5. `pnpm run test`
6. `cargo fmt --all --check`
7. `cargo clippy --workspace --all-targets -- -D warnings`
8. `cargo test --workspace --all-targets`
9. `./scripts/smoke-m1.sh`

Related references:

- Diagnostics contract: `docs/specs/DIAGNOSTICS.md`
- Capability boundary gate: `docs/specs/CAPABILITY_MATRIX.md`
- Executable M1 release gate: `docs/specs/M1_RELEASE_GATE.md`
- Test policy and required gates: `docs/specs/TESTING_STRATEGY.md`
