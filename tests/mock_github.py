#!/usr/bin/env python3
"""Minimal in-memory GitHub mock (GraphQL Projects V2 + REST issues/PRs) for end-to-end tests.
Run: python3 tests/mock_github.py 8787   then MAESTRO_GITHUB_API=http://127.0.0.1:8787"""
import json, re, sys
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

OPTS = [("o1", "Backlog"), ("o2", "Todo"), ("o3", "In Progress"), ("o4", "In Review"), ("o5", "Done")]
REPO = "me/game"
issues = {}   # number -> dict
items = {}    # item id -> {"issue": n, "status": name}
pulls = {}
comments = []
n_issue = [0]; n_pr = [100]

def add_issue(title, body, status="Backlog", labels=()):
    n_issue[0] += 1; n = n_issue[0]
    issues[n] = dict(number=n, title=title, body=body, state="OPEN", labels=list(labels), node_id=f"I_{n}")
    items[f"PVTI_{n}"] = {"issue": n, "status": status}
    return n

add_issue("Player can dash", "Add a dash ability with cooldown.", "Todo")
add_issue("Main menu", "Depends on #1\nSimple main menu.", "Backlog")
add_issue("Jump pad", "Bouncy pad that launches the player.", "Todo", ["cell:gameplay-squad"])
add_issue("Double jump", "Second jump in mid-air.", "Todo", ["cell:feature-pipeline"])

class H(BaseHTTPRequestHandler):
    def log_message(self, *a): pass
    def send(self, obj, code=200):
        b = json.dumps(obj).encode(); self.send_response(code)
        self.send_header("content-type", "application/json"); self.end_headers(); self.wfile.write(b)
    def body(self):
        l = int(self.headers.get("content-length", 0)); return json.loads(self.rfile.read(l) or b"{}")

    def do_GET(self):
        p = self.path.split("?")[0]
        if p == "/state": return self.send({"issues": issues, "items": items, "pulls": pulls, "comments": comments})
        if p == f"/repos/{REPO}": return self.send({"default_branch": "main"})
        m = re.match(rf"/repos/{REPO}/issues/(\d+)/comments", p)
        if m: return self.send([{"user": {"login": c["by"]}, "body": c["body"]} for c in comments if c["n"] == int(m[1])])
        if p == f"/repos/{REPO}/pulls":
            head = self.path.split("head=")[-1].split(":")[-1] if "head=" in self.path else None
            return self.send([{"number": k, "html_url": f"http://x/pull/{k}"} for k, v in pulls.items() if v["head"] == head and v["state"] == "open"])
        m = re.match(rf"/repos/{REPO}/pulls/(\d+)$", p)
        if m: v = pulls[int(m[1])]; return self.send({"merged": v["merged"], "state": v["state"]})
        self.send({"message": "nf"}, 404)

    def do_PUT(self):
        m = re.match(rf"/repos/{REPO}/pulls/(\d+)/merge", self.path)
        if m:
            pr = pulls[int(m[1])]; pr.update(merged=True, state="closed")
            issues[pr["closes"]]["state"] = "CLOSED"; return self.send({"merged": True})
        self.send({}, 404)

    def do_POST(self):
        p = self.path; b = self.body()
        if p == "/graphql": return self.graphql(b["query"], b.get("variables") or {})
        if p == f"/repos/{REPO}/issues":
            n = add_issue(b["title"], b["body"], None, b.get("labels", []))
            items[f"PVTI_{n}"]["status"] = None
            return self.send({"number": n, "html_url": f"http://x/issues/{n}", "node_id": f"I_{n}"}, 201)
        m = re.match(rf"/repos/{REPO}/issues/(\d+)/comments", p)
        if m: comments.append({"n": int(m[1]), "by": "maestro", "body": b["body"]}); return self.send({}, 201)
        if p == f"/repos/{REPO}/pulls":
            n_pr[0] += 1; closes = int(re.search(r"Closes #(\d+)", b["body"])[1])
            pulls[n_pr[0]] = dict(head=b["head"], base=b["base"], title=b["title"], state="open", merged=False, closes=closes)
            return self.send({"number": n_pr[0], "html_url": f"http://x/pull/{n_pr[0]}"}, 201)
        self.send({}, 404)

    def graphql(self, q, v):
        if "viewer" in q: return self.send({"data": {"viewer": {"login": "tester"}}})
        if "projectV2(number" in q:
            kind = "organization" if "organization(" in q else "user"
            if kind == "organization": return self.send({"data": {"organization": None}, "errors": [{"message": "no org"}]})
            return self.send({"data": {"user": {"projectV2": {"id": "PVT_1", "title": "Game board", "url": "http://x/p/1",
                "fields": {"nodes": [{}, {"id": "F_status", "name": "Status", "options": [{"id": i, "name": n} for i, n in OPTS]}]}}}}})
        if "items(first" in q:
            nodes = []
            for iid, it in items.items():
                i = issues[it["issue"]]
                nodes.append({"id": iid, "isArchived": False, "fieldValueByName": {"name": it["status"]} if it["status"] else None,
                    "content": {"__typename": "Issue", "id": i["node_id"], "number": i["number"], "title": i["title"], "body": i["body"],
                        "state": i["state"], "url": f"http://x/issues/{i['number']}", "labels": {"nodes": [{"name": l} for l in i["labels"]]},
                        "repository": {"nameWithOwner": REPO}}})
            return self.send({"data": {"node": {"items": {"pageInfo": {"hasNextPage": False, "endCursor": None}, "nodes": nodes}}}})
        if "updateProjectV2ItemFieldValue" in q:
            items[v["i"]]["status"] = dict(OPTS)[v["o"]]; return self.send({"data": {"updateProjectV2ItemFieldValue": {"projectV2Item": {"id": v["i"]}}}})
        if "addProjectV2ItemById" in q:
            n = int(v["c"].split("_")[1]); return self.send({"data": {"addProjectV2ItemById": {"item": {"id": f"PVTI_{n}"}}}})
        self.send({"errors": [{"message": "unknown query"}]})

ThreadingHTTPServer(("127.0.0.1", int(sys.argv[1]) if len(sys.argv) > 1 else 8787), H).serve_forever()
