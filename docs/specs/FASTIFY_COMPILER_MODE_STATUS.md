# Fastify Compatibility Status (Non-Default Reference)

This document is a compatibility reference for Fastify-shaped fixtures.
It is not a default roadmap narrative for compiler scope.

Default roadmap and release decisions must be anchored in capability-first compiler contracts.
If framework-specific status is needed, keep it explicitly in compatibility context only.

Milestone sequence remains locked to roadmap issue `#117` for contract/evidence tracking:

`M0 -> M1 -> M2 -> M3 -> M4 -> M5`

Canonical compiler contract source:

- Compiler-mode spec lock + fail-closed policy: `docs/specs/COMPILER_MODE_CONTRACTS.md`

Related compiler references:

- Diagnostics behavior: `docs/specs/DIAGNOSTICS.md`
- Capability gate (compile viability): `docs/specs/CAPABILITY_MATRIX.md`
- Near-term execution gate: `docs/specs/M1_RELEASE_GATE.md`
- Compatibility backlog queue (post-M4 planning only): `docs/backlog/NODE_COMPAT_MATRIX.md`

Policy: support claims are made through compiler-mode contracts + differential proof obligations.
Framework fixture status does not imply default support boundaries unless the milestone gate evidence is green.
