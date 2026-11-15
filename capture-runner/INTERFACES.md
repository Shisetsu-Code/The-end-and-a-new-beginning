# Implementation interfaces

Root owns Cargo, model, context, journal, catalog, engine, main/lib, adapters/mod. Agents own only their assigned adapter/network files. Do not commit until root coordinates integration. `cargo` is `/home/agent/.cargo/bin/cargo`.

## Provider connector interface

Every module exports:

```rust
pub fn supports(game: &crate::model::Game, mode: &crate::model::Mode) -> bool;
pub async fn execute(ctx: &crate::context::RunContext, game: &crate::model::Game,
    mode: &crate::model::Mode, settings: &serde_json::Value)
    -> anyhow::Result<crate::model::Completion>;
```

`Completion { pub native_round_id: Option<String>, pub details: Value }` means a recognized native end state. `Completion::new(id: Option<String>)` initializes empty details. No balance, cost, payout, checks or currency fields are mandatory. Remove audit-only parsing and any wallet-derived acceptance criteria from protocol code. Preserve the amounts/selectors REQUIRED to construct requests.

`Game { id: String, name: String, modes: Vec<Mode> }`; `Mode { key: String, kind: ModeKind, label: String, requires_prerequisites: Value }`. ModeKind has Normal, Purchase, AnteBet, Booster, Variant, Special (same serde names as source inventory). `model::{decimal,exact_add,exact_sub,exact_mul,exact_div100,sha256}` are available for protocol encoding only. No wallet_matches helper exists.

`model::unknown_event(message: impl Into<String>) -> anyhow::Error` returns a typed protocol novelty. Use it at unknown continuation/state/type boundaries. Raw messages are already persisted by context. Use ordinary anyhow errors for an unavailable requested mode or provider rejection. Source version mismatch affecting serialization is an unknown protocol version, not a financial failure.

## Context

Context uses a private per-attempt journal and a fresh cookie jar. Public methods mirror transport primitives from the evidence, WITHOUT Store or RoundStatus:

```rust
pub fn max_steps(&self) -> usize;
pub fn user_agent(&self) -> &'static str;
pub fn save_session(&self, owner: &Value) -> anyhow::Result<()>;
pub fn note(&self, event: &str, data: &Value) -> anyhow::Result<()>;
pub fn seed_cookie(&self, cookie: &str, url: &str) -> anyhow::Result<()>;
pub async fn get(&self, label:&str, url:&str) -> Result<WireResponse>;
pub async fn http(&self, label:&str, method:&str, url:&str,
 headers:BTreeMap<String,String>, body:String, round_action:bool) -> Result<WireResponse>;
pub async fn json(&self, label:&str, url:&str, body:&Value,
 headers:BTreeMap<String,String>, round_action:bool) -> Result<Value>;
pub async fn websocket(&self,url:&str,origin:&str,subprotocol:Option<&str>,
 headers:BTreeMap<String,String>) -> Result<NativeSocket>;
pub async fn ws_json(&self,socket:&mut NativeSocket,label:&str,request:&Value,
 round_action:bool,request_id:Option<&str>) -> Result<Value>;
pub async fn ws_text_decoded(&self,socket:&mut NativeSocket,label:&str,payload:&str,
 round_action:bool,request_id:Option<&str>,decode:fn(&str)->Result<Value>) -> Result<Value>;
pub async fn ws_codec<F>(&self,socket:&mut NativeSocket,label:&str,payload:SocketPayload,
 round_action:bool,request_id:Option<&str>,decode:F) -> Result<Value>
 where F: FnMut(&SocketPayload)->Result<Vec<Value>> + Send;
pub async fn ws_codec_controlled<F>(&self,socket:&mut NativeSocket,label:&str,payload:SocketPayload,
 round_action:bool,request_id:Option<&str>,decode:F) -> Result<Value>
 where F: FnMut(&SocketPayload)->Result<DecodedFrame> + Send;
pub async fn ws_exchange<F>(&self,socket:&mut NativeSocket,label:&str,payload:Option<SocketPayload>,
 round_action:bool,request_id:Option<&str>,decode:F) -> Result<Value>
 where F: FnMut(&SocketPayload)->Result<DecodedFrame> + Send;
pub async fn ws_expect_text(&self,socket:&mut NativeSocket,label:&str,expected:&str) -> Result<()>;
```

`WireResponse {status:u16,headers:BTreeMap<String,String>,body:String,set_cookies:Vec<String>}`.
`SocketPayload::Text(String)|Binary(Vec<u8>)`; `DecodedFrame {values:Vec<Value>,replies:Vec<SocketPayload>}`.
`context::validate_target(provider:&str,value:&str)->Result<url::Url>` is available for demo endpoint validation.

Public bindings under `capture-runner/bindings/` preserve the original relative `../../bindings/name.json` from adapter modules. Do not embed private session values. Old module tests referring to Store/native financial receipts must be replaced with relevant protocol tests, not copied verbatim.

## Configuration and supervisor (network agent)

Use JSON configuration. Export `Config`, `ProviderConfig`, `ExitConfig` with fields:

```rust
Config { evidence_root:PathBuf, data_root:PathBuf, run_id:String,
 default_parallel_games:usize, samples_per_mode:u32, max_steps:usize,
 timeout_seconds:u64, providers:Vec<ProviderConfig>, exits:Vec<ExitConfig>,
 wireguard_dir:Option<PathBuf>, run_as:Option<String>,
 games:Vec<String>, modes:Vec<String> }
ProviderConfig { id:String, parallel_games:Option<usize> }
ExitConfig { id:String, namespace:Option<String>, wireguard_config:Option<PathBuf>,
 parallel_games:BTreeMap<String,usize> }
```

`Config::load(path:&Path)->Result<Self>` resolves relative paths against the config location, discovers *.conf files in wireguard_dir as exits, validates IDs and strictly positive limits, requires explicit exits or directory (no automatic direct fallback). Empty providers means all evidence providers. `Config::parallel_games(&self, exit:&ExitConfig, provider:&str)->usize` uses exit override > provider override > default. `ExitConfig::namespace_name(&self)->Option<String>` can return a deterministic owned prefix for wireguard configs.

`supervisor::run(config_path:&Path, config:&Config)->Result<bool>` spawns the SAME binary with `worker --config <absolute path> --exit <id>` once per exit. For namespace exits use `ip netns exec <namespace>` and optionally `runuser -u <run_as> --`. Child failure must not cancel unrelated exits. Graceful Ctrl-C then bounded forced termination; no fallback to direct.

`wireguard::{plan,up,down,status}(config:&Config)->Result<Value>` for CLI. `plan` is read-only and omits private keys. `up` creates isolated owned namespaces using official WireGuard namespace routing. `down` never removes an unrelated namespace. Test generation and parser behavior without needing real peers. Keep mutation helpers testable through typed commands rather than shell strings.
