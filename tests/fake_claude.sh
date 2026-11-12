#!/usr/bin/env bash
# Fake `claude` CLI emitting stream-json, role detected from --json-schema. For e2e tests only.
[ "$1" = "--version" ] && { echo "0.0.0 (fake)"; exit 0; }
ARGS="$*"; PROMPT=$(cat)
emit(){ echo "$1"; }
emit '{"type":"system","subtype":"init","session_id":"s1","model":"fake","tools":["Read"]}'
if [[ "$ARGS" == *'"decision"'* ]]; then
  if [[ "$PROMPT" == *"Player can dash"* ]]; then
    OUT='{"decision":"split","reason":"needs design + code","subtasks":[{"key":"a","title":"Dash design doc","body":"Write GDD","agent":"gamedesign","depends_on":[]},{"key":"b","title":"Dash gameplay","body":"Implement","agent":"gamedev","depends_on":["a"]}]}'
  else OUT='{"decision":"assign","agent":"gamedev","reason":"ui work"}'; fi
elif [[ "$ARGS" == *'"approve"'* ]]; then
  if [ -f /tmp/mtest/reject_once ] && [ ! -f /tmp/mtest/rejected ]; then touch /tmp/mtest/rejected; OUT='{"approve":false,"summary":"missing cooldown","issues":["add cooldown constant"]}'
  else OUT='{"approve":true,"summary":"LGTM"}'; fi
else
  F="work_$(date +%s%N).txt"; echo "done by agent" > "$F"; git add "$F" >/dev/null 2>&1; git commit -qm "feat: work" >/dev/null 2>&1
  emit '{"type":"assistant","message":{"content":[{"type":"tool_use","name":"Write","input":{"file_path":"'"$F"'"}}]}}'
  OUT=null; RESULT="Implemented the ticket, wrote $F"
fi
sleep 0.5
if [ "$OUT" = null ]; then
  echo '{"type":"result","subtype":"success","is_error":false,"result":"'"$RESULT"'","num_turns":4,"total_cost_usd":0.02}'
else
  echo '{"type":"result","subtype":"success","is_error":false,"result":"ok","num_turns":2,"total_cost_usd":0.01,"structured_output":'"$OUT"'}'
fi
