# Native capture runners with WireGuard exits

## User requirements

- Build a new Rust runner from the published protocol evidence, not the old runner's financial-validation architecture.
- No GUI. Target a Linux virtual machine.
- Every WireGuard exit runs every selected provider concurrently. The limit is X active games for EACH provider/exit pair, not X globally or per provider across all exits.
- Modes of an individual game run sequentially within that lane. Different games, providers and exits run concurrently.
- Collect native requests, responses and continuations for identified modes. Persist partial attempts too. Money and wallet reconciliation are not acceptance criteria.
- Emit actionable alerts for unknown protocol events; never invent an unrecognized next action.
- Preserve all existing evidence and keep it as the source of protocol knowledge.

## Implementation decisions

The independent crate is `capture-runner/`. The existing crates are reference material only; the new crate has no dependency on them. Public bindings are copied with provenance. Provider protocol code may be reconstructed from the supplied reference implementations, removing financial audit gates and provider-wide permanent holds.

One worker process runs inside each exit's Linux network namespace. Its asynchronous scheduler has an independent game semaphore for each provider. A game owns its sequence of modes; each attempt creates a fresh demo session and an append-only journal. No provider-wide financial locks. Native closure is distinct from monetary correctness.

WireGuard interfaces originate in the host namespace and move into isolated namespaces with only loopback and WireGuard. This follows https://www.wireguard.com/netns/. DNS is configured per namespace. Tunnel failure has no direct-network fallback. Only explicitly named owned namespaces are managed; shell hooks from wg-quick files are not executed. Configuration contains paths to private WireGuard files, never copied keys.

Output: `<data>/<run>/<exit>/<provider>/<game>/<mode>/<sample>/attempt-<uuid>/wire.jsonl` and `result.json`. Results distinguish complete, unknown_event, transport_error, provider_error, unsupported and interrupted. Captures never disappear because accounting fields are missing. Balances remain in raw responses without mandatory normalization.

Evidence inventories are enumerated independently of old `ready`, `campaign_eligible`, receipt status and frozen inventory hashes. Actual executable support is reported separately for each provider/game/mode. Missing protocol implementation is visible; it is not claimed as successful collection.

## Verification

Offline fixtures must exercise complete rounds without balances, unknown continuation capture, request/response preservation, and failure isolation. Scheduler tests cover multiple exits and providers with different X values. WireGuard tests inspect generated commands/configuration and reject unsafe configuration; real tunnel establishment requires supplied peer configurations and kernel privileges.

No live provider traffic is triggered by tests or CI. No live WireGuard peers are supplied in this session.

## Approaches

The prior proposal was to refactor the old runner. The user chose a clean new runner using the collected evidence. A new small capture core with evidence-derived provider connectors implements that choice; wrapping the old executables would retain the accounting gates and is therefore excluded.
