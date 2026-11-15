//! Linux namespace management. No shell, wg-quick hooks, or implicit direct egress.
use crate::config::{Config, ExitConfig, short_hash, validate_namespace};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
#[cfg(unix)]
use std::os::unix::{
    fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    io::AsRawFd,
};
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    net::IpAddr,
    path::{Path, PathBuf},
    process::Command,
};

struct TunnelConfig {
    native: String,
    addresses: Vec<String>,
    dns: Vec<IpAddr>,
    mtu: u16,
    ipv4: bool,
    ipv6: bool,
}
impl TunnelConfig {
    fn fingerprint(&self) -> String {
        let material =
            serde_json::to_vec(&json!([self.native, self.addresses, self.dns, self.mtu]))
                .expect("serializable tunnel config");
        Sha256::digest(&material)
            .iter()
            .map(|v| format!("{v:02x}"))
            .collect()
    }
    fn parse(text: &str) -> Result<Self> {
        let mut native = String::new();
        let mut addresses = Vec::new();
        let mut dns = Vec::new();
        let mut mtu = None;
        let mut section = "";
        let mut interface = false;
        let mut private = false;
        let mut peer_fields = std::collections::BTreeSet::new();
        let mut interface_fields = std::collections::BTreeSet::new();
        let mut peers = 0usize;
        let mut default4 = false;
        let mut default6 = false;
        for raw in text.lines() {
            let line = raw.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            if line.starts_with('[') {
                if section == "Peer" {
                    validate_peer(&peer_fields)?;
                }
                match line {
                    "[Interface]" if !interface && peers == 0 => {
                        interface = true;
                        section = "Interface";
                    }
                    "[Peer]" if interface => {
                        section = "Peer";
                        peers += 1;
                        peer_fields.clear();
                    }
                    _ => bail!("invalid or duplicate WireGuard section"),
                }
                native.push_str(line);
                native.push('\n');
                continue;
            }
            let (key, value) = line
                .split_once('=')
                .context("WireGuard directive needs an equals sign")?;
            let (key, value) = (key.trim(), value.trim());
            if value.is_empty() || value.chars().any(|c| c.is_control()) {
                bail!("empty or control-character WireGuard directive");
            }
            if section == "Interface" {
                if !matches!(key, "Address" | "DNS") && !interface_fields.insert(key.to_owned()) {
                    bail!("duplicate WireGuard interface directive");
                }
                match key {
                    "Address" => {
                        for value in value.split(',').map(str::trim) {
                            parse_cidr(value)?;
                            addresses.push(value.to_owned());
                        }
                    }
                    "DNS" => {
                        for value in value.split(',').map(str::trim) {
                            let ip: IpAddr = value.parse().context(
                                "DNS must contain literal IP addresses, never a host resolver stub",
                            )?;
                            if ip.is_loopback() || ip.is_unspecified() || ip.is_multicast() {
                                bail!("DNS must be a tunnel-reachable unicast address");
                            }
                            dns.push(ip);
                        }
                    }
                    "MTU" => {
                        let n: u16 = value.parse().context("invalid WireGuard MTU")?;
                        if !(1280..=9000).contains(&n) {
                            bail!("MTU must be between 1280 and 9000");
                        }
                        mtu = Some(n);
                    }
                    "PrivateKey" => {
                        validate_key(value)?;
                        private = true;
                        append_native(&mut native, key, value);
                    }
                    "ListenPort" => {
                        let _: u16 = value.parse().context("invalid ListenPort")?;
                        append_native(&mut native, key, "0");
                    }
                    "FwMark" => {
                        bail!("FwMark is unnecessary in an isolated namespace and unsupported")
                    }
                    "PreUp" | "PostUp" | "PreDown" | "PostDown" => {
                        bail!("wg-quick shell hooks are forbidden")
                    }
                    _ => bail!("unsupported WireGuard interface directive"),
                }
            } else if section == "Peer" {
                if !peer_fields.insert(key.to_owned()) {
                    bail!("duplicate WireGuard peer directive");
                }
                match key {
                    "PublicKey" | "PresharedKey" => validate_key(value)?,
                    "Endpoint" => validate_endpoint(value)?,
                    "AllowedIPs" => {
                        for value in value.split(',').map(str::trim) {
                            let (ip, prefix) = parse_cidr(value)?;
                            if prefix == 0 {
                                if ip.is_ipv4() {
                                    if default4 {
                                        bail!("multiple peers claim IPv4 default");
                                    }
                                    default4 = true;
                                } else {
                                    if default6 {
                                        bail!("multiple peers claim IPv6 default");
                                    }
                                    default6 = true;
                                }
                            }
                        }
                    }
                    "PersistentKeepalive" => {
                        let _: u16 = value.parse().context("invalid PersistentKeepalive")?;
                    }
                    _ => bail!("unsupported WireGuard peer directive"),
                }
                append_native(&mut native, key, value);
            } else {
                bail!("WireGuard directives must be inside a section");
            }
        }
        if section == "Peer" {
            validate_peer(&peer_fields)?;
        }
        if !private || peers == 0 || addresses.is_empty() || dns.is_empty() {
            bail!(
                "WireGuard exit requires PrivateKey, Address, DNS and at least one complete peer"
            );
        }
        let ipv4 = addresses
            .iter()
            .any(|a| parse_cidr(a).is_ok_and(|(ip, _)| ip.is_ipv4()));
        let ipv6 = addresses
            .iter()
            .any(|a| parse_cidr(a).is_ok_and(|(ip, _)| ip.is_ipv6()));
        if (ipv4 && !default4) || (ipv6 && !default6) {
            bail!(
                "every configured address family requires full-tunnel AllowedIPs (0.0.0.0/0 or ::/0)"
            );
        }
        if dns
            .iter()
            .any(|ip| (ip.is_ipv4() && !ipv4) || (ip.is_ipv6() && !ipv6))
        {
            bail!("DNS family needs a configured tunnel address and default route");
        }
        Ok(Self {
            native,
            addresses,
            dns,
            mtu: mtu.unwrap_or(1420),
            ipv4,
            ipv6,
        })
    }
}
fn append_native(output: &mut String, key: &str, value: &str) {
    output.push_str(key);
    output.push_str(" = ");
    output.push_str(value);
    output.push('\n');
}
fn parse_cidr(value: &str) -> Result<(IpAddr, u8)> {
    let (ip, prefix) = value
        .split_once('/')
        .context("WireGuard address requires CIDR prefix")?;
    let ip: IpAddr = ip.parse().context("invalid WireGuard IP address")?;
    let prefix: u8 = prefix.parse().context("invalid WireGuard CIDR prefix")?;
    if prefix > if ip.is_ipv4() { 32 } else { 128 } {
        bail!("WireGuard CIDR prefix is out of range");
    }
    Ok((ip, prefix))
}
fn validate_key(value: &str) -> Result<()> {
    if value.len() != 44
        || !value.ends_with('=')
        || !value[..43]
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"+/".contains(&c))
    {
        bail!("WireGuard key must be a base64-encoded 32-byte key");
    }
    Ok(())
}
fn validate_peer(fields: &std::collections::BTreeSet<String>) -> Result<()> {
    if !["PublicKey", "AllowedIPs", "Endpoint"]
        .iter()
        .all(|key| fields.contains(*key))
    {
        bail!("each WireGuard peer needs PublicKey, AllowedIPs and Endpoint");
    }
    Ok(())
}
fn validate_endpoint(value: &str) -> Result<()> {
    let (host, port) = value
        .rsplit_once(':')
        .context("WireGuard Endpoint requires host:port")?;
    let port: u16 = port.parse().context("invalid endpoint port")?;
    let valid_host = if host.starts_with('[') && host.ends_with(']') {
        host[1..host.len() - 1]
            .parse::<std::net::Ipv6Addr>()
            .is_ok()
    } else {
        !host.is_empty()
            && host.len() <= 253
            && host
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b".-".contains(&c))
    };
    if port == 0 || !valid_host {
        bail!("invalid endpoint host or port");
    }
    Ok(())
}
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
struct CommandSpec {
    program: String,
    args: Vec<String>,
}
impl CommandSpec {
    fn new(program: &str, args: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            program: program.into(),
            args: args.into_iter().map(Into::into).collect(),
        }
    }
}
trait Runner {
    fn execute(&mut self, command: &CommandSpec) -> Result<String>;
}
struct SystemRunner;
impl Runner for SystemRunner {
    fn execute(&mut self, command: &CommandSpec) -> Result<String> {
        let output = Command::new(&command.program)
            .args(&command.args)
            .env_remove("WG_ENDPOINT_RESOLUTION_RETRIES")
            .output()
            .with_context(|| {
                format!(
                    "execute {} (install iproute2 and wireguard-tools)",
                    command.program
                )
            })?;
        // wg error text can contain key material. Never include child stderr or stdout in errors.
        if !output.status.success() {
            bail!(
                "{} command failed with {} (child output withheld)",
                command.program,
                output.status
            );
        }
        String::from_utf8(output.stdout).context("command returned non-UTF-8 output")
    }
}
fn interface_name(e: &ExitConfig) -> String {
    format!("cw{}", short_hash(&e.id))
}
fn steps(e: &ExitConfig, c: &TunnelConfig, secret: &Path) -> Vec<CommandSpec> {
    let ns = e.namespace_name().expect("WireGuard exit has a namespace");
    let iface = interface_name(e);
    let mut out = vec![
        CommandSpec::new("ip", ["netns", "add", &ns]),
        CommandSpec::new("ip", ["link", "add", &iface, "type", "wireguard"]),
        CommandSpec::new("wg", ["setconf", &iface, &secret.to_string_lossy()]),
        CommandSpec::new("ip", ["link", "set", &iface, "netns", &ns]),
        CommandSpec::new("ip", ["-n", &ns, "link", "set", "lo", "up"]),
    ];
    for addr in &c.addresses {
        out.push(CommandSpec::new(
            "ip",
            ["-n", &ns, "address", "add", addr, "dev", &iface],
        ));
    }
    out.push(CommandSpec::new(
        "ip",
        [
            "-n",
            &ns,
            "link",
            "set",
            "dev",
            &iface,
            "mtu",
            &c.mtu.to_string(),
            "up",
        ],
    ));
    if c.ipv4 {
        out.push(CommandSpec::new(
            "ip",
            ["-n", &ns, "-4", "route", "add", "default", "dev", &iface],
        ));
    }
    if c.ipv6 {
        out.push(CommandSpec::new(
            "ip",
            ["-n", &ns, "-6", "route", "add", "default", "dev", &iface],
        ));
    }
    out
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
struct Identity {
    dev: u64,
    ino: u64,
}
fn identity(path: &Path) -> Result<Identity> {
    let m = fs::metadata(path)?;
    Ok(Identity {
        dev: m.dev(),
        ino: m.ino(),
    })
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Ownership {
    version: u32,
    config_fingerprint: String,
    exit_id: String,
    namespace: String,
    interface: String,
    token: String,
    namespace_identity: Option<Identity>,
    interface_index: Option<u64>,
    namespace_created: bool,
    interface_created: bool,
    moved: bool,
    dns_created: bool,
}
struct Manager<'a, R: Runner> {
    state_root: PathBuf,
    dns_root: PathBuf,
    netns_root: PathBuf,
    runner: &'a mut R,
}
impl<'a, R: Runner> Manager<'a, R> {
    fn system(runner: &'a mut R) -> Self {
        Self {
            state_root: "/run/capture-runner".into(),
            dns_root: "/etc/netns".into(),
            netns_root: "/var/run/netns".into(),
            runner,
        }
    }
    fn record_path(&self, ns: &str) -> PathBuf {
        self.state_root.join(format!("{ns}.json"))
    }
    fn read_record(&self, ns: &str) -> Result<Ownership> {
        validate_namespace(ns)?;
        check_directory(&self.state_root, true)?;
        let file = private_file(&self.record_path(ns))?;
        let r: Ownership = serde_json::from_reader(file).context("read ownership record")?;
        if r.version != 1 || r.namespace != ns {
            bail!("invalid namespace ownership record");
        }
        crate::config::validate_id(&r.exit_id, "owned exit")?;
        if r.interface != format!("cw{}", short_hash(&r.exit_id)) {
            bail!("invalid owned interface name");
        }
        if r.token.is_empty()
            || !r
                .token
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        {
            bail!("invalid ownership token");
        }
        Ok(r)
    }
    fn save_record(&self, r: &Ownership) -> Result<()> {
        let mut tmp = tempfile::NamedTempFile::new_in(&self.state_root)?;
        tmp.as_file()
            .set_permissions(fs::Permissions::from_mode(0o600))?;
        serde_json::to_writer(tmp.as_file_mut(), r)?;
        tmp.as_file().sync_all()?;
        tmp.persist(self.record_path(&r.namespace))
            .map_err(|_| anyhow::anyhow!("persist ownership record"))?;
        Ok(())
    }
    fn prepare(&self) -> Result<File> {
        ensure_directory(&self.state_root, 0o700)?;
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(self.state_root.join("lock"))?;
        // Lock scope covers all exits, so concurrent up/down invocations cannot race ownership.
        if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            bail!("another capture-runner network operation holds the lock");
        }
        ensure_directory(&self.dns_root, 0o755)?;
        Ok(lock)
    }
    fn up_one(&mut self, e: &ExitConfig, c: &TunnelConfig) -> Result<()> {
        let ns = e.namespace_name().context("WireGuard namespace missing")?;
        validate_namespace(&ns)?;
        if self.record_path(&ns).exists()
            || self.netns_root.join(&ns).exists()
            || self.dns_root.join(&ns).exists()
        {
            bail!(
                "namespace, DNS directory or ownership record already exists; inspect status before down/up"
            );
        }
        let iface = interface_name(e);
        let links: Value = serde_json::from_str(
            &self
                .runner
                .execute(&CommandSpec::new("ip", ["-j", "link", "show"]))?,
        )?;
        if links
            .as_array()
            .context("ip link output is not an array")?
            .iter()
            .any(|l| l["ifname"] == iface)
        {
            bail!("host interface already exists and is not owned by this operation");
        }
        let token = format!(
            "{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos()
        );
        let mut r = Ownership {
            version: 1,
            config_fingerprint: c.fingerprint(),
            exit_id: e.id.clone(),
            namespace: ns.clone(),
            interface: iface,
            token,
            namespace_identity: None,
            interface_index: None,
            namespace_created: false,
            interface_created: false,
            moved: false,
            dns_created: false,
        };
        self.save_record(&r)?;
        let result = (|| -> Result<()> {
            let mut secret = tempfile::NamedTempFile::new()?;
            secret
                .as_file()
                .set_permissions(fs::Permissions::from_mode(0o600))?;
            secret.write_all(c.native.as_bytes())?;
            secret.flush()?;
            let commands = steps(e, c, secret.path());
            for (index, command) in commands.iter().enumerate() {
                self.runner.execute(command)?;
                match index {
                    0 => {
                        r.namespace_created = true;
                        self.save_record(&r)?;
                        r.namespace_identity = Some(identity(&self.netns_root.join(&ns))?);
                        self.save_record(&r)?;
                    }
                    1 => {
                        r.interface_created = true;
                        self.save_record(&r)?;
                        let links: Value = serde_json::from_str(&self.runner.execute(
                            &CommandSpec::new("ip", ["-j", "link", "show", "dev", &r.interface]),
                        )?)?;
                        r.interface_index = Some(
                            links[0]["ifindex"]
                                .as_u64()
                                .context("missing new interface index")?,
                        );
                        self.save_record(&r)?;
                    }
                    3 => {
                        r.moved = true;
                        self.save_record(&r)?;
                    }
                    _ => {}
                }
            }
            let dns_dir = self.dns_root.join(&ns);
            fs::create_dir(&dns_dir)?;
            fs::set_permissions(&dns_dir, fs::Permissions::from_mode(0o700))?;
            r.dns_created = true;
            self.save_record(&r)?;
            // Persist the ownership token before resolver contents; cleanup never trusts names alone.
            let mut marker = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(dns_dir.join(".capture-owner"))?;
            marker.write_all(r.token.as_bytes())?;
            marker.sync_all()?;
            r.dns_created = true;
            self.save_record(&r)?;
            let mut resolv = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o644)
                .open(dns_dir.join("resolv.conf"))?;
            for dns in &c.dns {
                writeln!(resolv, "nameserver {dns}")?;
            }
            writeln!(resolv, "options timeout:2 attempts:2")?;
            resolv.sync_all()?;
            Ok(())
        })();
        if let Err(error) = result {
            let cleanup = self.cleanup(&r);
            return match cleanup{Ok(())=>Err(error.context("WireGuard setup failed; owned resources rolled back")),Err(_)=>Err(error.context("WireGuard setup failed; rollback incomplete, inspect ownership record before retrying"))};
        }
        Ok(())
    }
    fn cleanup(&mut self, r: &Ownership) -> Result<()> {
        let ns_path = self.netns_root.join(&r.namespace);
        if r.namespace_created && r.namespace_identity.is_none() {
            bail!("namespace created but identity unavailable; refusing unverified rollback");
        }
        if r.interface_created && r.interface_index.is_none() {
            bail!("interface created but identity unavailable; refusing unverified rollback");
        }
        if let Some(expected) = &r.namespace_identity
            && ns_path.exists()
        {
            if &identity(&ns_path)? != expected {
                bail!("namespace identity changed; refusing removal");
            }
            let pids = self
                .runner
                .execute(&CommandSpec::new("ip", ["netns", "pids", &r.namespace]))?;
            if !pids.trim().is_empty() {
                bail!("namespace has running processes; stop workers before down");
            }
            // Delete the owned interface explicitly: named netns deletion alone does not
            // guarantee interface destruction while a process still has namespace handles.
            if r.moved {
                let links: Value = serde_json::from_str(&self.runner.execute(
                    &CommandSpec::new("ip", ["-j", "-n", &r.namespace, "link", "show"]),
                )?)?;
                if let Some(link) = links
                    .as_array()
                    .context("invalid namespace links")?
                    .iter()
                    .find(|l| l["ifname"] == r.interface)
                {
                    if link["ifindex"].as_u64() != r.interface_index {
                        bail!("owned interface identity changed; refusing removal");
                    }
                    self.runner.execute(&CommandSpec::new(
                        "ip",
                        ["-n", &r.namespace, "link", "del", &r.interface],
                    ))?;
                }
            }
            self.runner
                .execute(&CommandSpec::new("ip", ["netns", "del", &r.namespace]))?;
        }
        if !r.moved
            && let Some(index) = r.interface_index
        {
            let links: Value = serde_json::from_str(
                &self
                    .runner
                    .execute(&CommandSpec::new("ip", ["-j", "link", "show"]))?,
            )?;
            if let Some(link) = links
                .as_array()
                .context("invalid host links")?
                .iter()
                .find(|l| l["ifname"] == r.interface)
            {
                if link["ifindex"].as_u64() != Some(index) {
                    bail!("host interface identity changed; refusing removal");
                }
                self.runner
                    .execute(&CommandSpec::new("ip", ["link", "del", &r.interface]))?;
            }
        }
        if r.dns_created {
            let dir = self.dns_root.join(&r.namespace);
            check_directory(&dir, true)?;
            let token = fs::read_to_string(dir.join(".capture-owner"))?;
            if token != r.token {
                bail!("resolver ownership token changed; refusing removal");
            }
            let resolv = dir.join("resolv.conf");
            if resolv.exists() {
                if fs::symlink_metadata(&resolv)?.file_type().is_symlink() {
                    bail!("resolver file replaced by symlink");
                }
                fs::remove_file(resolv)?;
            }
            fs::remove_file(dir.join(".capture-owner"))?;
            fs::remove_dir(dir)?;
        }
        fs::remove_file(self.record_path(&r.namespace))?;
        Ok(())
    }
    fn ready(&mut self, e: &ExitConfig) -> Result<()> {
        let Some(ns) = e.namespace_name() else {
            return Ok(());
        };
        validate_namespace(&ns)?;
        let actual = identity(&self.netns_root.join(&ns))
            .context("namespace not available; operator must run wireguard up first")?;
        if e.wireguard_config.is_some() {
            let r = self.read_record(&ns)?;
            if r.config_fingerprint != read_tunnel(e)?.fingerprint() {
                bail!(
                    "WireGuard configuration changed since setup; run down then up before capture"
                );
            }
            if r.exit_id != e.id
                || r.namespace_identity.as_ref() != Some(&actual)
                || !r.moved
                || !r.dns_created
            {
                bail!("namespace ownership or setup is incomplete");
            }
            let links: Value = serde_json::from_str(&self.runner.execute(&CommandSpec::new(
                "ip",
                ["-j", "-d", "-n", &ns, "link", "show"],
            ))?)?;
            let links = links
                .as_array()
                .context("invalid namespace interface output")?;
            if links
                .iter()
                .any(|l| l["ifname"] != "lo" && l["ifname"] != r.interface)
            {
                bail!("namespace has an unexpected interface; refusing possible direct egress");
            }
            let wg = links
                .iter()
                .find(|l| l["ifname"] == r.interface)
                .context("WireGuard interface missing")?;
            if wg["ifindex"].as_u64() != r.interface_index
                || wg["linkinfo"]["info_kind"] != "wireguard"
                || !wg["flags"]
                    .as_array()
                    .is_some_and(|f| f.iter().any(|v| v == "UP"))
            {
                bail!("owned WireGuard interface identity or state changed");
            }
            let dir = self.dns_root.join(&ns);
            check_directory(&dir, true)?;
            if fs::read_to_string(dir.join(".capture-owner"))? != r.token {
                bail!("namespace resolver is not owned");
            }
        }
        Ok(())
    }
}
fn check_directory(path: &Path, private: bool) -> Result<()> {
    let meta = fs::symlink_metadata(path)?;
    if !meta.is_dir()
        || meta.file_type().is_symlink()
        || meta.uid() != unsafe { libc::geteuid() }
        || meta.permissions().mode() & if private { 0o077 } else { 0o022 } != 0
    {
        bail!(
            "unsafe owner, permissions or symlink on managed directory {}",
            path.display()
        );
    }
    Ok(())
}
fn ensure_directory(path: &Path, mode: u32) -> Result<()> {
    match fs::create_dir(path) {
        Ok(()) => fs::set_permissions(path, fs::Permissions::from_mode(mode))?,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(e.into()),
    };
    check_directory(path, mode == 0o700)
}
fn private_file(path: &Path) -> Result<File> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
        .context("open private file (regular file, no symlinks)")?;
    let meta = file.metadata()?;
    if !meta.is_file() || meta.permissions().mode() & 0o077 != 0 || meta.len() > 1024 * 1024 {
        bail!("private file must be regular, at most 1 MiB, with permissions 0600 or stricter");
    }
    Ok(file)
}
fn read_tunnel(e: &ExitConfig) -> Result<TunnelConfig> {
    let path = e
        .wireguard_config
        .as_deref()
        .context("exit has no WireGuard configuration")?;
    let file = private_file(path)?;
    use std::io::Read;
    let mut text = String::new();
    file.take(1024 * 1024 + 1)
        .read_to_string(&mut text)
        .context("read WireGuard config")?;
    TunnelConfig::parse(&text)
        .with_context(|| format!("invalid WireGuard configuration for exit {}", e.id))
}
fn require_root() -> Result<()> {
    if !cfg!(target_os = "linux") {
        bail!("WireGuard management requires Linux");
    }
    if unsafe { libc::geteuid() } != 0 {
        bail!("WireGuard up/down requires root privileges; plan is read-only");
    }
    Ok(())
}
pub fn plan(config: &Config) -> Result<Value> {
    let mut exits = Vec::new();
    for e in &config.exits {
        if e.wireguard_config.is_some() {
            let c = read_tunnel(e)?;
            exits.push(json!({"exit":e.id,"namespace":e.namespace_name(),"interface":interface_name(e),"addresses":c.addresses,"dns":c.dns,"mtu":c.mtu,"listen_port":"ephemeral (0)","commands":steps(e,&c,Path::new("<private-native-config-0600>")),"resolver":"private /etc/netns/<owned-namespace>/resolv.conf","managed":true}));
        } else {
            exits.push(json!({"exit":e.id,"namespace":e.namespace_name(),"managed":false,"direct":e.namespace_name().is_none()}));
        }
    }
    Ok(json!({"ok":true,"read_only":true,"exits":exits}))
}
pub fn up(config: &Config) -> Result<Value> {
    require_root()?;
    // Validate every secret file before the first host mutation.
    let tunnels = config
        .exits
        .iter()
        .filter(|e| e.wireguard_config.is_some())
        .map(|e| Ok((e, read_tunnel(e)?)))
        .collect::<Result<Vec<_>>>()?;
    let mut runner = SystemRunner;
    let mut manager = Manager::system(&mut runner);
    let _lock = manager.prepare()?;
    let mut exits = Vec::new();
    let mut ok = true;
    for (e, c) in tunnels {
        match manager.up_one(e, &c) {
            Ok(()) => exits.push(json!({"exit":e.id,"ok":true,"namespace":e.namespace_name()})),
            Err(error) => {
                ok = false;
                exits.push(json!({"exit":e.id,"ok":false,"error":format!("{error:#}")}));
            }
        }
    }
    Ok(json!({"ok":ok,"exits":exits}))
}
pub fn down(config: &Config) -> Result<Value> {
    require_root()?;
    let mut runner = SystemRunner;
    let mut manager = Manager::system(&mut runner);
    let _lock = manager.prepare()?;
    let mut ok = true;
    let mut exits = Vec::new();
    for e in config.exits.iter().filter(|e| e.wireguard_config.is_some()) {
        let ns = e.namespace_name().context("missing namespace")?;
        let result = (|| -> Result<()> {
            let r = manager.read_record(&ns)?;
            if r.exit_id != e.id {
                bail!("namespace belongs to a different exit");
            }
            manager.cleanup(&r)
        })();
        match result {
            Ok(()) => exits.push(json!({"exit":e.id,"ok":true})),
            Err(error) => {
                ok = false;
                exits.push(json!({"exit":e.id,"ok":false,"error":format!("{error:#}")}));
            }
        }
    }
    Ok(json!({"ok":ok,"exits":exits}))
}
pub fn status(config: &Config) -> Result<Value> {
    let mut runner = SystemRunner;
    let mut manager = Manager::system(&mut runner);
    let mut exits = Vec::new();
    let mut ok = true;
    for e in &config.exits {
        match manager.ready(e){Ok(())=>exits.push(json!({"exit":e.id,"ready":true,"namespace":e.namespace_name(),"managed":e.wireguard_config.is_some()})),Err(error)=>{ok=false;exits.push(json!({"exit":e.id,"ready":false,"error":format!("{error:#}")}));}}
    }
    Ok(json!({"ok":ok,"read_only":true,"exits":exits}))
}
pub(crate) fn require_ready(exit: &ExitConfig) -> Result<()> {
    let mut runner = SystemRunner;
    Manager::system(&mut runner).ready(exit)
}
#[cfg(test)]
mod tests {
    use super::*;
    const PRIVATE: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";
    fn fixture() -> String {
        format!(
            "[Interface]\nPrivateKey = {PRIVATE}\nAddress = 10.9.0.2/32, fd00::2/128\nDNS = 1.1.1.1, 2606:4700:4700::1111\nMTU = 1380\n[Peer]\nPublicKey = AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=\nEndpoint = vpn.example.test:51820\nAllowedIPs = 0.0.0.0/0, ::/0\nPersistentKeepalive = 25\n"
        )
    }
    fn exit() -> ExitConfig {
        ExitConfig {
            id: "test-exit".into(),
            namespace: None,
            wireguard_config: Some("/private/peer.conf".into()),
            parallel_games: Default::default(),
        }
    }
    #[test]
    fn parser_strips_quick_fields_but_preserves_native_protocol() {
        let c = TunnelConfig::parse(&fixture()).unwrap();
        assert_eq!(c.addresses, ["10.9.0.2/32", "fd00::2/128"]);
        assert_eq!(c.dns.len(), 2);
        assert_eq!(c.mtu, 1380);
        assert!(c.native.contains(PRIVATE));
        assert!(c.native.contains("Endpoint = vpn.example.test:51820"));
        for field in ["Address", "DNS", "MTU"] {
            assert!(!c.native.contains(field));
        }
    }
    #[test]
    fn rejects_hooks_unknown_fields_and_host_dns() {
        for extra in [
            "PreUp = echo unsafe\n",
            "PostDown = anything\n",
            "SaveConfig = true\n",
            "Table = auto\n",
            "Unknown = nope\n",
            "DNS = localhost\n",
        ] {
            let s = fixture().replacen("[Interface]\n", &format!("[Interface]\n{extra}"), 1);
            assert!(TunnelConfig::parse(&s).is_err(), "accepted {extra}");
        }
    }
    #[test]
    fn rejects_missing_dns_or_non_full_tunnel() {
        for s in [
            fixture().replace("DNS = 1.1.1.1, 2606:4700:4700::1111\n", ""),
            fixture().replace("0.0.0.0/0, ::/0", "10.0.0.0/8"),
            fixture().replace("10.9.0.2/32", "10.9.0.2/99"),
        ] {
            assert!(TunnelConfig::parse(&s).is_err());
        }
    }
    #[test]
    fn native_setup_precedes_move_and_routes_have_no_physical_path() {
        let e = exit();
        let c = TunnelConfig::parse(&fixture()).unwrap();
        let s = steps(&e, &c, Path::new("/tmp/private-config"));
        let cmds = s
            .iter()
            .map(|c| format!("{} {}", c.program, c.args.join(" ")))
            .collect::<Vec<_>>();
        let configure = cmds.iter().position(|s| s.contains("wg setconf")).unwrap();
        let mov = cmds
            .iter()
            .position(|s| s.contains(" netns crwg-"))
            .unwrap();
        assert!(configure < mov);
        assert!(cmds.iter().any(|s| s.contains("-4 route add default dev")));
        assert!(cmds.iter().any(|s| s.contains("-6 route add default dev")));
        assert!(
            cmds.iter()
                .all(|s| !s.contains(" veth ") && !s.contains(PRIVATE) && !s.contains("bash"))
        );
    }
    #[test]
    fn ipv4_only_has_no_ipv6_default() {
        let s = fixture()
            .replace(", fd00::2/128", "")
            .replace(", 2606:4700:4700::1111", "")
            .replace(", ::/0", "");
        let c = TunnelConfig::parse(&s).unwrap();
        assert!(!c.ipv6);
        assert!(
            !steps(&exit(), &c, Path::new("/secret"))
                .iter()
                .any(|s| s.args.contains(&"-6".into()))
        );
    }
    struct Fake {
        commands: Vec<CommandSpec>,
        ns_root: PathBuf,
        host: bool,
        moved: bool,
        fail: Option<String>,
        secret_seen: bool,
        active: bool,
    }
    impl Fake {
        fn new(root: &Path) -> Self {
            Self {
                commands: Vec::new(),
                ns_root: root.to_path_buf(),
                host: false,
                moved: false,
                fail: None,
                secret_seen: false,
                active: false,
            }
        }
    }
    impl Runner for Fake {
        fn execute(&mut self, c: &CommandSpec) -> Result<String> {
            self.commands.push(c.clone());
            let a = &c.args;
            let text = a.join(" ");
            if self
                .fail
                .as_ref()
                .is_some_and(|needle| text.contains(needle))
            {
                self.fail = None;
                bail!("injected command failure");
            }
            if c.program == "wg" {
                let p = Path::new(&a[2]);
                assert_eq!(fs::metadata(p)?.permissions().mode() & 0o777, 0o600);
                assert!(fs::read_to_string(p)?.contains(PRIVATE));
                self.secret_seen = true;
                return Ok(String::new());
            }
            if a.first().is_some_and(|v| v == "-j") {
                let present = if a.iter().any(|v| v == "-n") {
                    self.moved
                } else {
                    self.host
                };
                if present && a.iter().any(|v| v == "dev") {
                    return Ok(json!([{"ifname":interface_name(&exit()),"ifindex":7}]).to_string());
                }
                return Ok(if present {
                    json!([{"ifname":"lo","ifindex":1,"flags":["UP"]},{"ifname":interface_name(&exit()),"ifindex":7,"flags":["UP"],"linkinfo":{"info_kind":"wireguard"}}]).to_string()
                } else {
                    "[]".into()
                });
            }
            if a.starts_with(&["netns".into(), "add".into()]) {
                fs::write(self.ns_root.join(&a[2]), "namespace")?;
            } else if a.starts_with(&["netns".into(), "del".into()]) {
                fs::remove_file(self.ns_root.join(&a[2]))?;
                self.moved = false;
            } else if a.starts_with(&["netns".into(), "pids".into()]) {
                return Ok(if self.active {
                    "123\n".into()
                } else {
                    String::new()
                });
            } else if a.starts_with(&["link".into(), "add".into()]) {
                self.host = true;
            } else if a.starts_with(&["link".into(), "set".into()])
                && a.iter().any(|v| v == "netns")
            {
                self.host = false;
                self.moved = true;
            } else if a.iter().any(|v| v == "del") {
                if a.iter().any(|v| v == "-n") {
                    self.moved = false;
                } else {
                    self.host = false;
                }
            }
            Ok(String::new())
        }
    }
    fn manager<'a>(dir: &Path, fake: &'a mut Fake) -> Manager<'a, Fake> {
        Manager {
            state_root: dir.join("state"),
            dns_root: dir.join("dns"),
            netns_root: dir.join("netns"),
            runner: fake,
        }
    }
    #[test]
    fn rollback_removes_only_resources_created_by_failed_setup() {
        for failure in ["setconf", "address add"] {
            let d = tempfile::tempdir().unwrap();
            fs::create_dir(d.path().join("netns")).unwrap();
            let mut fake = Fake::new(&d.path().join("netns"));
            fake.fail = Some(failure.into());
            let mut m = manager(d.path(), &mut fake);
            let _lock = m.prepare().unwrap();
            assert!(
                m.up_one(&exit(), &TunnelConfig::parse(&fixture()).unwrap())
                    .is_err()
            );
            assert!(!m.netns_root.join(exit().namespace_name().unwrap()).exists());
            assert!(!m.record_path(&exit().namespace_name().unwrap()).exists());
            assert!(!m.runner.host);
            assert!(!m.runner.moved);
        }
    }
    #[test]
    fn down_refuses_replaced_namespace_and_active_workers() {
        let d = tempfile::tempdir().unwrap();
        fs::create_dir(d.path().join("netns")).unwrap();
        let mut fake = Fake::new(&d.path().join("netns"));
        let mut m = manager(d.path(), &mut fake);
        let _lock = m.prepare().unwrap();
        m.up_one(&exit(), &TunnelConfig::parse(&fixture()).unwrap())
            .unwrap();
        let ns = exit().namespace_name().unwrap();
        let r = m.read_record(&ns).unwrap();
        m.runner.active = true;
        assert!(m.cleanup(&r).is_err());
        assert!(m.netns_root.join(&ns).exists());
        m.runner.active = false;
        fs::rename(
            m.netns_root.join(&ns),
            m.netns_root.join("old-owned-namespace"),
        )
        .unwrap();
        fs::write(m.netns_root.join(&ns), "unrelated replacement").unwrap();
        assert!(m.cleanup(&r).is_err());
        assert!(m.netns_root.join(&ns).exists());
        assert!(m.record_path(&ns).exists());
    }
    #[test]
    fn secret_tempfile_and_plan_are_private_and_down_cleans_owned_dns() {
        let d = tempfile::tempdir().unwrap();
        fs::create_dir(d.path().join("netns")).unwrap();
        let mut fake = Fake::new(&d.path().join("netns"));
        let mut m = manager(d.path(), &mut fake);
        let _lock = m.prepare().unwrap();
        m.up_one(&exit(), &TunnelConfig::parse(&fixture()).unwrap())
            .unwrap();
        assert!(m.runner.secret_seen);
        let commands = serde_json::to_string(&m.runner.commands).unwrap();
        assert!(!commands.contains(PRIVATE));
        let setconf = m
            .runner
            .commands
            .iter()
            .find(|s| s.program == "wg")
            .unwrap();
        assert!(!Path::new(&setconf.args[2]).exists());
        let ns = exit().namespace_name().unwrap();
        let record = m.read_record(&ns).unwrap();
        assert!(
            !fs::read_to_string(m.record_path(&ns))
                .unwrap()
                .contains(PRIVATE)
        );
        assert_eq!(
            fs::metadata(m.dns_root.join(&ns))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        m.cleanup(&record).unwrap();
        assert!(!m.dns_root.join(&ns).exists());
        assert!(!m.netns_root.join(&ns).exists());
    }
    #[test]
    fn existing_namespace_is_never_claimed_or_deleted() {
        let d = tempfile::tempdir().unwrap();
        let nsroot = d.path().join("netns");
        fs::create_dir(&nsroot).unwrap();
        let ns = exit().namespace_name().unwrap();
        fs::write(nsroot.join(&ns), "operator resource").unwrap();
        let mut fake = Fake::new(&nsroot);
        let mut m = manager(d.path(), &mut fake);
        let _lock = m.prepare().unwrap();
        assert!(
            m.up_one(&exit(), &TunnelConfig::parse(&fixture()).unwrap())
                .is_err()
        );
        assert!(m.runner.commands.is_empty());
        assert!(m.read_record(&ns).is_err());
        assert_eq!(
            fs::read_to_string(nsroot.join(ns)).unwrap(),
            "operator resource"
        );
    }
    #[test]
    fn plan_reads_keys_without_output_or_filesystem_mutation() {
        let d = tempfile::tempdir().unwrap();
        let key = d.path().join("peer.conf");
        fs::write(&key, fixture()).unwrap();
        fs::set_permissions(&key, fs::Permissions::from_mode(0o600)).unwrap();
        let config_path = d.path().join("runner.json");
        fs::write(&config_path,json!({"run_id":"test","evidence_root":"evidence","exits":[{"id":"peer","wireguard_config":"peer.conf"}]}).to_string()).unwrap();
        let config = Config::load(&config_path).unwrap();
        let before = fs::read_dir(d.path()).unwrap().count();
        let p = plan(&config).unwrap();
        assert_eq!(p["read_only"], true);
        assert!(!p.to_string().contains(PRIVATE));
        assert_eq!(before, fs::read_dir(d.path()).unwrap().count());
    }
    #[test]
    fn configuration_fingerprint_tracks_effective_private_and_route_settings() {
        let c = TunnelConfig::parse(&fixture()).unwrap();
        let changed = TunnelConfig::parse(&fixture().replace("10.9.0.2", "10.9.0.3")).unwrap();
        assert_ne!(c.fingerprint(), changed.fingerprint());
        let changed = TunnelConfig::parse(&fixture().replace("MTU = 1380", "MTU = 1400")).unwrap();
        assert_ne!(c.fingerprint(), changed.fingerprint());
        assert!(!c.fingerprint().contains(PRIVATE));
    }
    #[test]
    fn fixed_exported_client_ports_become_ephemeral() {
        let c = TunnelConfig::parse(
            &fixture().replace("[Interface]", "[Interface]\nListenPort = 51820"),
        )
        .unwrap();
        assert!(c.native.contains("ListenPort = 0\n"));
        assert!(!c.native.contains("ListenPort = 51820"));
    }
    #[test]
    fn readiness_rejects_changed_configuration_until_down_and_up() {
        let d = tempfile::tempdir().unwrap();
        fs::create_dir(d.path().join("netns")).unwrap();
        let key = d.path().join("private.conf");
        fs::write(&key, fixture()).unwrap();
        fs::set_permissions(&key, fs::Permissions::from_mode(0o600)).unwrap();
        let mut e = exit();
        e.wireguard_config = Some(key.clone());
        let mut fake = Fake::new(&d.path().join("netns"));
        let mut m = manager(d.path(), &mut fake);
        let _lock = m.prepare().unwrap();
        m.up_one(&e, &read_tunnel(&e).unwrap()).unwrap();
        m.ready(&e).unwrap();
        fs::write(&key, fixture().replace("MTU = 1380", "MTU = 1400")).unwrap();
        let error = m.ready(&e).unwrap_err();
        assert!(error.to_string().contains("changed since setup"));
        // Down still works after edits: it checks recorded resource identity, not new keys.
        let owned = m.read_record(&e.namespace_name().unwrap()).unwrap();
        m.cleanup(&owned).unwrap();
    }
}
