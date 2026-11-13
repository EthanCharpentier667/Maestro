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
elif [[ "$ARGS" == *"mcp-assistant"* ]]; then
  # assistant chat turn: call a few admin tools and answer with what they returned
  CFG=$(printf "%s" "$ARGS" | grep -o -- "--mcp-config [^ ]*" | cut -d" " -f2)
  RESUMED=no; [[ "$ARGS" == *"--resume"* ]] && RESUMED=yes
  RESULT=$(python3 - "$CFG" "$PROMPT" "$RESUMED" <<'PY'
import json, sys, urllib.request
c = json.load(open(sys.argv[1]))["mcpServers"]["maestro"]; prompt = sys.argv[2]
def rpc(m, p=None, i=1):
    r = urllib.request.Request(c["url"], json.dumps({"jsonrpc": "2.0", "id": i, "method": m, "params": p or {}}).encode(), {**c["headers"], "content-type": "application/json"})
    return json.load(urllib.request.urlopen(r))
def tool(n, a=None):
    r = rpc("tools/call", {"name": n, "arguments": a or {}})["result"]
    return ("ERROR " if r["isError"] else "") + r["content"][0]["text"]
names = [t["name"] for t in rpc("tools/list")["result"]["tools"]]
out = {"tools": len(names), "resumed": sys.argv[3], "has_admin": "maestro_status" in names}
if "create repo" in prompt:
    out["repo"] = json.loads(tool("github_create_repo", {"name": "space-duel", "watch": True}))
    out["board"] = json.loads(tool("github_create_project", {"owner": "tester", "title": "Space duel", "link_repos": ["me/game"], "attach": "mirror"}))
    out["cfg"] = json.loads(tool("configure_project", {"max_parallel": 3}))
else:
    st = json.loads(tool("maestro_status"))
    out["status"] = {"project": st["project"], "source": st["source"], "per_state": st["tickets_per_state"]}
print(json.dumps(out).replace('"', "'"))
PY
)
  emit '{"type":"assistant","message":{"content":[{"type":"tool_use","name":"mcp__maestro__maestro_status","input":{}}]}}'
  OUT=null; RESULT="**Done.** $RESULT"
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
