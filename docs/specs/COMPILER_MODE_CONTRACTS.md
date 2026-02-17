# Compiler Mode Contracts (Canonical)

This document is the canonical policy source for compiler-mode support claims.

## Milestone lock anchor (M5)

This document is the contract anchor for `M5` in the locked sequence:

`M5 -> M1 -> M2 -> M3 -> M4`

All milestone evidence, roadmap references, and issue/PR wording must preserve this order.

## Declared semantic envelope

## 1) Declared semantic envelope (spec lock)

`tsgodown` is a compiler. It only guarantees correctness for a declared, versioned semantic envelope of input programs.

- The declared semantic envelope is the union of compiler-recognized source patterns explicitly listed in spec/capability documents.
- Any input program outside that envelope is **out of scope** for correctness claims.
- The compiler must not infer support from best-effort behavior or incidental pass cases.
- Milestone/release messaging must not expand claims beyond this locked semantic envelope.

In short: correctness claims are locked to explicit compiler contracts, not to any specific framework ecosystem.

## 2) Out-of-Scope Handling (Fail Closed)

For out-of-scope input programs, compiler behavior is fixed:

1. Emit deterministic diagnostics (stable code + reproducible location).
2. Stop compilation for that input (no silent partial success).
3. Never fall back to permissive/heuristic translation that could mask semantic mismatch.

This is a fail-closed compiler policy: no silent miscompile, no silent acceptance.

## 3) Proof Obligations for In-Scope Features

For each declared-contract feature, correctness is established by semantics-parity proof obligations.

Minimum obligations:

1. **Semantics parity tests**
   - Compare TypeScript runtime behavior vs generated Go runtime behavior for equivalent inputs.
   - Parity is measured by the normative dimensions in [`SEMANTIC_PARITY_CONTRACT.md`](./SEMANTIC_PARITY_CONTRACT.md) (status/body/headers/method behavior).
2. **Runtime-compatibility verification**
   - Specify and test semantic shims required to preserve source-program behavior.
3. **Fail-closed verification**
   - Verify out-of-scope inputs fail with deterministic diagnostics as specified (never silently miscompile).
4. **Performance SLO gates**
   - Enforce agreed compile/runtime budgets so correctness is delivered within accepted cost.

`100% behavioral coverage` means every behavior inside the declared semantic envelope is covered by these obligations and passes the semantics-parity gate.

## 4) Canonical References

- Capability boundary table: [`CAPABILITY_MATRIX.md`](./CAPABILITY_MATRIX.md)
- Diagnostic contract: [`DIAGNOSTICS.md`](./DIAGNOSTICS.md)
- M1 executable release gate: [`M1_RELEASE_GATE.md`](./M1_RELEASE_GATE.md)
- Test policy and required gate commands: [`TESTING_STRATEGY.md`](./TESTING_STRATEGY.md)
