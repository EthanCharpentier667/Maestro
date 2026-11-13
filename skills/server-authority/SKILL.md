---
name: server-authority
description: The AYLO client/server model - who owns which data, presence and side, replication modes, inputs, per-client entities, custom messages, and defensive handling of network bytes. Use when designing or reviewing anything networked, replicated, or that runs on only one side, in C++ or in Luau.
---

# Server authority in AYLO

Reference: `manual/pages/networking.md`, `docs/network/design.md`, `docs/network/protocol.md`,
`docs/network/custom-packets.md`, root `AGENTS.md` rule 6. A networked game is **one server and several clients, all
running the same scripts**.

## The model

- **The server is authoritative.** It runs gameplay and owns the truth. A client never writes replicated data.
- **Clients draw and send inputs.** The only way a client affects the game is its input actions; the server decides.
- **Replication copies server -> clients.** A component with `replication = "everyChange"` or `"spawnOnly"` appears on
  the clients with its entity and is updated. `"none"` stays local.
- **Events do not cross the network.** For something lasting, put it in a replicated component (e.g. a `game.Winner`
  spawned by the server). For a moment or request (a kick, picking a team) use custom messages
  (`docs/network/custom-packets.md`).

## Sides

| | `server` | `client` | `both` |
|---|---|---|---|
| system `side` | gameplay (default) | presentation | identical simulation on both |
| component `presence` | `serverOnly` | `clientOnly` | `both` (default) |

Both sides declare the **same** components (a client that declares different ones is refused on connect) and the same
input actions. Rule of thumb: the server spawns the entities that matter and writes their gameplay components; a
client system reacts to replicated entities (`added = {...}`) and adds what only it needs (transform, sprite, sound,
text) with `clientOnly` components.

## Players and inputs

- Engine events on the server: `net.ClientConnected`, `net.ClientDisconnected` (field `client`).
- `net.Owner { client }` ties an entity to a client (replicated); `net.PlayerInput` receives that client's inputs.
- A client learns its id with `aylo.replication.localClient` (`{ connected, client }`).
- Declare actions with `engine.input.action` in a file **both** sides load. On the server read them with
  `aylo.replication.readInput` (`held`, `pressed`, `released`, `axis`, `vector`), in a system that lists the call and
  runs **after `net.applyInputs`**.

## Design checklist (use it for every new datum)

1. Which side writes it? (exactly one)
2. Which sides need to read it? -> `presence` and `replication`.
3. Does it change every tick (`everyChange`, mind bandwidth) or only at spawn (`spawnOnly`)?
4. Is it client-only cosmetic state (particles, camera shake)? Then it must not influence gameplay.
5. Can a malicious client abuse it? A client message is a *request*; the server validates range, rate, ownership and
   game state before acting. Never trust a client-provided position, damage, or id.

## C++ side (engine work)

- Bytes from the network never crash, throw or assert: validate every length/field/enum and drop or disconnect with a
  result. Fuzz-style tests for new packet parsers (`tests/network/`, `tests/replication/`).
- `network` knows no ECS; `replication` is the only module knowing both. A new replicated type is registered via
  reflection (`add-component`); its layout is part of the manifest, so adding/reordering fields changes both sides.
- Server-only effects of an engine call must not run on a client (the call is absent there).

## Review red flags

- A client system writing a replicated component or spawning gameplay entities.
- Gameplay logic gated on `engine.isClient`/`isServer` inside a shared file instead of living under that side's entry.
- Reading input on the client to move the authoritative transform directly.
- A new packet whose length field is used before being checked.
- Random numbers in `both` systems without a replicated seed (desync).
