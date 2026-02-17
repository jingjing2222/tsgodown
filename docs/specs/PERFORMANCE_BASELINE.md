# Performance Baseline & Regression Guard (M3 in locked sequence M5 -> M1 -> M2 -> M3 -> M4)

## Goal
Provide a repeatable baseline/perf-regression scaffold for CLI orchestration latency.

## Scenarios
Current baseline scenarios (defined in `packages/cli/src/perf-baseline.ts`):
- `cli-build-fastify-scaffold-real`
- `cli-check-multi-file`
- `cli-report-route-object-variants`
- `cli-stages-nested-register-prefix`

Each scenario specifies:
- fixture project
- command (`build`/`check`/`report`/`stages`)
- warmup run count
- sample run count
- p95 threshold (ms)
- regression tolerance (%) against baseline median

## Measurement command
From repo root:

```bash
pnpm run perf:baseline
```

Behavior:
- runs all scenarios
- writes report JSON to `artifacts/perf/report.json`
- compares current median against `docs/perf/baseline.json` (if baseline median is set)
- exits non-zero on threshold failure or regression failure

## Updating baseline
After intentional performance improvements (or accepted budget changes):

```bash
pnpm run perf:baseline -- --update-baseline
```

This rewrites `docs/perf/baseline.json` with measured medians.

## Report format
`artifacts/perf/report.json` contains:
- run metadata (`generatedAt`, `host`, `platform`)
- per-scenario samples and summary stats (`mean`, `median`, `p95`, `min`, `max`)
- threshold check result
- baseline median and regression delta (%/ms)
- final boolean status (`ok`)

## Notes
- This is scaffolding for M3 and currently uses a deterministic Rust-engine stub to isolate CLI/pipeline orchestration overhead.
- Once CI hardware targets and stable runtime envelopes are finalized, set concrete baseline medians in `docs/perf/baseline.json` and gate PRs on non-regression.
