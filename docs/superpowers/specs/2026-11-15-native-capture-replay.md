# Native capture and local replay

## Goal and authorization
The user approved implementation after reviewing new Playtech and RubyPlay evidence. Collect native demo requests/responses per provider, exit, game and mode, then export and replay them locally. Rust is the runtime; historical Python/JS is read-only reference material.

## Design
Extend the existing adapters and durable journal. Derive provider bindings from supplied evidence with provenance and explicit unsupported cases. Add a self-contained capture-bank format and an offline cursor that returns recorded inbound messages only after the expected outbound request is matched. Preserve raw bodies, frame type, ordering and partial/error attempts; never replace game data with financial summaries. A CLI export/replay interface is the initial local player, usable without contacting providers. Original game graphics require separately supplied compatible assets and are outside this change.

## Constraints
- No live provider requests or bets during implementation/testing.
- No financial reconciliation gate for completion or export.
- Fresh sessions; durable journal before outbound actions; unknown events remain partial.
- Concurrency stays per provider/exit, game modes sequential.
- Existing evidence remains read-only; no old runner modules imported at runtime.
- Protect complete native data; credentials are not a reason to discard opaque game payloads.
- Replay must distinguish exact requests and choice values; variable-field matching must be explicit and consistent.
- Preserve existing files and never silently overwrite captures or fall back from requested VPN to direct networking.

## Approach choice
Reuse the native Rust runner and add replay over its journal. Running the old Python stack would reintroduce private workspace dependencies and incompatible finance gates. A new browser game frontend would require assets not delivered.
