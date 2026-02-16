# tsgodown Architecture Overview

## Goal
Project-scale TS/JS -> Go compiler pipeline using tsdown build artifacts, with **Rust as the only analysis/build core**.

## Ownership Boundaries (non-negotiable)
- **Rust core owns:**
  - build orchestration contract execution
  - source analysis / IR extraction
  - capability validation inputs
  - compile-time diagnostics contract
- **TypeScript owns only:**
  - CLI orchestration
  - UX/reporting surface
  - config loading / command routing
- **No fallback policy:**
  - Runtime path must not import or depend on legacy `@tsgodown/analyzer`.
  - If Rust engine is unavailable, pipeline fails with explicit source/cause/guidance errors.

## Primary Pipeline
1. `tsdown-driver`: invoke Rust engine build contract and persist artifact manifest
2. `rust core`: analysis + capability gate + build-time diagnostics
3. `ts orchestration`: command flow, summary formatting, user-facing output

## Packages
- `packages/tsdown-driver`: tsdown + rust engine adapter boundary
- `packages/pipeline`: orchestration-only runtime pipeline
- `packages/cli`: `tsgodown build/check/report/stages` user entry
- `packages/core`: command-level aggregation (runtime orchestration only)
- `packages/analyzer-rust` and `crates/*`: Rust analysis/build core

## Migration Checklist (TS core -> Rust core)
### Removed from runtime path
- [x] direct runtime dependency from `core` to `@tsgodown/analyzer`
- [x] direct runtime dependency from `pipeline` to `@tsgodown/analyzer`
- [x] TS analyzer import usage in runtime source (`core`/`pipeline`)
- [x] implicit fallback expectation that TS analyzer can rescue Rust failures

### Remains (by design)
- [x] TypeScript CLI/config/orchestration layers
- [x] Rust engine adapter contract in `tsdown-driver`
