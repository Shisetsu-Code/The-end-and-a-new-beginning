# Project purpose

Start with the repository [AGENTS.md](../AGENTS.md) and [current handoff](../docs/HANDOFF.md). This file adds crate-specific rules; historical agent assignments and plans are not active tasks.

This is a capture runner. Its output is native requests, responses and events grouped by provider, exit, game and mode. Missing balances or wallet differences MUST NOT block collection, completion based on the native protocol, export, or another game. Do not reintroduce financial certification requirements.

Concurrency is configured per (provider, exit). Do not restore the old global one-game-per-provider lock. Each game's modes are sequential within its own lane slot. Use fresh sessions for new collection; record partial attempts and unknown events, never fabricate a next action. Historical imports preserve their recorded state and session context instead of inventing a fresh initialization per round.

The existing evidence is read-only source material. Do not import old runner modules or use its executables at runtime. Keep dependencies, architecture and operational controls small and explicit. Tests do not contact providers.

Export/replay preserves complete and partial transcripts. A mismatched request must not advance the cursor or disclose the following result. Keep original opaque payloads and distinguish parsed native JSON imports from raw HTTP captures. Do not claim visual emulation, token rewriting, arbitrary new rounds or universal binary decoding from the existing replay interface.
