# Native Capture and Replay Implementation Plan

> Historical plan, completed and published in `f550c02`; CI run 43686603978 succeeded. Read [HANDOFF.md](../../HANDOFF.md) for current status. The task ownership below describes that completed implementation, not a new dispatch.

**Goal:** Extend evidence-backed providers and deliver portable local capture export/replay.
**Architecture:** Native Rust adapters feed the existing durable journal; an offline bank preserves and matches transport transcripts.
**Tech Stack:** Rust, existing Cargo dependencies, offline tests.
**Spec:** docs/superpowers/specs/2026-11-15-native-capture-replay.md

## Global Constraints
- No live provider requests or bets during implementation/testing.
- No financial reconciliation gate for completion or export.
- Fresh sessions; durable journal before outbound actions; unknown events remain partial.
- Concurrency stays per provider/exit, game modes sequential.
- Existing evidence remains read-only; no old runner modules imported at runtime.
- Replay distinguishes exact requests and choice values; this delivery retains recorded identities/counters without substitutions.
- Preserve existing files and never silently overwrite captures or fall back from requested VPN to direct networking.

## Review Focus
- Ruby init may omit sessionData; nonterminal action responses must still carry continuation state.
- Unknown provider continuations must stop without invented commands.
- Replay mismatch, truncated journals and binary bodies must retain data and never return an unrelated outcome.
- Missing WireGuard files must fail configuration; explicit direct exits must keep working.
- Interrupted metadata writes must not poison resumable run directories.

## Task 1: RubyPlay native capture expansion
Files: adapters/rubyplay_extra.rs, rubyplay.rs if needed, bindings/rubyplay*.json, focused fixtures and offline tests.
- [x] Pin observed init, purchase, continuation and choice behavior with failing tests.
- [x] Add evidence-derived profiles for the 255 closed game/mode pairs (179 games), retaining unsupported missing/rejected cases explicitly.
- [x] Verify every retained capture sequence offline, preserving base64 game data and version/action state.
- [x] Report exact coverage and provenance.

## Task 2: Playtech GPAS native expansion
Files: adapters/playtech.rs and owned child modules, playtech bindings/fixtures.
- [x] Pin supplied entry/continuation/choice commands and compressed responses in offline tests.
- [x] Port source-backed GPAS contract/configuration/selector resolution and native round state machine.
- [x] Enable evidence-supported modes with source/version checks; keep unresolved special engines and incomplete bindings pending.
- [x] Report exact supported inventory and redacted evidence limitations.

## Task 3: Reliability and Kalamba
Files: config.rs, wireguard.rs, journal.rs, engine.rs only if needed, adapters/kalamba.rs and bindings/fixtures, tracked target symlink and ignore entries.
- [x] Reproduce then fix missing WG config fallback, resolver permissions under restrictive umask, atomic new metadata writes.
- [x] Remove tracked target symlink and prevent new build artifacts entering Git.
- [x] Extend Kalamba normal profile from 1 to 66 evidence-confirmed inventory games; unknown continuation remains partial.
- [x] Run targeted offline regressions.

## Task 4: Portable capture bank and local replay
Files: src/capture_bank.rs, src/ruby_import.rs, src/replay.rs, main.rs, lib.rs, tests/replay_cli.rs, tests/ruby_import.rs, README/docs.
Interfaces: export a run/attempt directory to versioned self-contained JSON bank; replay a bank attempt locally by ordered request matching, yielding exact recorded inbound data.
- [x] Write failing tests for raw HTTP chunks/WS binary, multiple attempts, partial trailing line, mismatched choice, exact identifier preservation and no overwrite.
- [x] Implement export preserving metadata/results/raw events and transport grouping.
- [x] Implement offline CLI transcript replay with strict matching and preserved recorded identities/counters. Identity substitution was not implemented and is outside this delivery.
- [x] Test command-line export/replay on a real-format local fixture.

## Integration
- [x] Regenerate coverage and binding provenance; document actual limits and commands.
- [x] fmt, full offline tests, clippy, provenance verification, clean build output.
- [x] Independent review and fixes.
- [x] Publish code to the requested repository with an expected-head check.

## Result and implementation decisions

RubyPlay: 255 modes / 179 games; Playtech: 100 modes / 48 games; Kalamba: 66 normals. New CLI commands: export, import-ruby, replay. 264 offline tests passed with both evidence directories enabled.

The first replay interface uses exact recorded identities/counters, without automatic rewriting of opaque protocol data. Complete raw/parsed sources remain in the bank and explicit inspection, while automatic playback omits future outcome context before matching requests. Original client graphics/resources remain outside this delivery.

The supplied Ruby sessions can be imported directly: 434 attempts, 432 complete, 2 partial, 1491 accepted native pairs. All original evidence/inventories remain read-only.

The complete Ruby bank was subsequently packaged and delivered outside Git. Its identity and reconstruction instructions are recorded in [REPLAY.md](../../../capture-runner/docs/REPLAY.md). Exact published validation and CI results are in [VERIFICATION.md](../../../capture-runner/docs/VERIFICATION.md).
