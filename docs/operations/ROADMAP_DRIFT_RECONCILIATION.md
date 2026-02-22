# Roadmap Drift Reconciliation Log

This log tracks stale or contradictory roadmap/completion claims and their reconciliation status.

Source of truth: issue `#117` milestone checklist.

## 2026-02-23 reconciliation

- Milestone lock sequence wording normalized to:
  - `M0 -> M1 -> M2 -> M3 -> M4 -> M5`
  - updated in `README.md`, `docs/specs/TESTING_STRATEGY.md`, `docs/specs/FASTIFY_COMPILER_MODE_STATUS.md`
- Framework-shaped examples moved to explicit compatibility track:
  - `examples/fastify-scaffold-real`
  - `examples/hono-scaffold-real`
  - default gate path now uses non-compat baseline example
- Default gate/docs language aligned to capability-first wording:
  - docs/specs and README no longer treat framework sample names as product scope boundaries

## Reconciliation rule

- Any future mismatch between issue `#117`, normative docs, and active gates must be added to this file with:
  - mismatch summary
  - affected files
  - fix commit or PR link
