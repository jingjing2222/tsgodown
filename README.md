# tsgodown

A long-term TypeScript/JavaScript → Go compiler project built around tsdown artifacts (bundle + sourcemap + d.ts), with a Fastify-first profile and an SSoT-driven architecture.

## Project Goals
- Keep a tsdown-like DX: `defineConfig` + `tsgodown.config.ts`
- Build a semantic compiler pipeline: artifacts → IR → capability gate → Go emitter
- Keep profile adapters thin (framework parsing only; no policy/rule ownership)
- Enforce TDD and CI as merge gates

## Current Status (early skeleton)
- Config loading/normalization
- Basic Fastify route detection
- Go `main.go` scaffold emission
- Initial SSoT docs:
  - `docs/specs/IR_SPEC.md`
  - `docs/specs/CAPABILITY_MATRIX.md`
  - `docs/specs/ARTIFACT_SCHEMA.md`

## Quick Start
```bash
pnpm install
pnpm run build
cd examples/fastify-min
node --import tsx ../../packages/cli/src/index.ts build
```

Generated output:
- `examples/fastify-min/dist-go/main.go`

## Development Commands
- `pnpm run lint`
- `pnpm run format:check`
- `pnpm run test:tdd`

## Migration Note
- Legacy package `@tsgodown/ir` is deprecated and intentionally inactive.
- Active IR model/package is `@tsgodown/ir-core`.

## License
MIT
