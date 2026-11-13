---
name: maestro-operations
description: Run the studio day to day from chat - progress reports, planning a goal into tickets, starting/pausing the engine, diagnosing failed runs from logs, unblocking needs-human tickets, retrying, reassigning, merging, and tuning agents. Use for "where are we", "what's blocked", "plan X", "why did #n fail", "start/pause", "merge", "improve the agent".
---

# Operate Maestro

## Progress report ("où en est-on ?")

1. `maestro_status`, then `list_tickets` for detail if needed.
2. Answer in this shape (short):
   - **Headline**: one sentence (e.g. "7/12 done, 2 PRs waiting for your merge, 1 blocked on a question").
   - **Needs you** first: needs_human tickets with their note, awaiting_merge PRs (URLs), failing runs.
   - **In progress**: running/reviewing tickets with agent.
   - **Next up**: ready/blocked and what they wait for.
   - **Cost** so far (stats.total_cost_usd) when asked or when notable.
   - Engine state, and a proposed next action.
3. For a parent ticket that was split, report its sub-tickets as a progress fraction.

## Plan a goal into tickets

1. Clarify only what blocks planning (target repo, scope); otherwise state assumptions.
2. Look at the existing tickets (`list_tickets`) to avoid duplicates and to set dependencies on existing work.
3. Design the graph with `plan-tickets`: PR-sized tickets, one speciality each, acceptance criteria, `depends_on` keys,
   right agent or cell. Typical AYLO order: design doc -> engine API (with manual page + script test) -> gameplay ->
   docs / balance pass.
4. Show the plan as a table (key, title, agent, depends on) and **ask for confirmation** if more than 3 tickets.
5. `create_tickets` (`start: true` to queue them, false to leave them in the backlog). Report the created numbers.
6. Offer to start the engine if it is paused.

## Start / pause

- Start (`engine_control {enabled: true}`) when there are queued tickets and the user agrees. Mention `max_parallel`.
- Pause when the user asks, before switching project, or after repeated failures (stop burning money); running agents
  finish their current step.
- `engine_control {sync: true}` after the user changed things on GitHub.

## Diagnose a failure ("pourquoi #n a échoué ?")

1. `ticket_details` -> note, attempts, review rounds, feedback, transitions.
2. `list_runs {number}` -> the failed run -> `get_run_log {run_id, tail: 120}`.
3. Classify and act:

| Cause in the log | Fix |
|---|---|
| auth (`Invalid API key`, login required, 401) | tell the user to fix Settings -> Claude Code; nothing to retry |
| GitHub push / PR error (403, protected branch) | token scopes / branch protection; human fix |
| "no changes produced" | ticket too vague or already done: improve the body (comment), then `retry`, or close as duplicate |
| build/test failures the agent could not fix | comment with the precise error and guidance, `retry`; or split the ticket |
| reviewer rounds exhausted | read the feedback; decide: retry with a comment, reassign, or ask the human |
| budget / turns exceeded | split the ticket into smaller ones |
| missing engine capability (game ticket) | create the engine ticket and set it as a dependency (`ticket_action deps`) |

4. Say the cause in one sentence, the evidence (quote 1-3 log lines), and the action you propose or took.

## Unblock needs_human

- Clarification question: get the answer from the user, post it (`comment_ticket`), then `ticket_action retriage`.
- Wrong assignee: `ticket_action assign` (moves it to ready).
- Waiting on a dependency that will not happen: remove it (`ticket_action deps` with the new list) or close it.
- Duplicate / obsolete: `ticket_action close` with a reason comment (confirm first).

## Merge

Only when the user asks or confirms. Check the ticket is awaiting_merge (reviewed) and say what will be merged (PR URL).
`ticket_action merge` squash-merges; the ticket becomes done and its worktree is cleaned.

## Tune the team

When a pattern of failures points at an agent's instructions (same mistake twice), propose a concrete edit:
`get_definition` -> show the diff in plain words -> after confirmation `save_definition` with the full updated content
(keep the front-matter valid: name, role, model, tools, skills). Same for skills. Never delete agents from chat.

## Hygiene

- One action at a time when it is destructive; read the result before the next.
- Do not close or merge in bulk without an explicit list confirmed by the user.
- Keep the user in the loop with links (`issue url`, `pr url`) rather than bare numbers.
