# Changelog

All notable changes to this crate are documented in this file. The format is
based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this
crate adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] — 2026-09-28

The first SEI engine: a complete implementation of the engine side of
[SEI 1.0.0](https://sashite.dev/specs/sei/1.0.0/) for Sanki
(`sashite.sanki.kernel/1`), whose only judgement is a random pick.

### Added

- The JSON Lines transport and the envelope: strict I-JSON (duplicate keys
  and non-integers refused), fatal errors without `re` and a non-zero exit,
  exit 0 at the end of the input, blank lines and CR tolerated.
- `hello` (version 1, the rules identifier, the `roots` feature, the `seed`
  option), `ping`, `configure` (atomic, complete), `search`, `cancel`.
- `search`: validation in SEI's order (`invalid`, then `unsupported`, then
  `illegal`, one defect with its JSON Pointer), the canonical FEEN and PMN
  of *SEI Rules Document — Sanki* through `sashite-sanki-engine`'s `pmn`,
  the replay of the history with the repetition, move-limit and move-cap
  endings, `roots`, the safety-net `info`, an infinite search ended by the
  next request, `fresh` and a seeded generator for reproducible games.
- Tests that drive the binary as a host does: the error classes, the fatal
  envelope errors and exit codes, the infinite search with `ping` and
  `cancel`, `roots`, seeded replay, and random games on the nine pairings
  with every answer validated by the rules.
