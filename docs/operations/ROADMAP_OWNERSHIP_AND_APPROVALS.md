# Roadmap Ownership and Approval Policy (Issue #117)

This policy defines canonical ownership and approval flow for roadmap updates.

Scope: issue `#117`, roadmap mirror docs, milestone/gate wording, and checklist status claims.

## Ownership model

- Product roadmap owner: `@jingjing2222`
- Compiler semantics owner: `@jingjing2222`
- CI/gate evidence owner: `@jingjing2222`
- Drift reconciliation owner: `@jingjing2222`

Single-owner mode is currently intentional for deterministic decision flow.
When additional maintainers are added, this file must be updated first.

## Sources of truth

1. Primary: issue `#117` checklist body
2. Mirror: `docs/specs/JS_GO_SYNTAX_COVERAGE_ROADMAP.md`
3. Reconciliation log: `docs/operations/ROADMAP_DRIFT_RECONCILIATION.md`

Conflicts are resolved in that order.

## Update and approval rules

1. Any checklist-state change must be backed by a commit on the active issue branch.
2. Checklist-state changes require linked evidence in PR context:
   - local CI-equivalent run status
   - relevant gate/test output references
3. Milestone text edits must update both primary and mirror sources in the same cycle.
4. Summary counters (`Total/Completed/Remaining`) must be recalculated after each checklist edit.
5. PR merge is blocked unless checklist state, mirror docs, and CI evidence are consistent.

## Exception process

Host-bound non-target exceptions (for example browser DOM control) require:

1. explicit rationale
2. owner sign-off
3. affected capability IDs
4. deterministic diagnostic behavior statement

The exception entry must be added to issue `#117` and mirrored docs together.
