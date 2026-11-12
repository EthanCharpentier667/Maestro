# 🎼 Maestro — orchestrateur IA pour ton kanban GitHub

Maestro transforme un **GitHub Project (V2)** en équipe de développement IA :
tu déplaces une carte dans la colonne déclencheur, un **orchestrateur** analyse le ticket, le route vers le bon
**spécialiste** (engine, gamedev, gamedesign, devops…) ou le découpe en sous-tickets avec dépendances.
Chaque spécialiste code dans **son propre git worktree** via **Claude Code** sur ta machine, ouvre une **PR**,
un **reviewer** la relit (boucle de corrections), et le kanban GitHub est mis à jour tout seul.

- **Backend Rust** (axum + tokio + SQLite) : un seul binaire, ~10 Mo, UI embarquée.
- **Claude Code en local** : utilise *ton compte Claude* (Pro/Max), un token `claude setup-token`, ou une clé API.
- **UI web temps réel** (SSE) : Board, Graph (DAG des tickets), State machine animée, Planner, Agents & skills, logs live.

---

## Installation (macOS / Linux)

Prérequis : [Rust](https://rustup.rs), `git`, et Claude Code :

```bash
npm i -g @anthropic-ai/claude-code
claude            # une fois, puis /login avec ton compte Claude
```

Lancer :

```bash
./run.sh          # = cargo build --release && ./target/release/maestro
# → http://127.0.0.1:7878
```

Variables d'env optionnelles :

| Variable | Défaut | Rôle |
|---|---|---|
| `MAESTRO_ADDR` | `127.0.0.1:7878` | adresse d'écoute (laisse en localhost : pas d'auth sur l'UI) |
| `MAESTRO_DATA` | `./maestro-data` | DB, agents, skills, clones, worktrees |
| `MAESTRO_GITHUB_API` | `https://api.github.com` | GitHub Enterprise : `https://ghe.x.com/api/v3` |
| `MAESTRO_GIT_BASE` | `https://github.com` | hôte git (GHE) |
| `RUST_LOG` | `maestro=info` | logs |

## Configuration (onglet Settings)

1. **Claude Code**
   - `account` → utilise le login local de `claude` (abonnement Pro/Max, les limites d'usage de l'abonnement s'appliquent).
   - `oauth_token` → token longue durée généré par `claude setup-token` (pratique sur un serveur / une autre machine).
   - `api_key` → clé `ANTHROPIC_API_KEY` facturée à l'usage.
   - Bouton **Test Claude** pour vérifier.
2. **GitHub** : un PAT classique (scopes `repo` + `project`) ou `gh auth login` puis mode *gh CLI*.
   Renseigne le propriétaire et le numéro du projet (`github.com/users/<owner>/projects/<n>`), le champ de statut
   (`Status`) et les noms de colonnes. **Test GitHub** liste les colonnes trouvées et celles qui manquent.
3. **Engine** : nombre d'agents en parallèle, tentatives, rounds de review, auto-merge…
4. **Git** : branche de base, et éventuellement des chemins locaux `owner/repo=/chemin/du/repo`
   (sinon Maestro clone dans son dossier de données).

Puis **▶ Start engine** en haut à droite.

## Comment ça marche

```
GitHub Project ──sync (poll)──► tickets locaux ──► state machine ──► Claude Code (worktree/branche par ticket)
      ▲                                                                    │
      └──────── statut de colonne, commentaires, PR, merge ◄───────────────┘
```

### State machine d'un ticket

`backlog → triage → (ready | blocked | split | needs_human) → running → reviewing ⇄ running → awaiting_merge → done`

- **triage** : routage direct si le ticket a un label `agent:<nom>` ou un label déclaré par un agent,
  sinon l'orchestrateur décide (`assign`, `split` en sous-tickets avec `depends_on`, ou `clarify` → il pose une question en commentaire).
- **blocked** : attend que ses dépendances soient `done`. Les dépendances sont lues dans le corps des issues
  (`Depends on #12`, `Blocked by #3`, `Requires owner/repo#5`) et éditables dans l'UI.
- **split** : passe à `done` quand tous les sous-tickets sont terminés.
- **running** : l'agent travaille sur `maestro/<n>-<titre>` dans un worktree isolé, commit ; Maestro push et ouvre la PR (`Closes #n`).
- **reviewing** : le reviewer (lecture seule) approuve ou renvoie un feedback au worker, qui corrige sur la même branche.
- **awaiting_merge** : tu merges (sur GitHub ou bouton *Merge PR*), ou auto-merge. Une PR mergée / issue fermée → `done`.
- **needs_human** : échec, aucun changement, question de l'orchestrateur… Boutons *Retriage* / *Retry agent*.

Le diagramme de l'onglet **State machine** est généré depuis la même table de transitions que le moteur (`src/model.rs`).

### Agents & skills

Les agents sont des fichiers Markdown dans `maestro-data/agents/` (éditables dans l'UI) :

```markdown
---
name: engine
description: Engine programmer — C++/Rust core systems…   # sert à l'orchestrateur pour router
role: worker            # worker | orchestrator | reviewer
model: sonnet           # opus | sonnet | haiku | id complet
color: '#4f8cff'
labels: [engine, rendering]      # labels GitHub routés directement vers lui
tools: [Read, Edit, Write, Glob, Grep, Bash]   # --allowedTools
skills: [cpp-engine, commit-conventions]
---
Prompt système ajouté à Claude Code…
```

Les **skills** sont des dossiers `SKILL.md` standards de Claude Code (`maestro-data/skills/`). Pour chaque agent, Maestro
génère un plugin Claude Code contenant ses skills et le charge avec `--plugin-dir` : chaque spécialiste n'a que ses skills.

Équipe par défaut : `orchestrator` (opus), `engine`, `gamedev`, `gamedesign`, `devops`, `reviewer`.

### Planner

Onglet **Planner** : tu décris un objectif (« ajouter un dash au joueur… »), l'orchestrateur inspecte le repo et propose un
graphe de tickets (agent + dépendances). Tu édites, puis **Create issues** : les issues sont créées dans le repo, ajoutées
au projet, et (si coché) mises en file directement.

## Sécurité

- L'UI n'a **pas d'authentification** : garde `MAESTRO_ADDR` sur `127.0.0.1`.
- Tokens stockés en clair dans `maestro-data/maestro.db` (local). Le token GitHub n'est jamais écrit dans la config git.
- Les agents exécutent du code sur ta machine. `acceptEdits` + une allow-list d'outils par agent est le défaut ;
  `bypassPermissions` uniquement sur une machine/VM dédiée.
- L'orchestrateur et le reviewer tournent en lecture seule (`Edit/Write` interdits).

## Développement

```
src/
  main.rs      boot, boucles, serveur
  model.rs     types + state machine (transitions autorisées)
  db.rs        SQLite (tickets, deps, transitions, runs, logs)
  github.rs    Projects V2 GraphQL + REST (issues, commentaires, PR)
  claude.rs    lancement `claude -p --output-format stream-json`, parsing, annulation
  git.rs       clones, worktrees, push
  engine.rs    sync, scheduler, triage, worker, review, planner
  api.rs       API HTTP + SSE + UI embarquée
web/           index.html, app.js, style.css (vanilla, sans build)
agents/ skills/  équipe par défaut (copiée dans maestro-data au 1er lancement)
tests/         mock GitHub + faux `claude` pour tester tout le cycle sans réseau
```

Test de bout en bout sans GitHub ni Claude :

```bash
python3 tests/mock_github.py 8787 &
mkdir -p /tmp/mtest/remotes/me && git init --bare -b main /tmp/mtest/remotes/me/game.git   # + un commit initial sur main
MAESTRO_GITHUB_API=http://127.0.0.1:8787 MAESTRO_GIT_BASE=file:///tmp/mtest/remotes cargo run --release
# Settings : claude_bin = tests/fake_claude.sh, token = x, owner = me, project = 1
```

`cargo test` couvre le parsing des dépendances et la state machine.
