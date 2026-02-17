# FASTIFY_SUPPORT_MATRIX

Status: **SSOT** for Fastify mapping support in analyzer → IR extraction.

This document defines what Fastify route declaration patterns are currently mapped, what is conditionally supported, and what diagnostics are emitted for unsupported boundaries.

---

## Current status

The legacy Fastify unsupported-diagnostics analyzer path has been removed.
As a result, the prior `ANALYZER_*` Fastify unsupported diagnostics are not emitted by the current analyzer.

Use this file as the SSOT for future support-boundary changes. If unsupported diagnostics are reintroduced,
update this file, `DIAGNOSTICS.md`, and `FASTIFY_UNSUPPORTED_INVENTORY.md` in the same PR.

---

## Diagnostic code inventory linkage

The full code+message inventory is maintained in `docs/specs/FASTIFY_UNSUPPORTED_INVENTORY.md`.

<!-- AUTO-GENERATED:DIAGNOSTIC_CODES:START -->

<!-- AUTO-GENERATED:DIAGNOSTIC_CODES:END -->

- The empty generated block above is intentional while no Fastify unsupported diagnostics are emitted.
- Keep diagnostic code spellings exact and stable whenever new codes are added.
