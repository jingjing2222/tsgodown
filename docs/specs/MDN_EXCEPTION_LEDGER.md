# MDN Checklist Exception Ledger

This ledger tracks rows closed as `EXCEPTION` during issue `#117` execution.

## Entries

### mdn.classes.constructor

- child issue: #254
- status: EXCEPTION
- reason: Class constructor semantics are outside current analyzer target surface, which is limited to route extraction and deterministic scaffold generation.
- scope impact: No change to accepted route extraction subset semantics.
- re-entry condition: Reopen as `PLANNED` when class declaration/member semantics are added to supported analyzer surface.

### mdn.classes.extends

- child issue: #254
- status: EXCEPTION
- reason: Class inheritance semantics (`extends` + `super` dispatch) are outside the current analyzer target surface focused on route extraction and deterministic scaffold generation.
- scope impact: No change to current accepted route extraction subset semantics.
- re-entry condition: Reopen as `PLANNED` when class inheritance semantics are included in supported analyzer/runtime surface.
