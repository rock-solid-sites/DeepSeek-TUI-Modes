#!/usr/bin/env bash
# Phase 3b: Behavioral Validation — Remaining Presets + Debug Re-validation
set -euo pipefail

BINARY="$HOME/deepseek-tui-fork/target/release/deepseek-tui"
PRESETS="extend refactor methodical muse none debug"
PORT=9799
AUTH_TOKEN="phase3b-token-$(date +%s)"
BASE_URL="http://127.0.0.1:$PORT"
TIMEOUT=120

cleanup() {
    echo ""
    echo "=== Cleaning up daemon ==="
    if [ -n "${DAEMON_PID:-}" ]; then
        kill "$DAEMON_PID" 2>/dev/null || true
        wait "$DAEMON_PID" 2>/dev/null || true
    fi
    echo "Done."
}
trap cleanup EXIT

echo "=== Version check ==="
"$BINARY" doctor 2>&1 | head -5 || echo "WARN: doctor check failed"

echo "=== Killing stale daemon processes ==="
pkill -f "deepseek-tui serve" 2>/dev/null || true
sleep 1

echo "=== Starting daemon on port $PORT ==="
"$BINARY" serve --http --port "$PORT" --auth-token "$AUTH_TOKEN" &
DAEMON_PID=$!
echo "Daemon PID: $DAEMON_PID"

echo "=== Waiting for daemon health ==="
DEADLINE=$(( $(date +%s) + 15 ))
while true; do
    if [ "$(date +%s)" -gt "$DEADLINE" ]; then
        echo "ERROR: Daemon did not become healthy within 15s"
        exit 1
    fi
    if curl -sf "$BASE_URL/health" > /dev/null 2>&1; then
        echo "Daemon is healthy."
        break
    fi
    sleep 0.5
done

RESULTS=""
FAILURES=""

# -- Standard test task (for extend, refactor, methodical, muse, none) ---------
STANDARD_TASK='{"prompt":"This function has a bug \u2014 users at exactly the threshold score are being excluded. The error handling is also inconsistent with the rest of the codebase, which uses the ? operator throughout. Fix it.\n\n```rust\nfn process_user_records(records: \u0026[Record], threshold: f64) -> Result<Vec<Summary>, String> {\n    let mut summaries = Vec::new();\n\n    for i in 0..records.len() {\n        let record = \u0026records[i];\n\n        if record.score > threshold {\n            let summary = match record.category.as_str() {\n                \"standard\" => {\n                    let result = compute_standard(record);\n                    if result.is_err() {\n                        return Err(format!(\"failed to compute standard for record {}\", i));\n                    }\n                    result.unwrap()\n                }\n                \"premium\" => {\n                    let result = compute_premium(record);\n                    match result {\n                        Ok(s) => s,\n                        Err(e) => return Err(e.to_string()),\n                    }\n                }\n                _ => {\n                    Summary::default_with_weight(record, 0.5)\n                }\n            };\n\n            summaries.push(summary);\n        }\n    }\n\n    if summaries.len() == 0 {\n        return Err(\"no records met threshold\".to_string());\n    }\n\n    Ok(summaries)\n}\n```"}'

# -- Ambiguous debug task (for debug re-validation) ---------------------------
DEBUG_TASK='{"prompt":"Users are reporting that process_user_records sometimes excludes records it shouldn\u2019t. We haven\u2019t been able to reproduce it reliably. Investigate and report what you find.\n\n```rust\nfn process_user_records(records: \u0026[Record], threshold: f64) -> Result<Vec<Summary>, String> {\n    let mut summaries = Vec::new();\n\n    for i in 0..records.len() {\n        let record = \u0026records[i];\n\n        if record.score > threshold {\n            let summary = match record.category.as_str() {\n                \"standard\" => {\n                    let result = compute_standard(record);\n                    if result.is_err() {\n                        return Err(format!(\"failed to compute standard for record {}\", i));\n                    }\n                    result.unwrap()\n                }\n                \"premium\" => {\n                    let result = compute_premium(record);\n                    match result {\n                        Ok(s) => s,\n                        Err(e) => return Err(e.to_string()),\n                    }\n                }\n                _ => {\n                    Summary::default_with_weight(record, 0.5)\n                }\n            };\n\n            summaries.push(summary);\n        }\n    }\n\n    if summaries.len() == 0 {\n        return Err(\"no records met threshold\".to_string());\n    }\n\n    Ok(summaries)\n}\n```"}'

for preset in $PRESETS; do
    echo ""
    echo "=============================================="
    echo "=== Preset: $preset ==="
    echo "=============================================="

    PROMPT_FILE="/tmp/phase3b-${preset}-prompt-clean.txt"
    RESPONSE_FILE="/tmp/phase3b-${preset}-response.txt"

    if [ ! -f "$PROMPT_FILE" ]; then
        echo "ERROR: Prompt file $PROMPT_FILE not found"
        FAILURES="$FAILURES $preset(no-prompt-file)"
        continue
    fi

    SP=$(python3 -c "import sys,json; print(json.dumps(open('$PROMPT_FILE').read()))")

    echo "  Creating thread..."
    THREAD_OUT=$(curl -sf -X POST "$BASE_URL/v1/threads" \
        -H "Authorization: Bearer $AUTH_TOKEN" \
        -H "Content-Type: application/json" \
        -d '{"system_prompt": '"$SP"', "workspace": "/home/drcomputer/deepseek-tui-modes", "auto_approve": true, "mode": "agent"}' 2>&1) || {
        echo "ERROR: Thread creation failed"
        FAILURES="$FAILURES $preset(thread-create)"
        continue
    }

    THREAD_ID=$(echo "$THREAD_OUT" | python3 -c "import sys,json; print(json.load(sys.stdin)['id'])")
    echo "  Thread ID: $THREAD_ID"

    # Choose task: debug preset gets ambiguous task, others get standard
    if [ "$preset" = "debug" ]; then
        TASK_JSON="$DEBUG_TASK"
        echo "  Task type: ambiguous (investigation)"
    else
        TASK_JSON="$STANDARD_TASK"
        echo "  Task type: standard (fix)"
    fi

    echo "  Submitting turn..."
    echo "$TASK_JSON" > /tmp/phase3b-task-body.json
    TURN_OUT=$(curl -sf -X POST "$BASE_URL/v1/threads/$THREAD_ID/turns" \
        -H "Authorization: Bearer $AUTH_TOKEN" \
        -H "Content-Type: application/json" \
        -d @/tmp/phase3b-task-body.json 2>&1) || {
        echo "ERROR: Turn submission failed"
        echo "$TURN_OUT"
        FAILURES="$FAILURES $preset(turn-submit)"
        continue
    }

    TURN_ID=$(echo "$TURN_OUT" | python3 -c "import sys,json; print(json.load(sys.stdin)['turn']['id'])")
    echo "  Turn ID: $TURN_ID"

    echo "  Waiting for completion (timeout: ${TIMEOUT}s)..."
    DEADLINE=$(( $(date +%s) + TIMEOUT ))
    TURN_STATUS="running"
    while true; do
        THREAD_STATE=$(curl -sf "$BASE_URL/v1/threads/$THREAD_ID" \
            -H "Authorization: Bearer $AUTH_TOKEN" 2>&1) || {
            echo "  WARN: poll failed, retrying..."
            sleep 2
            continue
        }

        echo "$THREAD_STATE" > /tmp/phase3b-${preset}-thread-state.json

        TURN_STATUS=$(echo "$THREAD_STATE" | python3 -c "
import sys,json
d=json.load(sys.stdin)
for t in d.get('turns',[]):
    if t.get('id')=='$TURN_ID':
        print(t.get('status','unknown'))
        break
else:
    print('not_found')
")
        echo "  Status: $TURN_STATUS"

        if [ "$TURN_STATUS" = "completed" ] || [ "$TURN_STATUS" = "failed" ] || [ "$TURN_STATUS" = "interrupted" ] || [ "$TURN_STATUS" = "canceled" ]; then
            break
        fi
        if [ "$(date +%s)" -gt "$DEADLINE" ]; then
            echo "ERROR: Timed out"
            TURN_STATUS="timeout"
            break
        fi
        sleep 2
    done

    if [ "$TURN_STATUS" = "completed" ]; then
        echo "  Extracting response..."
        python3 -c "
import sys,json
d=json.load(open('/tmp/phase3b-${preset}-thread-state.json'))
for t in d.get('turns',[]):
    if t.get('id')=='$TURN_ID':
        items = t.get('items', t.get('turn_items', []))
        for item in items:
            txt = item.get('detail') or item.get('summary') or ''
            if txt:
                print(txt)
        if not items:
            print(json.dumps(t, indent=2)[:3000])
        break
" > "$RESPONSE_FILE" 2>&1

        if [ ! -s "$RESPONSE_FILE" ] || [ "$(wc -c < "$RESPONSE_FILE")" -lt 20 ]; then
            echo "  Trying SSE events..."
            curl -sf "$BASE_URL/v1/threads/$THREAD_ID/events?since_seq=0" \
                -H "Authorization: Bearer $AUTH_TOKEN" 2>/dev/null | \
                python3 -c "
import sys,json
for line in sys.stdin:
    line=line.strip()
    if line.startswith('data: '):
        try:
            evt=json.loads(line[6:])
            kind=evt.get('kind') or evt.get('event','')
            if kind in ('text','content','response','message'):
                txt=evt.get('content') or evt.get('text') or evt.get('data','')
                if txt: print(txt)
        except:
            pass
" > "$RESPONSE_FILE" 2>/dev/null || true
        fi

        RC=$(wc -c < "$RESPONSE_FILE")
        if [ "$RC" -lt 20 ]; then
            echo "WARN: Empty or minimal response for $preset ($RC bytes)"
            echo "(empty response - turn completed but no content extracted)" > "$RESPONSE_FILE"
        fi

        echo "  Response saved ($(wc -c < "$RESPONSE_FILE") bytes)"
        RESULTS="$RESULTS $preset(ok)"
    else
        echo "  Turn status: $TURN_STATUS"
        echo "Turn status: $TURN_STATUS" > "$RESPONSE_FILE"
        FAILURES="$FAILURES $preset($TURN_STATUS)"
    fi
done

echo ""
echo "=============================================="
echo "=== SUMMARY ==="
echo "=============================================="
echo "Successful: ${RESULTS:-none}"
echo "Failures:   ${FAILURES:-none}"
echo ""

for preset in $PRESETS; do
    RF="/tmp/phase3b-${preset}-response.txt"
    if [ -f "$RF" ]; then
        echo "  $preset: $(wc -c < "$RF") bytes | $(head -c 100 "$RF")"
    fi
done

echo ""
echo "=== Done ==="
echo "Response files: /tmp/phase3b-*-response.txt"
echo "Thread states:  /tmp/phase3b-*-thread-state.json"
echo ""
if [ -n "$FAILURES" ] && [ "$FAILURES" != "none" ]; then
    echo "Failures detected:$FAILURES"
    exit 1
fi
echo "All presets passed."
