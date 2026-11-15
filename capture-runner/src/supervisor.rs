//! One independent worker process per explicitly configured exit.
use crate::{
    config::{Config, ExitConfig},
    wireguard,
};
use anyhow::{Context, Result, bail};
#[cfg(unix)]
use std::os::{
    fd::AsRawFd,
    unix::fs::{MetadataExt, PermissionsExt},
};
use std::{future::Future, path::Path, time::Duration};
use tokio::process::{Child, Command};

pub fn validate_worker_exit(_config: &Config, exit: &ExitConfig) -> Result<()> {
    if let Some(ns) = exit.namespace_name() {
        crate::config::validate_namespace(&ns)?;
        let current =
            std::fs::metadata("/proc/self/ns/net").context("inspect worker network namespace")?;
        let expected = std::fs::metadata(Path::new("/var/run/netns").join(&ns))
            .context("configured worker namespace is unavailable")?;
        if current.dev() != expected.dev() || current.ino() != expected.ino() {
            bail!(
                "worker is outside configured namespace {ns}; launch with run, never fall back to direct networking"
            );
        }
    }
    Ok(())
}
fn worker_command(
    executable: &Path,
    config_path: &Path,
    config: &Config,
    exit: &ExitConfig,
) -> Result<Command> {
    let namespace = exit.namespace_name();
    if let Some(ns) = &namespace {
        crate::config::validate_namespace(ns)?;
    }
    if let Some(user) = &config.run_as {
        crate::config::validate_user(user)?;
    }
    let mut parts = Vec::<std::ffi::OsString>::new();
    if let Some(ns) = namespace {
        parts.extend(["ip".into(), "netns".into(), "exec".into(), ns.into()]);
    }
    if let Some(user) = &config.run_as {
        parts.extend(["runuser".into(), "-u".into(), user.into(), "--".into()]);
    }
    parts.extend([
        executable.as_os_str().to_owned(),
        "worker".into(),
        "--config".into(),
        config_path.as_os_str().to_owned(),
        "--exit".into(),
        exit.id.clone().into(),
    ]);
    let mut command = Command::new(&parts[0]);
    command.args(&parts[1..]);
    command.process_group(0);
    command.kill_on_drop(true);
    Ok(command)
}
struct Worker {
    id: String,
    child: Child,
    group: u32,
    finished: bool,
}
impl Worker {
    fn signal(&self, signal: i32) {
        if !self.finished && self.group > 1 {
            unsafe { libc::kill(-(self.group as i32), signal) };
        }
    }
    fn exited_without_reaping(&self) -> std::io::Result<bool> {
        // Keep the leader's PID reserved while terminating any descendants.
        // Reaping first and retaining a numeric PGID could target a reused group later.
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        let code = unsafe {
            libc::waitid(
                libc::P_PID,
                self.group,
                &mut info,
                libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
            )
        };
        if code < 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(unsafe { info.si_pid() } == self.group as i32)
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.signal(libc::SIGKILL);
    }
}
async fn stopped(workers: &mut [Worker], ok: &mut bool) -> bool {
    for worker in workers.iter_mut().filter(|w| !w.finished) {
        match worker.exited_without_reaping() {
            Ok(false) => continue,
            Ok(true) => worker.signal(libc::SIGKILL),
            Err(error) => {
                // If another reaper has consumed it, ownership of that numeric PID is gone.
                if error.raw_os_error() == Some(libc::ECHILD) {
                    worker.group = 0;
                    worker.finished = true;
                }
                eprintln!("cannot inspect owned exit {}: {error}", worker.id);
                *ok = false;
                continue;
            }
        }
        match worker.child.try_wait() {
            Ok(Some(status)) => {
                worker.finished = true;
                worker.group = 0;
                if !status.success() {
                    eprintln!("exit {} worker finished with {status}", worker.id);
                    *ok = false;
                }
            }
            Ok(None) => {}
            Err(error) => {
                eprintln!("cannot inspect exit {} worker: {error}", worker.id);
                worker.group = 0;
                worker.finished = true;
                *ok = false;
            }
        }
    }
    workers.iter().all(|w| w.finished)
}
async fn drain(workers: &mut [Worker], duration: Duration) {
    let deadline = tokio::time::Instant::now() + duration;
    let mut ignored = true;
    while !stopped(workers, &mut ignored).await && tokio::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}
async fn supervise<F: Future<Output = ()>>(
    commands: Vec<(String, Command)>,
    shutdown: F,
) -> Result<bool> {
    let mut ok = true;
    let mut workers = Vec::new();
    // Spawn every exit before waiting for any exit; failures are local to that exit.
    for (id, mut command) in commands {
        match command.spawn() {
            Ok(child) => {
                let group = child.id().context("worker has no process id")?;
                workers.push(Worker {
                    id,
                    child,
                    group,
                    finished: false,
                });
            }
            Err(error) => {
                eprintln!("cannot start exit {id}: {error}");
                ok = false;
            }
        }
    }
    tokio::pin!(shutdown);
    loop {
        if stopped(&mut workers, &mut ok).await {
            return Ok(ok);
        }
        tokio::select! {
            _=&mut shutdown=>break,
            _=tokio::time::sleep(Duration::from_millis(50))=>{},
        }
    }
    eprintln!("stopping capture workers");
    for worker in &workers {
        worker.signal(libc::SIGINT);
    }
    drain(&mut workers, Duration::from_secs(5)).await;
    for worker in &workers {
        worker.signal(libc::SIGTERM);
    }
    drain(&mut workers, Duration::from_secs(2)).await;
    for worker in &workers {
        worker.signal(libc::SIGKILL);
    }
    for worker in workers.iter_mut().filter(|w| !w.finished) {
        let _ = tokio::time::timeout(Duration::from_secs(2), worker.child.wait()).await;
        worker.finished = true;
        worker.group = 0;
    }
    Ok(false)
}
fn worker_snapshot(config: &Config) -> Result<tempfile::NamedTempFile> {
    let mut resolved = config.clone();
    resolved.wireguard_dir = None;
    for exit in &mut resolved.exits {
        exit.namespace = exit.namespace_name();
        exit.wireguard_config = None;
    }
    let mut file = tempfile::Builder::new()
        .prefix("capture-runner-worker-")
        .tempfile_in("/tmp")?;
    serde_json::to_writer(file.as_file_mut(), &resolved)?;
    file.as_file().sync_all()?;
    file.as_file()
        .set_permissions(std::fs::Permissions::from_mode(0o600))?;
    if let Some(user) = &config.run_as {
        let name = std::ffi::CString::new(user.as_str())?;
        let mut entry: libc::passwd = unsafe { std::mem::zeroed() };
        let mut output = std::ptr::null_mut();
        let mut buffer = vec![0u8; 65536];
        let code = unsafe {
            libc::getpwnam_r(
                name.as_ptr(),
                &mut entry,
                buffer.as_mut_ptr().cast(),
                buffer.len(),
                &mut output,
            )
        };
        if code != 0 || output.is_null() {
            bail!("run_as account cannot be resolved: {user}");
        }
        let code = unsafe { libc::fchown(file.as_file().as_raw_fd(), entry.pw_uid, entry.pw_gid) };
        if code != 0 {
            return Err(std::io::Error::last_os_error())
                .context("grant worker access to resolved configuration");
        }
    }
    Ok(file)
}
pub async fn run(config_path: &Path, config: &Config) -> Result<bool> {
    std::fs::canonicalize(config_path).context("resolve worker config path")?;
    // Only the supervisor reads private WireGuard paths. Workers get resolved public settings.
    let snapshot = worker_snapshot(config)?;
    let executable = std::env::current_exe().context("locate capture-runner executable")?;
    let mut commands = Vec::new();
    let mut preflight_ok = true;
    for exit in &config.exits {
        if let Err(error) = wireguard::require_ready(exit) {
            eprintln!("exit {} unavailable: {error:#}", exit.id);
            preflight_ok = false;
            continue;
        }
        commands.push((
            exit.id.clone(),
            worker_command(&executable, snapshot.path(), config, exit)?,
        ));
    }
    let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .context("install SIGTERM handler")?;
    let shutdown = async move {
        tokio::select! {_=tokio::signal::ctrl_c()=>{},_=term.recv()=>{}}
    };
    // Do not short-circuit: surviving exits must still run after a failed preflight.
    let workers_ok = supervise(commands, shutdown).await?;
    Ok(preflight_ok && workers_ok)
}
#[cfg(test)]
mod tests {
    use super::*;
    fn config() -> Config {
        serde_json::from_value(serde_json::json!({"evidence_root":"/evidence","data_root":"/captures","run_id":"test","exits":[{"id":"one"}]})).unwrap()
    }
    #[test]
    fn namespace_and_runuser_wrap_same_binary_without_shell() {
        let mut c = config();
        c.run_as = Some("capture".into());
        let mut e = c.exits[0].clone();
        e.wireguard_config = Some("/private/key.conf".into());
        let cmd = worker_command(
            Path::new("/opt/capture-runner"),
            Path::new("/opt/config.json"),
            &c,
            &e,
        )
        .unwrap();
        let cmd = cmd.as_std();
        assert_eq!(cmd.get_program(), "ip");
        let args = cmd
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert_eq!(&args[..3], ["netns", "exec", &e.namespace_name().unwrap()]);
        assert!(
            args.windows(4)
                .any(|w| w == ["runuser", "-u", "capture", "--"])
        );
        assert!(args.ends_with(&[
            "worker".into(),
            "--config".into(),
            "/opt/config.json".into(),
            "--exit".into(),
            "one".into()
        ]));
    }
    #[test]
    fn direct_is_only_explicit_without_namespace_or_wireguard() {
        let c = config();
        let cmd =
            worker_command(Path::new("/runner"), Path::new("/config"), &c, &c.exits[0]).unwrap();
        assert_eq!(cmd.as_std().get_program(), "/runner");
        validate_worker_exit(&c, &c.exits[0]).unwrap();
    }
    #[tokio::test]
    async fn failed_exit_does_not_cancel_other_exit() {
        let mut fail = Command::new("/usr/bin/false");
        fail.process_group(0);
        let mut slow = Command::new("/usr/bin/sleep");
        slow.arg("0.2").process_group(0);
        let start = tokio::time::Instant::now();
        assert!(
            !supervise(
                vec![("failed".into(), fail), ("survivor".into(), slow)],
                std::future::pending()
            )
            .await
            .unwrap()
        );
        assert!(start.elapsed() >= Duration::from_millis(190));
    }
    #[tokio::test]
    async fn shutdown_interrupts_and_reaps_workers() {
        let mut slow = Command::new("/usr/bin/sleep");
        slow.arg("30").process_group(0);
        let start = tokio::time::Instant::now();
        assert!(
            !supervise(
                vec![("sleeping".into(), slow)],
                tokio::time::sleep(Duration::from_millis(60))
            )
            .await
            .unwrap()
        );
        assert!(start.elapsed() < Duration::from_secs(2));
    }
    #[test]
    fn worker_cannot_bypass_missing_namespace() {
        let c = config();
        let mut e = c.exits[0].clone();
        e.namespace = Some("crwg-missing-offline-test".into());
        assert!(validate_worker_exit(&c, &e).is_err());
    }
    #[test]
    fn worker_snapshot_does_not_reopen_private_tunnel_directory() {
        let mut c = config();
        c.wireguard_dir = Some("/not-readable/wireguard".into());
        c.exits[0].wireguard_config = Some("/not-readable/wireguard/one.conf".into());
        let expected = c.exits[0].namespace_name();
        let snapshot = worker_snapshot(&c).unwrap();
        let worker = Config::load(snapshot.path()).unwrap();
        assert!(worker.wireguard_dir.is_none());
        assert!(worker.exits[0].wireguard_config.is_none());
        assert_eq!(worker.exits[0].namespace_name(), expected);
        assert!(
            !std::fs::read_to_string(snapshot.path())
                .unwrap()
                .contains("not-readable")
        );
    }
    #[tokio::test]
    async fn reaped_worker_retires_process_group_id() {
        let mut command = Command::new("/usr/bin/true");
        command.process_group(0);
        let child = command.spawn().unwrap();
        let group = child.id().unwrap();
        let mut workers = vec![Worker {
            id: "short".into(),
            child,
            group,
            finished: false,
        }];
        let mut ok = true;
        for _ in 0..100 {
            if stopped(&mut workers, &mut ok).await {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        assert!(workers[0].finished);
        assert_eq!(workers[0].group, 0);
        assert!(ok);
    }
    #[tokio::test]
    async fn shutdown_also_kills_descendants_after_group_leader_exits() {
        let d = tempfile::tempdir().unwrap();
        let pidfile = d.path().join("pid");
        let script = "import subprocess,sys,time; p=subprocess.Popen([sys.executable,'-c','import signal,time; signal.signal(signal.SIGINT,signal.SIG_IGN); signal.signal(signal.SIGTERM,signal.SIG_IGN); time.sleep(30)']); open(sys.argv[1],'w').write(str(p.pid)); time.sleep(30)";
        let mut cmd = Command::new("/usr/bin/python3");
        cmd.args(["-c", script])
            .arg(&pidfile)
            .process_group(0)
            .stderr(std::process::Stdio::null());
        let shutdown = async {
            for _ in 0..100 {
                if pidfile.exists() {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    return;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        };
        assert!(
            !supervise(vec![("descendants".into(), cmd)], shutdown)
                .await
                .unwrap()
        );
        let pid = std::fs::read_to_string(pidfile).unwrap();
        tokio::time::sleep(Duration::from_millis(50)).await;
        let state = std::fs::read_to_string(format!("/proc/{pid}/stat")).unwrap_or_default();
        let terminated = state.is_empty()
            || state
                .rsplit_once(") ")
                .is_some_and(|(_, tail)| tail.starts_with('Z'));
        if !terminated {
            unsafe { libc::kill(pid.parse().unwrap(), libc::SIGKILL) };
        }
        assert!(terminated, "descendant survived supervisor shutdown");
    }
}
