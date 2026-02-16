# ADR-0001: Pipeline-first architecture

## Decision
Adopt `tsdown artifacts -> semantic index -> IR -> Go emitter` as the core architecture.

## Why
- Preserves source-level traceability via sourcemaps
- Reuses existing TS build ecosystem
- Enables progressive Node-compat implementation
- Keeps diagnostics actionable (original source mapping)

## Consequences
- Requires strong artifact contracts (`manifest.json`)
- Requires dedicated source map + d.ts linker
- Runtime shim is a first-class component, not optional
