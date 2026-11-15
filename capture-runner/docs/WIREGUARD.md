# Linux WireGuard exit workers

Every configured exit gets its own worker process. Each worker runs every selected provider concurrently, with a separate game limit for each provider. For example, 30 exits × 3 providers × 4 games permits 360 active games. Modes of the same game remain sequential. A worker failure does not stop the other exits.

A Linux VM needs `iproute2`, `wireguard-tools`, kernel WireGuard support and root privileges for namespace creation and `ip netns exec`. No GUI is used. `runuser` from util-linux is needed only when `run_as` is configured. Use a dedicated unprivileged `capture` account for the workers; the supervisor must retain the privileges required to enter the namespaces.

## Configuration

Start with [wireguard-directory.json](../examples/wireguard-directory.json). Put one private peer configuration in `/etc/capture-runner/wireguard/<exit-id>.conf`. All regular `*.conf` files in that directory become simultaneous exits, including 30 or more files. Filenames are stable exit IDs. Files are sorted for deterministic discovery. A matching explicit exit adds overrides; conflicting explicit file paths fail validation. An empty `providers` array selects all evidence providers.

Paths in runner JSON resolve relative to that JSON file, not the working directory. `run_id`, provider IDs and exit IDs are safe ASCII path components. An explicit namespace must begin with `crwg-`. Without an explicit namespace, WireGuard exits receive a deterministic `crwg-<hash-of-exit-id>` namespace and a `cw<hash>` interface.

Concurrency precedence is:

1. `exits[].parallel_games[provider]`.
2. `providers[].parallel_games`.
3. `default_parallel_games`.

[wireguard-overrides.json](../examples/wireguard-overrides.json) runs 8 Hacksaw and 3 Pragmatic games on `exit-01`, and 6 Hacksaw and 3 Pragmatic games on every other discovered exit. These are independent limits, not a shared global quota. Defaults are one parallel game, one sample, 256 protocol steps and 30 seconds per request. All limits must be positive.

Runner JSON contains private configuration **paths**, never WireGuard keys. Only the supervisor reads that file and discovers private peer files. It creates a temporary resolved worker configuration, removes all private WireGuard paths, and grants mode `0600` access to the `run_as` account. Workers do not enumerate the private directory. Evidence and executable directories must be readable/traversable by the worker account; give that account write access to `data_root`. The private peer directory may remain root-owned mode `0700`, and each `.conf` mode `0600`. An operator can prepare a dedicated account, then use:

```sh
sudo install -d -m 0700 /etc/capture-runner/wireguard
sudo install -d -o capture -g capture -m 0750 /var/lib/capture-runner/captures
sudo chmod 0600 /etc/capture-runner/wireguard/*.conf
```

Use actual locally provisioned peer files. Do not commit them or paste keys into command arguments.

## Supported peer configuration

The parser accepts `[Interface]` with `PrivateKey`, `Address`, `DNS`, `MTU`, and `ListenPort`, plus `[Peer]` sections with `PublicKey`, optional `PresharedKey`, `Endpoint`, `AllowedIPs` and optional `PersistentKeepalive`.

`Address` and `DNS` may contain comma-separated IPv4 and IPv6 values or repeated directives. Addresses require CIDR prefixes. DNS entries must be literal reachable IP addresses: a host stub such as `127.0.0.53` cannot serve this namespace. Every configured address family requires full-tunnel `AllowedIPs`, respectively `0.0.0.0/0` and `::/0`. An IPv4-only exit has no IPv6 default route. A DNS family also requires an address and default route for that family. Default MTU is 1420; set an explicit MTU between 1280 and 9000 if the underlay needs it.

Client listen ports are always ephemeral (`ListenPort = 0`), even when exported peer files contain a fixed `ListenPort`. All encrypted UDP sockets originate in the host namespace, so this avoids port collisions when many exits run together. This client-exit runner does not preserve fixed inbound listen-port requirements.

Shell hooks (`PreUp`, `PostUp`, `PreDown`, `PostDown`), `SaveConfig`, `Table`, `FwMark`, unknown directives and malformed files fail validation. No shell or `wg-quick` is invoked. Each peer requires an endpoint, public key and allowed IPs. Endpoint hostnames are resolved during native WireGuard configuration in the host namespace before the interface moves; capture traffic and resolver requests originate inside the isolated namespace.

## Operator workflow

Commands below are examples for the operator. Tests and builds do not execute them.

```sh
capture-runner wg plan --config /opt/capture-runner/runner.json
sudo capture-runner wg up --config /opt/capture-runner/runner.json
sudo capture-runner wg status --config /opt/capture-runner/runner.json
sudo capture-runner run --config /opt/capture-runner/runner.json
sudo capture-runner wg down --config /opt/capture-runner/runner.json
```

`wg plan` reads and validates peer files and prints commands with a placeholder for the private native config. It does not create files, namespaces, interfaces or routes. It must run under an account allowed to read the private peer files. `wg status` checks local setup and ownership only; it does not probe a remote peer or prove a handshake. The general `plan` command reports evidence and executable protocol coverage without loading keys.

`wg up` validates every peer file before mutation, then sets up each exit independently. Errors identify unsuccessful exits; previously successful exits remain available. `run` does not invoke `up`, modify routing or repair tunnels automatically. Missing namespaces fail only their own exit; there is no direct fallback. A missing remote peer will result in transport timeouts inside that exit.

The supervisor invokes the same binary as `ip netns exec <namespace> runuser -u <account> -- <binary> worker --config <resolved-private-temp-file> --exit <id>`. Without `run_as` the worker inherits the supervisor user. The worker verifies `/proc/self/ns/net` against the named namespace's device and inode, so invoking its hidden worker command from the host fails for namespace exits.

Ctrl-C or SIGTERM signals worker process groups, allows five seconds for SIGINT cleanup, then two seconds for SIGTERM cleanup, then forces SIGKILL and reaps workers. An exited leader remains unreaped until its descendants are terminated, then its group ID is retired immediately; completed exits are never signalled later. Namespaces remain provisioned after the run; the explicit `wg down` operation removes them after workers stop. `down` refuses namespaces containing active processes.

Direct networking requires an explicit exit with neither `namespace` nor `wireguard_config`; see [direct-explicit.json](../examples/direct-explicit.json). Merely omitting exits is invalid. For an explicit WireGuard override, include its `wireguard_config` path, so a missing file cannot be mistaken for an intentionally direct exit. Namespace-only exits use an explicitly supplied `crwg-` namespace that the operator provisions; this runner never creates or deletes those namespaces.

## Isolation and ownership

The implementation follows WireGuard's [network namespace integration](https://www.wireguard.com/netns/) and [quick start](https://www.wireguard.com/quickstart/). An interface originates in the host, receives its native configuration there, then moves into a fresh network namespace. Its encrypted UDP socket stays in the original namespace. The worker's namespace contains only loopback and its WireGuard interface; no veth or physical interface supplies another route. Host default routes and firewall rules are unchanged. Tunnel failure leaves no plaintext path to the host network.

Native keys are passed to `wg setconf` through a temporary regular file with mode `0600`. Its pathname, never its contents, is the argument, and it is removed when setup returns or rolls back. Child command output is withheld from errors because `wg` diagnostics can contain configuration material. The sanitized plan, ownership records and logs omit private keys. An uncatchable process crash can leave a private temporary file until the host cleans its temporary directory; that file remains mode `0600`.

Ownership records live in root-owned mode `0700` `/run/capture-runner`, with mode `0600` records and a nonblocking operation lock. Namespace device/inode and interface index are checked before removal. Records also retain a SHA-256 fingerprint of the effective native configuration, addresses, DNS and MTU. Readiness rejects a changed peer configuration until the operator runs `wg down` followed by `wg up`; `down` remains possible after an edit. Existing namespaces, interfaces, DNS directories and records are never adopted. Setup failure rolls back only resources created by that operation; if identity cannot be proved or cleanup fails, records remain for inspection rather than deleting a resource by name alone. A crash between kernel mutation and recording its identity may require manual inspection. Do not manually replace owned resources or remove ownership records during a run.

The per-namespace `/etc/netns/<owned-namespace>` directory is root-owned mode `0700` and has a private ownership token. Its `resolv.conf` lists only the configured tunnel resolvers. The resolver file itself is mode `0644`, allowing the unprivileged worker to read the bind-mounted `/etc/resolv.conf` that `ip netns exec` prepares before `runuser`; the private parent directory prevents account traversal of the source path. Removal checks the token and does not recursively remove arbitrary contents.

## Offline verification limits

Unit tests use typed mock commands and temporary directories. They verify 30-file discovery, concurrency precedence, parser rejection, route families, host-before-move order, key redaction, temporary-file permissions/removal, ownership refusal, rollback and child-process failure isolation/termination. They never invoke a real networking mutation, use a real peer key, contact a provider, or establish a tunnel. A Linux operator must verify real handshakes, DNS reachability and endpoint egress with supplied peers and the deployment's kernel privileges before production capture.
