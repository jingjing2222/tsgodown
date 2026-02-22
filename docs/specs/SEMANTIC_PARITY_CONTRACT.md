# Semantic Parity Contract (TS Runtime ↔ Go Runtime)

## Purpose
Define the **observable equivalence contract** between:

- TypeScript runtime behavior (reference)
- Generated Go runtime behavior (target)

for in-contract capability scenarios.

This document is the normative definition used by M1+ differential parity tests.

## Observable equivalence (normative)
For any request accepted by the contracted semantics surface, TS runtime and Go runtime are considered semantically equivalent when all parity dimensions below hold.

### Parity dimensions

| Dimension | Contract definition | Notes |
| --- | --- | --- |
| Status code | Exact numeric equality (`2xx/4xx/5xx` + exact code) | e.g. `200 != 201`; `404` and `405` must not be collapsed |
| Body | Byte-equivalent response body for scaffolded deterministic fixtures | For non-deterministic content, tests must assert a canonicalized comparator (explicitly documented per fixture) |
| Headers | Equality on the parity header set: `Content-Type`, `Allow` (when method mismatch), and any fixture-declared deterministic headers | Header name comparison is case-insensitive; values are compared after trimming OWS around comma-separated entries |
| Method behavior | Identical request dispatch outcome for equivalent scenario inputs | Ordering and fallback behavior must be deterministic and stable in fixtures |

## Non-goals (explicitly out of parity scope)

- Byte-for-byte framework-default header parity beyond the parity header set (e.g., `Date`, `Server`, transport/runtime injected headers).
- Streaming/chunking and connection-level behavior (keep-alive semantics, flush timing, HTTP/2 specific framing).
- Performance equivalence (covered by performance baseline/SLO docs, not semantic parity).
- Behavior for out-of-contract source patterns (those are governed by diagnostics/fallback contracts).

## Acceptance criteria by test layer

| Layer | Required acceptance criteria | Representative tests / artifacts |
| --- | --- | --- |
| Unit | Deterministic method matrix and header construction rules are stable (`Allow` generation, route method normalization). | `packages/emitter-go/test/emit-go.test.ts` |
| Integration | Rust analyzer/emitter contract preserves syntax capability semantics and diagnostics boundaries for the contracted semantics surface. | `packages/analyzer-rust/tests/contract_parity_regression.rs` |
| E2E differential parity | Same fixture requests against TS runtime and generated Go runtime satisfy parity dimensions (status/body/headers/method behavior). | `packages/cli/test/commands.e2e.test.ts` (M2/M3 acceptance tests) |
| Release gate | Canonical M1 gate proves build path and generated runtime scaffold viability (generation + compile path). | `scripts/m1-release-gate.sh`, `docs/specs/M1_RELEASE_GATE.md` |

## Pass/fail rule
A parity suite run is **PASS** only when every in-scope assertion in each required layer passes. Any mismatch in a normative parity dimension is a release-blocking failure for the corresponding milestone gate.
