# Maestro

Maestro turns GitHub issues into pull requests with a team of AI agents.

Point it at one or more repositories (or at a GitHub Project board), label an issue `maestro` (or move a card into
the trigger column of the board) and an orchestrator agent triages it: it either assigns the ticket to a
specialist (engine, gameplay, game design, devops...) or to a cell (a group of agents), splits it into
sub-tickets with dependencies, or asks a clarifying question. Each specialist works in its own git worktree using Claude Code on your machine, Maestro
pushes the branch and opens a pull request, a reviewer agent checks it (and sends it back for fixes if needed),
and the issue's `maestro:*` status labels (or the board columns) are kept in sync along the way.

An **Assistant** you chat with in the UI knows how Maestro works and drives it for you: it can create a GitHub
repository and watch it, create a GitHub Project board and mirror it, plan a goal into tickets, report progress,
explain a failed run from its log and unblock tickets.

- Rust backend (axum, tokio, SQLite), single binary with the web UI embedded.
- Runs the local Claude Code CLI: uses your Claude account (Pro/Max), a `claude setup-token` token, or an API key.
- Live web UI (server-sent events): assistant chat, board, ticket dependency graph, state machine view, planner, agent and skill editor, run logs.

## Requirements

- [Rust](https://rustup.rs)
- `git`
- Claude Code:

```bash
npm i -g @anthropic-ai/claude-code
claude   # run once and /login with your Claude account
```

## Running

```bash
cp .env.example .env   # then fill in your secrets
./run.sh               # cargo build --release && ./target/release/maestro
```

Then open http://127.0.0.1:7878.

Maestro reads a `.env` file from the working directory at startup (see `.env.example`). Secrets set there
override the Settings page and are never written to the database.

| Variable | Default | Purpose |
|---|---|---|
| `MAESTRO_GITHUB_TOKEN` | | GitHub personal access token (`repo`; `project` for boards; `admin:org` to create organization repositories). |
| `MAESTRO_CLAUDE_OAUTH_TOKEN` | | Token from `claude setup-token` (also reads `CLAUDE_CODE_OAUTH_TOKEN`). |
| `MAESTRO_ANTHROPIC_API_KEY` | | Anthropic API key (also reads `ANTHROPIC_API_KEY`). |
| `MAESTRO_ADDR` | `127.0.0.1:7878` | Listen address. Keep it on localhost, the UI has no authentication. |
| `MAESTRO_DATA` | `./maestro-data` | Database, agents, skills, clones, worktrees. |
| `MAESTRO_GITHUB_API` | `https://api.github.com` | For GitHub Enterprise, e.g. `https://ghe.example.com/api/v3`. |
| `MAESTRO_GIT_BASE` | `https://github.com` | Git host (GitHub Enterprise). |
| `RUST_LOG` | `maestro=info` | Log level. |

## Configuration

Everything is configured from the Settings page.

1. **Claude Code**
   - `account`: uses whatever the local `claude` CLI is logged into. Your subscription's usage limits apply.
   - `oauth_token`: a long-lived token from `claude setup-token`, useful on a server or another machine.
   - `api_key`: an `ANTHROPIC_API_KEY`, billed per use.
   - Use "Test Claude" to check the setup.
2. **GitHub access**: a classic personal access token with the `repo` scope (plus `project` for GitHub boards), or
   `gh auth login` and the "gh CLI" mode.
3. **Project** (where tickets come from), one of:
   - **Repositories** (default): `owner/name` per line, the primary first. Maestro imports their open issues; an issue
     carrying the **trigger label** (`maestro`) is picked up, and progress is shown with the labels
     `maestro:in-progress`, `maestro:in-review`, `maestro:done` (created on the repositories for you). Optionally,
     **mirror** to a GitHub Project board: issues are added to it and its Status column follows the labels.
   - **GitHub Project board**: owner and number (`github.com/users/<owner>/projects/<number>`), the status field
     (`Status`) and the column names; cards moved to the trigger column (`Todo`) are picked up.
   "Test connection" checks the repositories (push access) and the board columns.
4. **Execution**: number of parallel agents, attempts per ticket, review rounds, auto-merge, always-triage.
5. **Git**: base branch, and optional local paths (`owner/repo=/path/to/repo`, one per line). Without a path,
   Maestro clones the repository into its data directory.

**Several projects**: the switcher at the top of the sidebar (like GitHub Desktop) lists your saved projects.
"Add project…" lists the repositories your token can push to (pick several, the first is the primary) or the GitHub
boards it can see. Each project keeps its own source, labels or columns, base branch and repo paths (Settings →
Project). Only one project is active at a time: switching
pauses the engine, and is refused while agents are running.

Then click "Start engine".

## How it works

```
repo issues / Project board --(polling sync)--> local tickets --> state machine --> Claude Code (one worktree/branch per ticket)
      ^                                                                       |
      +-------- status labels / columns, comments, pull requests, merges <---+
```

### Ticket states

```
backlog -> triage -> ready | blocked | split | needs_human
ready -> running -> reviewing <-> running -> awaiting_merge -> done
```

- **triage**: tickets with an `agent:<name>` or `cell:<name>` label, or a label listed by an agent or cell, skip the orchestrator.
  Otherwise the orchestrator decides: `assign`, `split` (sub-tickets with `depends_on`), or `clarify`
  (it posts a question on the issue).
- **blocked**: waiting for dependencies to reach `done`. Dependencies are parsed from issue bodies
  (`Depends on #12`, `Blocked by #3`, `Requires owner/repo#5`) and can be edited in the UI.
- **split**: becomes `done` once all of its sub-tickets are done.
- **running**: the agent works on `maestro/<number>-<title>` in an isolated worktree and commits.
  Maestro pushes the branch and opens a PR that closes the issue.
- **reviewing**: the reviewer (read-only) approves or sends feedback; the worker fixes it on the same branch.
- **awaiting_merge**: merge on GitHub, with the "Merge PR" button, or enable auto-merge. A merged PR or closed issue moves the ticket to `done`.
- **needs_human**: the agent failed, produced no changes, or the orchestrator needs an answer. Use "Retriage" or "Retry agent".

The state machine page is generated from the same transition table the engine enforces (`src/model.rs`).

### Agents and skills

Agents are Markdown files in `maestro-data/agents/`, editable from the UI:

```markdown
---
name: engine
description: Engine programmer - C++/Rust core systems...   # used by the orchestrator for routing
role: worker            # worker | orchestrator | reviewer | assistant
model: sonnet           # opus | sonnet | haiku | full model id
color: '#4f8cff'
labels: [engine, rendering]                     # GitHub labels routed directly to this agent
tools: [Read, Edit, Write, Glob, Grep, Bash]    # passed as --allowedTools
skills: [cpp-engine, commit-conventions]
---
Appended to Claude Code's system prompt.
```

Skills are standard Claude Code `SKILL.md` folders in `maestro-data/skills/`. For each agent, Maestro generates a
Claude Code plugin containing only that agent's skills and loads it with `--plugin-dir`.

There is no limit on the number of agents or skills, and an agent can use any number of skills. The number of
tickets worked on at the same time is set by "Parallel agents" in Settings.

Default team: `maestro` (the assistant), `orchestrator`, `engine`, `gamedev`, `gamedesign`, `docs`, `devops`,
`reviewer`. Defaults added in a new version are copied into `maestro-data` once (files you edited or deleted are
left alone; `maestro-data/.seeded-defaults` records what was seeded).

### Several agents on one ticket

A ticket is assigned to an agent or a cell, and can also get **helpers**: extra agents picked in the ticket panel
(the orchestrator can add them too).

A **cell** is a reusable group of agents, stored in `maestro-data/cells/`:

```markdown
---
name: gameplay-squad
description: Gameplay feature squad.      # used by the orchestrator for routing
lead: gamedev
members: [gamedesign, engine]
mode: delegate                             # delegate | pipeline
labels: [squad]                            # GitHub labels routed to this cell
---
Instructions for the lead (delegate) or for every step (pipeline).
```

- `delegate`: one Claude Code session for the lead, with the members (and helpers) declared as Claude Code
  subagents through `--agents`. The lead delegates with the Task tool and integrates the result. The skills of
  every member are mounted.
- `pipeline`: the lead, then each member in order, one session each on the same branch. Every step receives the
  summaries of the previous ones. If a step fails, the attempt fails.

Default cells: `gameplay-squad` (delegate), `feature-pipeline` (gamedesign then gamedev) and `engine-feature`
(engine then docs).

### Tickets and the MCP server

Maestro talks to GitHub directly (REST for repositories, issues, labels and pull requests; GraphQL for boards). From the UI you can
create a ticket ("New ticket" on the board), and close or reopen an issue from the ticket panel. When all
sub-tickets of a split ticket are done, its issue is closed.

Maestro also serves an MCP server at `/mcp` (Streamable HTTP, protected by a token generated on first start).
Worker agents get it automatically through `--mcp-config`, so they can manage tickets themselves:

| Tool | Purpose |
|---|---|
| `list_tickets` | Tickets with state, assignee, dependencies (filter by state or title). |
| `get_ticket` | One ticket with its body. |
| `list_agents` | Agents and cells a ticket can be assigned to. |
| `create_ticket` | New issue on the board (follow-ups, bugs found, out-of-scope work), with assignee and dependencies. |
| `comment_ticket` | Comment on an issue. |
| `close_ticket` | Close an issue (duplicate, not needed). An agent cannot close the ticket it is working on. |

Tickets created by an agent link back to the ticket it was working on and land in the backlog, unless
"queue right away" is enabled in Settings. An agent can create at most 10 tickets per ticket it works on.
Disable the tools for an agent with `maestro_tools: false` in its front-matter.

The same server works from your own Claude Code (the exact command, with the token, is shown in Settings):

```bash
claude mcp add --transport http maestro http://127.0.0.1:7878/mcp \
  --header "Authorization: Bearer <token>"
```

### Assistant

The Assistant page is a chat with the `maestro` agent (Settings → Execution picks another one). Each conversation is
a Claude Code session resumed turn after turn (`--resume`); every turn starts with a snapshot of the live state. The
assistant cannot edit files or run commands: it acts through the MCP server in **assistant scope**, which adds:

| Tool | Purpose |
|---|---|
| `maestro_status` | Active project, engine, counts per state, tickets needing you, running agents, recent failures, cost. |
| `ticket_details`, `ticket_action` | Everything about a ticket; start, retriage, retry, pause, resume, cancel, reset, assign, helpers, deps, close, reopen, merge. |
| `create_tickets` | A dependency graph of tickets in one call (like the Planner). |
| `list_runs`, `get_run_log` | What agents did, and why a run failed. |
| `engine_control` | Start / pause the engine, sync now. |
| `list_projects`, `switch_project`, `add_project`, `configure_project` | Maestro projects and their settings (repos, trigger label, base branch, mirror board, limits). |
| `github_list_repos`, `github_repo_info`, `github_create_repo` | Repositories: list, check push access, create (and watch). |
| `github_list_projects`, `github_create_project` | GitHub boards: list, create with Maestro's columns, link to repos, attach as mirror or as a new project. |
| `get_definition`, `save_definition` | Read and edit agents, cells and skills. |

Its skills (`maestro-guide`, `maestro-project-setup`, `maestro-operations`) describe Maestro in detail and tell it to
confirm before anything outward-facing (creating repositories or boards, merging, closing, switching project).
To use the same tools from your own Claude Code, add `?scope=assistant` to the MCP URL.

### Planner

Describe a goal in the Planner page. The orchestrator reads the repository and proposes a set of tickets with
agents and dependencies. Edit them, then create the issues: they are added to the project and, optionally,
queued right away.

## Security

- The UI has no authentication. Keep `MAESTRO_ADDR` bound to `127.0.0.1`.
- Put secrets in `.env` (git-ignored). Secrets entered in the Settings page instead are stored in plain text in
  `maestro-data/maestro.db`. The GitHub token is never written to git config.
- In `account` mode, `ANTHROPIC_API_KEY` and `CLAUDE_CODE_OAUTH_TOKEN` are removed from the Claude Code
  environment so the local login is always used.
- Agents run code on your machine. The default is `acceptEdits` with a per-agent tool allow-list.
  Only use `bypassPermissions` on a dedicated machine or VM.
- The orchestrator and the reviewer run read-only (`Edit` and `Write` are disallowed).
- The MCP endpoint requires its token (`Authorization: Bearer`). The token is stored in the database and shown in Settings.

## Project layout

```
src/
  main.rs      startup, background loops, HTTP server
  model.rs     types and the state machine (allowed transitions)
  db.rs        SQLite: tickets, dependencies, transitions, runs, logs
  github.rs    REST (repositories, issues, labels, comments, pull requests) and Projects V2 GraphQL
  projects.rs  Maestro projects: switching, repo/board setup, labels
  claude.rs    runs `claude -p --output-format stream-json`, parses events, cancellation
  git.rs       clones, worktrees, push
  engine.rs    sync, scheduler, triage, worker, review, planner
  api.rs       HTTP API, SSE, embedded UI
  mcp.rs       MCP server (ticket tools, assistant admin tools)
  chat.rs      assistant conversations (resumed Claude Code sessions)
web/           index.html, app.js, style.css (plain JS, no build step)
agents/        default agents (copied to maestro-data on first start)
cells/         default cells
skills/        default skills
tests/         GitHub mock and fake `claude` CLI for end-to-end tests
```

## Testing

Unit tests:

```bash
cargo test
```

End-to-end without GitHub or Claude:

```bash
python3 tests/mock_github.py 8787 &
mkdir -p /tmp/mtest/remotes/me && git init --bare -b main /tmp/mtest/remotes/me/game.git   # then push an initial commit to main
MAESTRO_GITHUB_API=http://127.0.0.1:8787 MAESTRO_GIT_BASE=file:///tmp/mtest/remotes cargo run --release
```

In Settings, set the Claude binary to `tests/fake_claude.sh` and any token, then add the repository `me/game` (repo
mode) or the board owner `me`, number `1` (project mode). The fake CLI also answers Assistant messages by calling
the admin tools ("create repo" in the message creates a repository and a mirror board).

## License

MIT
