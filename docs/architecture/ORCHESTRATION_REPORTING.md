# Orchestration Reporting Standard

## Goal
Report not just "whether it runs," but exactly "where and why it is blocked."

## Agent report template
1. Assigned task
2. What actually changed (file list)
3. Commands executed + real output
4. Current status (done/in-progress/blocked)
5. Exact blocker
   - file/function/line(or command)
   - error message
   - root cause hypothesis
6. Next action with ETA

## Blocker classification
- ENV: tool/bin missing, path, permission
- CONTRACT: interface/type mismatch
- LOGIC: test failure/behavior mismatch
- SCOPE: requirement ambiguity

## Escalation rule
If the same blocker repeats twice, immediately report to the orchestrator with a structural change proposal.
