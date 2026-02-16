# Orchestration Reporting Standard

## 목적
'돌아가는지'가 아니라 '어디서 왜 막혔는지'를 정확히 보고한다.

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
- CONTRACT: 인터페이스/타입 불일치
- LOGIC: 테스트 실패/동작 불일치
- SCOPE: 요구사항 불명확

## Escalation rule
동일 blocker 2회 반복 시 즉시 오케스트레이터에게 구조 변경 제안 포함 보고.
