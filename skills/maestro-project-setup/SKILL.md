---
name: maestro-project-setup
description: Set up or change a Maestro project from chat - create a GitHub repository, watch repositories (repo mode), create a GitHub Project board and link or mirror it, configure labels, base branch and limits, switch projects. Use whenever the user wants to start a project, add a repo, create a board, or change how Maestro is wired to GitHub.
---

# Set up a Maestro project

## 0. Understand the request, then read the current state

`maestro_status` and `list_projects` first. Decide what the user actually wants:

| User says | You do |
|---|---|
| "start a new project / game", nothing exists yet | create repo(s) -> repo-mode project -> optional board -> plan first tickets |
| "use my repo X" / "add repo Y" | `github_repo_info` -> `add_project kind=repo` or `configure_project repos=[...]` |
| "I want a kanban on GitHub" | `github_create_project attach=mirror` (repo mode) |
| "use my existing board" | `add_project kind=project owner number` (board-driven) |
| "switch to the engine project" | `list_projects` -> `switch_project key` |

**Recommend repo mode** unless the user explicitly wants the board as the source of truth: it needs no `project`
token scope, works per repository, and the board can still be mirrored.

## 1. Create a repository (`github_create_repo`)

Confirm first, in one message: owner (user or organization), name, private/public (default private), description,
whether to initialise it (default yes: a README gives it a `main` branch agents can branch from - without it the first
ticket fails). Then call it with `watch: true` if it belongs to the active repo-mode project, or `add_project` right
after for a new project.

Naming: lowercase kebab-case, no spaces (`aylo-game`, `space-duel`). For a game on the AYLO engine, the game repo
holds `project.luau`, `scripts/`, `assets/` at its root (or under `game/`); the engine stays its own repository.

## 2. Create the Maestro project (`add_project`)

- `kind: repo`, `repos`: primary first (the repository most tickets target), then the others (e.g. game repo, then
  engine repo). `name`: what the user calls it.
- Maestro creates the labels on each repo: the trigger label (`maestro`) and `maestro:in-progress`,
  `maestro:in-review`, `maestro:done`.
- Switching pauses the engine. Tell the user, and do not start it until there is something worth running.
- Verify with `github_repo_info`: `push: false` means agents' branches cannot be pushed - say so and how to fix it (token
  scopes / collaborator access).

## 3. A GitHub board (`github_create_project`)

- `owner` = the user or organization that will own the board; `title`; `link_repos` = the project's repositories (they
  show it in their Projects tab).
- `attach: mirror` for the active repo-mode project (status columns follow the labels; issues are added as they move),
  `attach: switch` for a board-driven Maestro project, `none` to only create it.
- Maestro adds the missing Status columns (Todo, In Progress, In Review, Done). If the result notes it could not, tell
  the user to add "In Review" by hand in the board's Status field.
- Boards need the token's `project` scope; a "resource not accessible" error means that scope is missing.

## 4. Configure (`configure_project`)

Only pass what changes. Typical:
- `repos` (full list, primary first) - adding the engine repo to a game project so engine-gap tickets are tracked
  (tracked anyway once created, but importing its open issues needs it in the list);
- `base_branch` when work must branch from something other than the default branch (e.g. `develop`);
- `trigger_label` if the team already uses another label;
- `max_parallel` (cost and machine load: 1-3 on a laptop), `review_enabled`, `auto_merge` (only if the user asks; never
  for an engine with CI that matters), `max_attempts`, `max_review_rounds`;
- `mirror_owner`/`mirror_number` to attach an existing board as a mirror (`mirror_owner: ""` detaches it).

Things you cannot change from chat (tell the user where): Claude authentication and model (Settings -> Claude Code),
the GitHub token (Settings -> GitHub access or `.env`), local repo paths (Settings -> Project).

## 5. After setup - a checklist to report

- [ ] repositories reachable with push access (`github_repo_info`)
- [ ] labels created (mention them)
- [ ] board created/linked/mirrored, with its URL
- [ ] project active, engine paused
- [ ] next step proposed: plan the first tickets (`plan-tickets`), then start the engine

Report in a short table: what was created (with URLs), what was configured, what the user still has to do.

## Failure handling

- `422` on repo creation: the name exists - propose another name or to watch the existing one.
- `404` on an org: the token cannot see the organization or lacks `admin:org`/membership.
- `Agents are still running` on switch: wait, or pause the engine and retry once runs end.
- Never retry a creation blindly: read the error, then explain and propose.
