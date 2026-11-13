#!/usr/bin/env bash
# Fake `claude` CLI emitting stream-json, role detected from --json-schema. For e2e tests only.
[ "$1" = "--version" ] && { echo "0.0.0 (fake)"; exit 0; }
ARGS="$*"; PROMPT=$(cat)
[ -n "$FAKE_CLAUDE_LOG" ] && printf "%s\n" "$ARGS" | head -c 300 >> "$FAKE_CLAUDE_LOG" && echo >> "$FAKE_CLAUDE_LOG"
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
  # exercise the Maestro MCP server when asked (FAKE_MCP=1): create a follow-up ticket
  CFG=$(printf "%s" "$ARGS" | grep -o -- "--mcp-config [^ ]*" | cut -d" " -f2)
  if [ -n "$FAKE_MCP" ] && [ -n "$CFG" ] && [[ "$PROMPT" == *"Jump pad"* ]]; then
    python3 - "$CFG" <<'PY'
import json, sys, urllib.request
c = json.load(open(sys.argv[1]))["mcpServers"]["maestro"]
def rpc(m, p=None, i=1):
    r = urllib.request.Request(c["url"], json.dumps({"jsonrpc": "2.0", "id": i, "method": m, "params": p or {}}).encode(), {**c["headers"], "content-type": "application/json"})
    return json.load(urllib.request.urlopen(r))
rpc("initialize", {"protocolVersion": "2025-06-18"})
names = [t["name"] for t in rpc("tools/list")["result"]["tools"]]
res = rpc("tools/call", {"name": "create_ticket", "arguments": {"title": "Jump pad SFX", "body": "Add a sound when bouncing.", "assignee": "gamedev", "depends_on": [3]}})
own = rpc("tools/call", {"name": "close_ticket", "arguments": {"number": 3, "comment": "x"}})
open("/tmp/mtest/mcp.txt", "w").write(json.dumps({"tools": names, "create": res["result"]["content"][0]["text"][:300], "close_own_error": own["result"]["isError"]}))
PY
  fi
  # FAKE_CONFLICT=1: on "Main menu", edit a shared file and make someone else change it on the remote base
  # (merge conflict); on the next run, resolve it with `git merge` as instructed in the feedback.
  if [ -n "$FAKE_CONFLICT" ] && [[ "$PROMPT" == *"Main menu"* ]]; then
    REF=$(printf "%s" "$PROMPT" | grep -o 'git merge refs/maestro/base/[^` ]*' | head -1 | cut -d" " -f3)
    if [[ "$PROMPT" == *"Merge conflicts with the latest"* ]] && [ -n "$REF" ]; then
      git merge -q "$REF" >/dev/null 2>&1; git checkout --ours shared.txt 2>/dev/null; git add shared.txt; git commit -qm "merge base, resolve conflict" >/dev/null 2>&1
      echo resolved > /tmp/mtest/conflict_resolved
    elif [ ! -f /tmp/mtest/conflict_pushed ]; then
      echo "menu version" > shared.txt; git add shared.txt; git commit -qm "menu edits shared" >/dev/null 2>&1
      D=$(mktemp -d); git clone -q "$(git remote get-url origin)" "$D/c" && (cd "$D/c" && echo "other version" > shared.txt && git add shared.txt && git -c user.name=o -c user.email=o@x commit -qm other && git push -q origin HEAD:main) && touch /tmp/mtest/conflict_pushed
    fi
  fi
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
