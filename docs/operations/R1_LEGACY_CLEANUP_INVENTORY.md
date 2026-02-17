# R1 Legacy Cleanup Inventory + Cut List

Status: R1 execution planning artifact (legacy/framework/subset-centric leftovers)
Date: 2026-02-17

## Scope
Inventory of remaining artifacts that are either:
- legacy migration residue, or
- naming/wording coupled to framework/subset transitional language.

Classification keys:
- **remove now**: safe to delete in immediate R1 cleanup PR.
- **rename now**: keep behavior, but rename wording/API/test labels now.
- **postpone**: intentionally retained for contract/gate stability; revisit later.

---

## 1) Inventory (code/docs/scripts/tests)

| Area | Path | Artifact | Classification | Reason |
|---|---|---|---|---|
| code (package) | `packages/ir/` (`package.json`, `README.md`) | Deprecated placeholder package `@tsgodown/ir (legacy)` | **remove now** | Migration is complete; package is inactive and only retains legacy marker text. |
| tests | `packages/cli/test/runtime-entry.test.js` | `legacy @tsgodown/ir package is marked inactive` test block | **rename now** | Keep guard intent, but rename to neutral “deprecated placeholder package remains inactive” until package deletion lands. |
| tests | `packages/cli/test/runtime-entry.test.js` | `legacy TS analyzer package is physically pruned` test title/message | **rename now** | Guard is still useful; “legacy” wording is migration-era and can be normalized. |
| tests | `packages/cli/test/commands.e2e.test.ts` | `CLI rejects removed legacy compiler command` case | **rename now** | Behavior must remain, wording can become “removed compiler alias/command”. |
| scripts/config | `package.json` (`gate:compliance`) | Test name pattern includes `removed legacy compiler command` | **rename now** | Sync with e2e test rename to avoid old terminology. |
| scripts | `scripts/guard-compiler-mode-only.mjs` | Forbidden path/reference list includes legacy migration markers | **postpone** | These are active anti-regression guardrails and should stay until all migration cleanup PRs settle. |
| code API | `packages/tsdown-driver/src/index.ts` | export names `resolveSubsetFromEntries`, `ResolverSubsetResult` | **postpone** | Public-ish API rename can cause ripple across callers/tests; defer to dedicated API-compat PR. |
| code internals | `packages/tsdown-driver/src/artifact-indexer/resolver.ts` | diagnostics wording `unsupported in resolver subset` | **postpone** | Diagnostic string changes may break snapshots/contract fixtures; do with coordinated fixture update. |
| tests | `packages/tsdown-driver/test/resolver-subset.test.ts` | file/test names with `subset` terminology | **postpone** | Should follow resolver API+diagnostics rename in one atomic PR to minimize churn. |
| scripts/tests | `scripts/differential-harness.mjs`, `packages/cli/test/differential-harness.test.ts` | report field `subset` and subset-centric description | **postpone** | Field is part of report contract; change requires contract version bump + fixture consumers update. |
| docs | `docs/specs/COMPILER_MODE_CONTRACTS.md`, `docs/specs/SEMANTIC_PARITY_CONTRACT.md`, `docs/specs/TESTING_STRATEGY.md`, `README.md` | supported-subset policy wording | **postpone** | This is current product contract, not accidental residue; cannot remove in R1 without spec change. |
| docs (framework-centric) | `docs/specs/FASTIFY_COMPILER_MODE_STATUS.md` | Fastify-specific framing | **postpone** | Still aligned with current milestone scope; move only when multi-framework strategy is approved. |

---

## 2) Actionable next-PR order (cut list)

1. **PR-A (low risk): terminology-only rename in tests/scripts**
   - Rename “legacy compiler command” test titles/messages and `gate:compliance` pattern.
   - Keep command behavior identical.

2. **PR-B (medium risk): remove deprecated `packages/ir` placeholder**
   - Delete package dir.
   - Update workspace/tests/docs that assert placeholder existence.
   - Ensure no lock/workspace regression.

3. **PR-C (medium/high risk): resolver “subset” naming cleanup (internal + tests)**
   - Rename test file names/internal symbol names first.
   - Keep external contracts stable or provide alias layer.

4. **PR-D (high risk): differential report contract rename (`subset` field)**
   - Introduce versioned schema update.
   - Update harness tests, fixtures, and downstream parser expectations.

5. **PR-E (strategy-dependent): docs/framework reframing**
   - Only after roadmap decision on non-Fastify scope.

---

## 3) Risk notes

- **Contract break risk**: diagnostic strings and JSON report fields are assertion targets in tests/fixtures.
- **Hidden consumer risk**: exported resolver symbols may be consumed by external/internal scripts beyond direct grep hits.
- **Gate fragility risk**: `gate:compliance` test-name filtering is string-sensitive; renames must stay synchronized.
- **Docs-policy risk**: “supported subset” language is intentional governance text; changing it prematurely weakens correctness boundaries.

## 4) R1 execution recommendation

For immediate R1, execute **PR-A + PR-B** only (high confidence, concrete debt reduction), and defer PR-C/D/E as scoped follow-ups with explicit contract-change notes.
