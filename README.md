# sanki-sei-random-engine

A complete engine for the **Sashité Engine Interface** — [SEI 1.0.0](https://sashite.dev/specs/sei/1.0.0/) —
that plays [Sanki](https://sashite.com/) (chess, ōgi and xiongqi on one 8×8
board, in any pairing) by picking a **random legal move**.

It is the engine to **fork**. Everything an SEI engine must do is here and
tested: the JSON Lines transport, the envelope and its fatal errors, `hello`,
`ping`, `configure`, `search`, `cancel`, the three error classes with their
JSON Pointers and their order, the canonical FEEN and PMN of *SEI Rules
Document — Sanki* (`sashite.sanki.kernel/1`), the endings that depend on
history (repetition, the move limit, the move cap), the `roots` feature, a
seeded generator for reproducible games. The one thing it does badly is
choose the move — and that is one function.

## The stack, in one table

| Concept | Where |
|---|---|
| The engine protocol | [SEI](https://sashite.dev/specs/sei/1.0.0/) — how a host and an engine talk |
| What SEI means for Sanki | *SEI Rules Document — Sanki*: canonical FEEN and PMN, styles `W` `J` `C`, the nine pairings |
| The rules | [`sashite-sanki-engine`](https://github.com/sashite/sanki-engine.rs): legal moves, `apply`, terminal statuses, and `pmn`, the PMN converters |
| The bot that hosts an engine on Nostr | [`sanki-bot.rs`](https://github.com/sashite/sanki-bot.rs): challenges, sessions, Plies, clocks, Conclusions — the plumbing you never write |
| **This engine** | the brain, replaceable |

You write an engine. The bot does the rest: it launches your engine as a
child process, sends it each position with the whole history, validates the
move it returns against the rules, and plays it on `relay.sanki.app`.

## Try it

```sh
cargo run --release
```

Then type, one request per line (the engine answers one event per line):

```
{"id":1,"op":"hello","versions":[1]}
{"id":2,"op":"configure","options":{"seed":7}}
{"id":3,"op":"search","rules":"sashite.sanki.kernel/1","position":"-rnbik^bn-r/+f+f+f+f+f+f+f+f/8/8/8/8/+P+P+P+P+P+P+P+P/-RNBQK^BN-R / W/j","moves":["e2-e4"],"clock":{"own":{"deadline":5000,"remaining":60000},"overhead":300}}
```

```
{"engine":{"name":"sanki-sei-random-engine",…},"ev":"done","re":1,"rules":{"sashite.sanki.kernel/1":{}},"version":1,…}
{"ev":"done","re":2}
{"depth":1,"ev":"info","nodes":1,"re":3,"variations":[{"pv":["g7-g5"],"score":{"cp":0}}]}
{"best":"g7-g5","ev":"done","re":3,"variations":[{"pv":["g7-g5"],"score":{"cp":0}}]}
```

Close the input (Ctrl-D) and the engine exits with code 0.

## Fork it

1. Fork the repository, rename the crate and the binary in `Cargo.toml`, and
   the `engine.name` in `src/protocol.rs` — that is what hosts log.
2. Replace [`search::choose`](src/search.rs). It receives the searched
   position, every legal move (or the host's `roots`), and the generator; it
   returns one of those moves in canonical PMN. `sashite-sanki-engine` gives
   you `engine::apply`, `engine::legal_moves`, `engine::status` and
   `Position` to look ahead; `pmn::to_pmn` writes the string.
3. Keep the tests: `cargo test` plays random games on the nine pairings and
   checks every answer the way a host does.

### When your engine starts to think

This engine answers in microseconds, so a single thread that reads the input
line by line is enough to answer `ping` at once and to end an infinite search
with the next request (SEI §7). An engine that searches for seconds must:

- **read its input on a thread of its own**, so that `ping` is answered
  within 50 ms and `cancel` is seen while searching;
- **stop by `own.deadline − overhead`** after receiving the `search`, and
  emit its `done` by then (§8.4): past that bound the host plays what it
  has — the last `info` — and treats the engine as failed after a short
  grace;
- **emit an `info` with a legal move early** — the safety net — then better
  ones as they come, at most about ten a second;
- treat `depth`, `nodes`, `movetime` and `mate` as hard caps.

The `search` is self-contained (the initial position and the whole history
come with every request), so the engine may keep tables between searches
but never needs to; a `search` with `fresh: true` must answer as a fresh
process would.

## What this engine does not do

- **Evaluate.** Every score is `cp: 0`. It never offers a draw or resigns; a
  host decides those on its own policy.
- **Wait.** A search with limits — `movetime` included — is answered at
  once, under the exception §8.4 *Without a clock* grants to a result that
  is proven final: a random pick cannot improve. An infinite search — no
  `clock`, no `limits` — is answered by its `info` at once and its `done`
  when the next request arrives, as the protocol says.
- **Replay by default.** The `seed` option defaults to `0`, which draws the
  seed from the clock: two bots left unconfigured do not play the same game
  twice. SEI's reproducibility (§8.4, a SHOULD) holds once a host configures
  a `seed`: a `fresh` search then answers the same `done` every time.
- **Judge a position beyond its form.** A FEEN is refused when malformed
  (`invalid`), outside the 8×8 board (`illegal`), with a style outside
  `W`/`J`/`C` (`unsupported`), or not in the canonical form the rules
  document fixes (`illegal`). A position that is canonical in form but
  unreachable — two royals in check at once, say — is played as it stands.

## Errors, as SEI classes them

| The host sent | The engine answers |
|---|---|
| a bad envelope: an `id` that does not increase, a duplicate key, a fraction, a request before `hello`, a second `hello` | `{"ev":"error","code":"invalid"}` without `re`, then exit code 1 |
| an unknown field, a value out of domain, a drop without its piece (`*e4`) | `invalid`, with the field's JSON Pointer |
| rules other than `sashite.sanki.kernel/1`, a style outside Sanki | `unsupported` |
| an illegal move, a move spelled in a non-canonical way (`e1-g1` for a castling), a move after the end of the game | `illegal`, with the pointer of the move |
| a non-canonical FEEN, a board that is not 8×8 | `illegal`, with `/position` |

When a request has several defects, one is reported, in SEI's order:
`invalid` before `unsupported` before `illegal`.

## Licence

Apache-2.0. Part of the [Sashité](https://sashite.com/) project.
