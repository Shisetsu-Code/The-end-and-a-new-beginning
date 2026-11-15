# Project purpose

This is a capture runner. Its output is native requests, responses and events grouped by provider, exit, game and mode. Missing balances or wallet differences MUST NOT block collection, completion based on the native protocol, export, or another game. Do not reintroduce financial certification requirements.

Concurrency is configured per (provider, exit). Do not restore the old global one-game-per-provider lock. Each game's modes are sequential within its own lane slot. Use fresh sessions; record partial attempts and unknown events, never fabricate a next action.

The existing evidence is read-only source material. Do not import old runner modules or use its executables at runtime. Keep dependencies, architecture and operational controls small and explicit. Tests do not contact providers.
