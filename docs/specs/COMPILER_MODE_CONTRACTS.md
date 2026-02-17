# Compiler Mode Contracts

## Supported Subset Contract

`tsgodown` only claims correctness for a declared, versioned supported subset.

- The subset is the union of patterns explicitly documented in capability/spec docs.
- Any source shape outside this subset is **out of contract**.
- Out-of-contract cases must produce deterministic diagnostics and fail closed.
- Shipping milestones must not silently expand claims beyond the documented subset.

In short: coverage claims are scoped to what we explicitly support, not the full TypeScript/Fastify ecosystem.

## Proof Contract

For each supported-subset feature, correctness is established by differential proof obligations.

Minimum proof obligations:

1. **Semantic differential tests**
   - compare TS runtime behavior and generated Go runtime behavior for equivalent inputs.
2. **Runtime compatibility layer verification**
   - document and test any semantic shims required to match TS/Fastify behavior.
3. **Fail-closed policy verification**
   - verify out-of-contract inputs fail with deterministic diagnostics as specified (never silently miscompile).
4. **Performance SLO gates**
   - enforce agreed build/runtime budgets so correctness is delivered at acceptable cost.

`100% behavioral coverage` means every behavior inside the supported subset is covered by these proof obligations and passes the differential gate.