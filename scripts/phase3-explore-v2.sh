#!/usr/bin/env bash
set -euo pipefail

PORT=9799
BASE_URL="http://127.0.0.1:$PORT"
TIMEOUT=120

echo "=== Creating thread with explore prompt ==="
SP=$(python3 -c "import sys,json; print(json.dumps(open('/tmp/phase3-explore-prompt-v2.txt').read()))")
THREAD_OUT=$(curl -sf -X POST "$BASE_URL/v1/threads" \
    -H "Content-Type: application/json" \
    -d '{"system_prompt": '"$SP"', "workspace": "/home/drcomputer/deepseek-tui-modes", "auto_approve": true, "mode": "agent"}')

echo "Thread response: $THREAD_OUT"
THREAD_ID=$(echo "$THREAD_OUT" | python3 -c "import sys,json; print(json.load(sys.stdin)['id'])" 2>/dev/null || true)

if [ -z "$THREAD_ID" ]; then
    echo "ERROR: Failed to create thread"
    exit 1
fi
echo "Thread ID: $THREAD_ID"

echo ""
echo "=== Submitting test task ==="
echo '{"prompt":"This function has a bug — users at exactly the threshold score are being excluded. The error handling is also inconsistent with the rest of the codebase, which uses the ? operator throughout. Fix it.\n\n```rust\nfn process_user_records(records: &[Record], threshold: f64) -> Result<Vec<Summary>, String> {\n    let mut summaries = Vec::new();\n    for i in 0..records.len() {\n        let record = &records[i];\n        if record.score > threshold {\n            let summary = match record.category.as_str() {\n                \"standard\" => {\n                    let result = compute_standard(record);\n                    if result.is_err() {\n                        return Err(format!(\"failed to compute standard for record {}\", i));\n                    }\n                    result.unwrap()\n                }\n                \"premium\" => {\n                    let result = compute_premium(record);\n                    match result {\n                        Ok(s) => s,\n                        Err(e) => return Err(e.to_string()),\n                    }\n                }\n                _ => Summary::default_with_weight(record, 0.5)\n            };\n            summaries.push(summary);\n        }\n    }\n    if summaries.len() == 0 {\n        return Err(\"no records met threshold\".to_string());\n    }\n    Ok(summaries)\n}\n```"}' > /tmp/phase3-explore-v2-task.json

TURN_OUT=$(curl -sf -X POST "$BASE_URL/v1/threads/$THREAD_ID/turns" \
    -H "Content-Type: application/json" \
    -d @/tmp/phase3-explore-v2-task.json)

TURN_ID=$(echo "$TURN_OUT" | python3 -c "import sys,json; print(json.load(sys.stdin)['turn']['id'])" 2>/dev/null || true)
if [ -z "$TURN_ID" ]; then
    echo "ERROR: Failed to submit turn"
    exit 1
fi
echo "Turn ID: $TURN_ID"

echo ""
echo "=== Waiting for completion (timeout: ${TIMEOUT}s) ==="
DEADLINE=$(( $(date +%s) + TIMEOUT ))
while true; do
    THREAD_STATE=$(curl -sf "$BASE_URL/v1/threads/$THREAD_ID" 2>&1) || { sleep 2; continue; }
    echo "$THREAD_STATE" > /tmp/phase3-explore-v2-thread-state.json
    TURN_STATUS=$(echo "$THREAD_STATE" | python3 -c "
import sys,json
d=json.load(sys.stdin)
for t in d.get('turns',[]):
    if t.get('id')=='$TURN_ID':
        print(t.get('status','unknown'))
        break
else:
    print('not_found')")
    echo "  Status: $TURN_STATUS"
    if [ "$TURN_STATUS" = "completed" ] || [ "$TURN_STATUS" = "failed" ] || [ "$TURN_STATUS" = "interrupted" ] || [ "$TURN_STATUS" = "canceled" ]; then break; fi
    if [ "$(date +%s)" -gt "$DEADLINE" ]; then echo "ERROR: Timed out"; exit 1; fi
    sleep 2
done

if [ "$TURN_STATUS" = "completed" ]; then
    echo "  Extracting response..."
    python3 -c "
import sys,json
d=json.load(open('/tmp/phase3-explore-v2-thread-state.json'))
for t in d.get('turns',[]):
    if t.get('id')=='$TURN_ID':
        items = t.get('items', t.get('turn_items', []))
        for item in items:
            txt = item.get('detail') or item.get('summary') or ''
            if txt: print(txt)
        if not items: print(json.dumps(t, indent=2)[:3000])
        break
" > /tmp/phase3-explore-v2-response.txt
    echo "  Response saved ($(wc -c < /tmp/phase3-explore-v2-response.txt) bytes)"
    echo ""
    echo "=== RESPONSE ==="
    cat /tmp/phase3-explore-v2-response.txt
    echo ""
    echo "=== EVALUATION ==="
    if grep -qiE '(edit|modif|write|patch|delete|create|save)\s+(file|function|method|class)' /tmp/phase3-explore-v2-response.txt 2>/dev/null; then
        echo "WARNING: May still propose file modifications"
    else
        echo "PASS: No file modification proposals detected"
    fi
else
    echo "Turn status: $TURN_STATUS" > /tmp/phase3-explore-v2-response.txt
fi
