# Capture Runner Implementation Plan

> **For agentic workers:** use subagent-driven-development for independent owned modules. Root integrates and verifies the complete crate.

**Goal:** Headless native demo collectors with X games per provider per WireGuard exit, durable wire captures and unknown-event alerts.

**Architecture:** Independent Rust crate, evidence-derived adapters, append-only attempt journals, per-exit processes and per-provider game scheduling. Each isolated WireGuard namespace carries only its own traffic.

**Tech Stack:** Rust 2024, Tokio, reqwest/rustls, tokio-tungstenite, JSON configuration, Linux iproute2 and wireguard-tools.

**Spec:** `docs/superpowers/specs/2026-11-14-capture-runner-design.md`

## Global Constraints

- New code belongs in `capture-runner/`; no changes to evidence or old execution semantics.
- No required balances or financial reconciliation; no provider-wide financial holds.
- All requests/responses must be stored before parsing; partial attempts are retained.
- X is per provider/exit; game modes are sequential inside that slot.
- No GUI, no plaintext fallback from WireGuard, no historical-session replay.
- No provider traffic during tests; source credentials and WireGuard keys never enter Git.

## Review Focus

- Missing balances must not stop a known native sequence.
- Unknown protocol action must retain the received message and stop only that attempt.
- Multiple providers and exits must each obtain their configured concurrency.
- Failed WireGuard setup must not change the host default route or destroy unrelated namespaces.
- Interrupted attempts must retain evidence without silently claiming completion.

### Task 1: Capture core and scheduling (root)

Files: `src/{model,catalog,journal,context,engine,main,lib}.rs`, `tests/`.

- [x] Add meaningful offline tests for capture without balances and independent game scheduling.
- [x] Implement the transport/journal boundary and catalog importer.
- [x] Implement native registry, per-game queue, attempt results and CLI.
- [x] Verify complete crate, CLI planning and offline integration.

### Task 2: HTTP provider connectors (agent HTTP)

Files owned: `src/adapters/{hacksaw,pragmatic,peterandsons,playson,evoplay,habanero,mascot,thunderspin,spinomenal,kendoo,betsoft_live,gamesglobal}.rs`.

- [x] Reconstruct native sessions/selectors/continuations from supplied evidence.
- [x] Return protocol completion without wallet requirements.
- [x] Test unknown continuations and preserve public protocol bindings.

### Task 3: WebSocket provider connectors (agent WSS)

Files owned: `src/adapters/{endorphina,onespin4win,egt_digital,ela_games,amusnet,amigogaming,playngo,playtech,nolimit,kalamba,elk}.rs`.

- [x] Implement evidence-derived framing, bootstrap and native continuation handling.
- [x] Remove financial gates and old store coupling.
- [x] Test known/unknown protocol states offline.

### Task 4: WireGuard, configuration, supervisor (agent networking)

Files owned: `src/{config,wireguard,supervisor}.rs`, `examples/`, `docs/WIREGUARD.md`.

- [x] Test configuration and per-exit/provider overrides.
- [x] Implement owned namespace lifecycle, wg config parsing and isolated DNS/routes.
- [x] Implement exit workers with optional privilege dropping and coordinated shutdown.
- [x] Test argument construction, no hooks, and failure cleanup.

### Task 5: Remaining connectors and delivery (root / available agent)

- [x] Integrate BGaming, SmartSoft, Three Oaks, RubyPlay, NetEnt, Red Tiger, KA Gaming, Belatra, Microgaming, Yggdrasil and Iron Dog.
- [x] Audit every provider's executable modes against evidence and publish coverage.
- [x] Add Linux CI and operator documentation with exact commands.
- [x] Compile release, run tests and independent review, fix material findings, prepare a reviewable branch.

## Verified initial implementation scope

All delivered protocol families that could be reconstructed with their required bindings have been integrated. The common catalogue has 35 providers and 9,874 games; supplementary GamesGlobal controls bring it to 20,068 modes. The registry exposes 13,841 modes across 6,393 games. Missing private decoders, prepared native sources and selectors remain explicit; see capture-runner/docs/MISSING-EVIDENCE.md. Historical financial success is not a support gate. Real VPN/provider operation requires the deployment VM and peer configurations, which were not supplied.

## Follow-up: ENTREGA-CLAUDE evidence, 15 November 2026

The user supplied `ENTREGA-CLAUDE.zip` and `CLAUDE-COMPLETO.md` and asked about the remaining providers. Audit the new native captures and helper code against the exact pending mode keys. Mock servers and reconstructed responses do not establish official request contracts. The previous description of Mascot's 31 gaps was incorrect: they comprise 22 purchase modes and nine normal modes, which must be assessed separately.

- [x] Audit RubyPlay and Playtech decoder/launcher coverage from the new package.
- [x] Audit the six Claude providers and recover supported Mascot modes from actual captures.
- [x] Audit the other provider helper code for complete native controls.
- [x] Correct the missing-evidence report, provenance and generated coverage.
- [x] Verify new contracts and prepare the follow-up for the same PR.

Verified follow-up: 538 additional executable modes across seven providers, including 22 new exact IronDog catalogue pairs. The catalogue now contains 9,878 games and 20,090 modes; 14,379 modes across 6,675 games have executable contracts and 5,711 remain explicit. All 216 local tests pass, clippy/fmt/release gates pass, and the generated coverage matches the release plan. Original evidence and all 33 previous binding files are unchanged. Native provider/VPN verification remains a deployment task.
