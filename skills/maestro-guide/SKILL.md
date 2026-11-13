---
name: maestro-guide
description: Complete description of how Maestro works - sources (repo or GitHub Project), ticket state machine, triage/split/work/review loop, labels and columns, agents/cells/skills, worktrees and PRs, MCP tools, settings. Load it before answering any question about Maestro or explaining why a ticket is where it is.
---

# How Maestro works

Maestro is a local Rust server (web UI on `http://127.0.0.1:7878`) that turns GitHub issues into pull requests with a
team of Claude Code agents running on the user's machine. One Maestro **project** is active at a time; each project
keeps its own tickets and settings.

## 1. Where tickets come from (the source)

| Mode | Tickets | "Queue this" signal | Status shown on GitHub |
|---|---|---|---|
| **repo** (default) | Open issues of the watched repositories (`repos`, first = primary) | the **trigger label** (default `maestro`) | labels `maestro:in-progress`, `maestro:in-review`, `maestro:done` (+ `maestro:blocked` if a Blocked column is set) |
| **project** | Issues on a GitHub **Project V2** board | the card is in the **trigger column** (default `Todo`) | the board's Status column (`In Progress`, `In Review`, `Done`) |

- In repo mode an optional **mirror board** (a Project V2) can be attached: issues are added to it and its Status column
  follows the labels. Good for a kanban view on GitHub without making the board the source of truth.
- Sync is polling (default every 30 s; "Sync" button / `engine_control {sync: true}` forces one). Repo mode imports all
  open issues of the watched repos, plus tracked tickets in other repos (e.g. an engine ticket filed from the game).
- Closing an issue on GitHub makes the ticket Done. Deleting/transferring it removes the ticket.
- Pull requests are never tickets.

## 2. The ticket state machine

```
backlog --(trigger label/column, Start, created queued)--> triage
triage --assign--> ready | blocked (deps open)     triage --split--> split (waits for its sub-tickets)
triage --clarify--> needs_human (question posted on the issue)
blocked --deps done--> ready --free worker slot--> running
running --PR opened--> reviewing (or awaiting_merge if review is off)
running --failed--> ready (retry, up to max_attempts) | needs_human
reviewing --approved--> awaiting_merge   reviewing --changes requested--> running (up to max_review_rounds) | needs_human
awaiting_merge --PR merged / issue closed--> done        split --all sub-tickets done--> done
needs_human --Retriage / Retry--> triage / ready         any --Pause--> paused --Resume--> triage/ready
any --Reset--> backlog (clears the trigger)              any --issue closed--> done
```

States: `backlog`, `triage`, `split`, `blocked`, `ready`, `running`, `reviewing`, `awaiting_merge`, `done`,
`needs_human`, `paused`. Nothing moves unless the **engine is running** (except cheap bookkeeping: blocked/ready,
split -> done). The engine runs at most `max_parallel` agents at once; triage goes first, then ready tickets oldest
first.

## 3. The loop in detail

1. **Triage** (orchestrator agent, read-only on the repo). Fast path: a label `agent:<name>` / `cell:<name>` or a label
   listed in an agent's `labels` routes directly (unless "always triage"). Otherwise the orchestrator answers
   `assign` (agent or cell, optional helpers), `split` (2-8 sub-issues created with `Part of #n` / `Depends on #m`
   footers and `agent:`/`cell:` labels) or `clarify` (one question posted; the ticket waits in needs_human).
2. **Work** (the assigned agent or cell): Maestro clones the repo (or uses a local path from `repo_paths`), creates a
   **git worktree and branch per ticket** from the base branch, runs Claude Code headless there with the agent's
   system prompt, tools, skills (mounted as a plugin) and the Maestro MCP tools. The agent commits; it never pushes.
   Maestro pushes the branch and opens the PR (`Closes #n`, the agent's summary, diff stat). No changes = needs_human.
3. **Review** (reviewer agent, read-only): approve or list blocking issues; feedback goes back to the worker on the same
   branch; after `max_review_rounds` it needs a human.
4. **Merge**: a human merges on GitHub (or the Merge action / `auto_merge`). Merge -> done, worktree cleaned.

Cells: `delegate` = the lead runs one session with the members as Claude Code subagents; `pipeline` = lead then each
member in order on the same branch, each reading the previous summaries. Helpers on a ticket = extra subagents.

Dependencies: `depends_on` set by the planner/UI/MCP, or parsed from issue bodies ("Depends on #12", "blocked by
owner/repo#3"). A ticket never starts before its dependencies are Done.

## 4. The team (default library)

| Agent | Role |
|---|---|
| `maestro` | you: the assistant (never assigned tickets) |
| `orchestrator` | triage, split, planner |
| `engine` | C++ engine work (AYLO): ECS, plugins, Luau bridge, Vulkan, network, xmake; ships the Luau API with manual + tests |
| `gamedev` | Luau gameplay, `project.luau`, assets; files engine gaps |
| `gamedesign` | design docs, rules, balance data |
| `docs` | game developer manual and engine docs |
| `devops` | CI, build, packaging, release tooling |
| `reviewer` | reviews every PR |

Cells: `feature-pipeline` (gamedesign -> gamedev), `gameplay-squad` (gamedev leads gamedesign, engine, docs),
`engine-feature` (engine -> docs). Agents, cells and skills are markdown files the user can edit in "Agents, cells,
skills" (or you via `get_definition` / `save_definition`).

## 5. Tools agents get (MCP `maestro`)

Workers: `list_tickets`, `get_ticket`, `list_agents`, `create_ticket` (max 10 per ticket run; queued only if
"auto-queue" is on), `comment_ticket`, `close_ticket` (not their own). You (assistant scope) also get:
`maestro_status`, `ticket_details`, `ticket_action`, `create_tickets`, `list_runs`, `get_run_log`, `engine_control`,
`list_projects`, `switch_project`, `add_project`, `configure_project`, `github_list_repos`, `github_repo_info`,
`github_create_repo`, `github_list_projects`, `github_create_project`, `get_definition`, `save_definition`.

## 6. Settings that matter

- **Project tab**: source, repositories, trigger label, base branch (empty = repo default), local repo paths, mirror /
  board owner + number, column names.
- **Claude Code**: auth (local account, `claude setup-token` token, or API key), default model, permission mode, budget.
- **GitHub access**: a classic token with `repo` (+ `project` for boards) or the `gh` CLI login.
- **Execution**: `max_parallel`, `max_attempts`, `max_review_rounds`, orchestrator/reviewer agents, review on/off,
  auto-merge, always triage.
- **Git & MCP**: commit identity, auto-queue of agent-created tickets, the MCP command to use the same tools from
  your own Claude Code.

## 7. Answering "why is #n stuck?"

Check, in order: engine running? -> ticket state and `note` (`ticket_details`) -> open dependencies -> last runs and
their log tail (`get_run_log`) -> reviewer feedback and review rounds -> worker slots (`max_parallel` vs running) ->
last sync error. Then say the cause in one sentence and the fix (an action you can do, or what the human must do).
