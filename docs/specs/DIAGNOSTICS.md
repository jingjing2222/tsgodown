# DIAGNOSTICS

This document explains diagnostics emitted by the current analyzer pipeline.

## Fastify diagnostics status

Legacy Fastify-specific unsupported-pattern diagnostics were removed with the legacy analyzer path.
`packages/analyzer-rust` no longer emits the previous `ANALYZER_*` Fastify unsupported codes.

If/when new Fastify diagnostics are introduced again, this file must be updated in the same PR,
along with:

- `docs/specs/FASTIFY_SUPPORT_MATRIX.md`
- `docs/specs/FASTIFY_UNSUPPORTED_INVENTORY.md`

---

## Canonical diagnostic messages (verbatim)

These canonical lines must stay aligned with `packages/analyzer-rust`.

<!-- AUTO-GENERATED:DIAGNOSTIC_MESSAGES:START -->

<!-- AUTO-GENERATED:DIAGNOSTIC_MESSAGES:END -->

---

## Notes

- The empty generated block above is intentional while no Fastify diagnostics are emitted.
- Run `pnpm run docs:diagnostics:sync` after analyzer diagnostic changes.
