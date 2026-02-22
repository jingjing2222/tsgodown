# MDN Checklist Exception Policy

This policy defines when a MDN JavaScript reference row can be closed as `EXCEPTION` for issue `#117` child checklists.

## Allowed `EXCEPTION` conditions

A row may be marked `EXCEPTION` only when all conditions are satisfied.

1. The capability is outside the current compiler target surface (route extraction + deterministic scaffold generation), not just "not implemented yet".
2. The capability cannot change accepted route extraction semantics within the current supported subset.
3. The row has an explicit evidence note in the child issue (reason + scope boundary).
4. Re-entry condition is documented (what must change to move back to `PLANNED`).

## Disallowed `EXCEPTION` use

- Using `EXCEPTION` to bypass missing implementation for an in-scope `LANG_CORE` or `BUILTIN` feature.
- Marking an item `EXCEPTION` without a concrete scope-boundary reason.

## Re-entry rule

When compiler scope expands to include the syntax/runtime surface, convert the row from `EXCEPTION` to `PLANNED` and reopen implementation tracking.
