# tsgodown Architecture Overview

## Goal
Project-scale TS/JS -> Go compiler pipeline using tsdown build artifacts.

## Primary Pipeline
1. `tsdown-driver`: build project and emit artifacts (bundle, sourcemap, d.ts, manifest)
2. `artifact-indexer`: link symbols across bundle <-> sourcemap <-> d.ts
3. `ir-core`: build semantic IR + run normalization/lowering passes
4. `node-compat`: resolve Node runtime/API semantics (native map/adapter/shim)
5. `go-emitter`: emit compilable Go project
6. `test-harness`: JS-vs-Go contract tests, e2e checks, regression snapshots

## Packages
- `packages/tsdown-driver`: tsdown orchestration and artifact capture
- `packages/artifact-indexer`: source map graph + symbol index
- `packages/ir-core`: compiler IR model + passes
- `packages/node-compat`: Node API support matrix + adapters
- `packages/go-emitter`: IR to Go codegen
- `packages/runtime-go`: shared Go runtime helpers
- `packages/pipeline`: end-to-end orchestration
- `packages/cli`: `tsgodown build/check/report`
- `packages/test-harness`: fixtures/golden/e2e runners

## Profiles
- `profiles/fastify`: first-class profile for long-term support
- `profiles/express`, `profiles/nest`: future profiles
