use crate::{
    adapters,
    catalog::Catalog,
    config::{Config, ExitConfig},
    context::RunContext,
    journal::{Journal, private_dir, write_new_json},
    model::{CaptureStatus, Completion, Game, Mode, classify_error, component, now_ms},
};
use anyhow::{Result, ensure};
use async_trait::async_trait;
use futures_util::{StreamExt, stream};
use serde_json::{Value, json};
#[cfg(unix)]
use std::os::{fd::AsRawFd, unix::fs::OpenOptionsExt};
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::watch;

#[async_trait]
pub trait Executor: Send + Sync {
    fn supports(&self, provider: &str, game: &Game, mode: &Mode) -> bool;
    async fn execute(
        &self,
        ctx: &RunContext,
        provider: &str,
        game: &Game,
        mode: &Mode,
        settings: &Value,
    ) -> Result<Completion>;
}
pub struct NativeExecutor;
#[async_trait]
impl Executor for NativeExecutor {
    fn supports(&self, p: &str, g: &Game, m: &Mode) -> bool {
        adapters::supports(p, g, m)
    }
    async fn execute(
        &self,
        c: &RunContext,
        p: &str,
        g: &Game,
        m: &Mode,
        s: &Value,
    ) -> Result<Completion> {
        adapters::execute(c, p, g, m, s).await
    }
}
#[derive(Clone)]
struct EventLog {
    file: Arc<Mutex<File>>,
    alerts: Arc<Mutex<File>>,
}
fn append_file(path: &Path) -> Result<File> {
    let mut options = OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    options.mode(0o600);
    Ok(options.open(path)?)
}
impl EventLog {
    fn new(dir: &Path) -> Result<Self> {
        Ok(Self {
            file: Arc::new(Mutex::new(append_file(&dir.join("events.jsonl"))?)),
            alerts: Arc::new(Mutex::new(append_file(&dir.join("alerts.jsonl"))?)),
        })
    }
    fn emit(&self, event: &Value, alert: bool) -> Result<()> {
        let line = serde_json::to_vec(event)?;
        let mut f = self
            .file
            .lock()
            .map_err(|_| anyhow::anyhow!("event log poisoned"))?;
        f.write_all(&line)?;
        f.write_all(b"\n")?;
        if alert {
            f.sync_data()?;
            let mut a = self
                .alerts
                .lock()
                .map_err(|_| anyhow::anyhow!("alert log poisoned"))?;
            a.write_all(&line)?;
            a.write_all(b"\n")?;
            a.sync_data()?;
            eprintln!("{}", event);
        }
        Ok(())
    }
}
fn acquire_exit_lock(dir: &Path) -> Result<File> {
    let file = append_file(&dir.join("worker.lock"))?;
    #[cfg(unix)]
    ensure!(
        unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0,
        "another worker owns this run/exit"
    );
    Ok(file)
}
#[derive(Default, Debug)]
pub struct Summary {
    pub counts: BTreeMap<String, u64>,
}
impl Summary {
    fn add(&mut self, key: &str) {
        *self.counts.entry(key.into()).or_default() += 1;
    }
}
fn status_name(status: CaptureStatus) -> &'static str {
    match status {
        CaptureStatus::Complete => "complete",
        CaptureStatus::UnknownEvent => "unknown_event",
        CaptureStatus::TransportError => "transport_error",
        CaptureStatus::ProviderError => "provider_error",
        CaptureStatus::Unsupported => "unsupported",
        CaptureStatus::Interrupted => "interrupted",
    }
}
pub fn exit_dir(config: &Config, exit: &ExitConfig) -> PathBuf {
    config.data_root.join(&config.run_id).join(&exit.id)
}
fn existing_sample(sample: &Path) -> Result<Option<bool>> {
    if !sample.exists() {
        return Ok(None);
    }
    let attempts = fs::read_dir(sample)?.collect::<std::io::Result<Vec<_>>>()?;
    if attempts.is_empty() {
        return Ok(None);
    }
    let complete = attempts.iter().any(|entry| {
        fs::read(entry.path().join("result.json"))
            .ok()
            .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
            .is_some_and(|v| v["status"] == "complete")
    });
    Ok(Some(complete))
}

pub async fn run_exit(
    config: &Config,
    exit: &ExitConfig,
    catalog: &Catalog,
    executor: Arc<dyn Executor>,
    stop: watch::Receiver<bool>,
) -> Result<Value> {
    let selected = catalog.selected(config)?;
    let dir = exit_dir(config, exit);
    private_dir(&dir)?;
    let _lock = acquire_exit_lock(&dir)?;
    let manifest = json!({"run_id":config.run_id,"exit":exit.id,"namespace":exit.namespace_name(),"catalog_sha256":catalog.fingerprint,"sources":catalog.sources});
    let manifest_path = dir.join("run.json");
    if manifest_path.exists() {
        let prior: Value = serde_json::from_slice(&fs::read(&manifest_path)?)?;
        ensure!(
            prior == manifest,
            "run/exit evidence or namespace changed; choose a new run_id"
        );
    } else {
        write_new_json(&manifest_path, &manifest)?;
    }
    let log = EventLog::new(&dir)?;
    let summary = Arc::new(Mutex::new(Summary::default()));
    let errors = Arc::new(Mutex::new(Vec::<String>::new()));
    log.emit(
        &json!({"event":"worker_start","at_ms":now_ms(),"exit":exit.id}),
        false,
    )?;
    stream::iter(selected).for_each_concurrent(None,|provider|{
        let executor=executor.clone();let summary=summary.clone();let log=log.clone();let errors=errors.clone();let dir=dir.clone();let stop=stop.clone();
        async move {
            let provider_id=provider.id;
            stream::iter(provider.games).for_each_concurrent(config.parallel_games(exit,&provider_id),|game|{
                let executor=executor.clone();let summary=summary.clone();let log=log.clone();let errors=errors.clone();let dir=dir.clone();let mut stop=stop.clone();let provider_id=&provider_id;
                async move {
                    let result:Result<()>=async {
                        // A game owns one slot through all its modes/samples. No global provider lock.
                        for mode in &game.modes {for sample in 0..config.samples_per_mode {
                            if *stop.borrow(){summary.lock().unwrap().add("cancelled_before_start");return Ok(());}
                            if !executor.supports(provider_id,&game,mode){summary.lock().unwrap().add("unsupported");log.emit(&json!({"event":"unsupported","exit":exit.id,"provider":provider_id,"game":game.id,"mode":mode.key,"sample":sample,"reason":adapters::unsupported_reason(provider_id,&game,mode)}),false)?;continue;}
                            let sample_dir=dir.join(provider_id).join(component(&game.id)).join(component(&mode.key)).join(format!("sample-{sample:06}"));
                            if let Some(complete)=existing_sample(&sample_dir)? {summary.lock().unwrap().add(if complete{"existing_complete"}else{"existing_incomplete"});log.emit(&json!({"event":"resume_skipped","exit":exit.id,"provider":provider_id,"game":game.id,"mode":mode.key,"sample":sample,"complete":complete,"path":sample_dir}),false)?;continue;}
                            let path=sample_dir.join(format!("attempt-{}",uuid::Uuid::new_v4()));
                            let metadata=json!({"run_id":config.run_id,"exit":exit.id,"provider":provider_id,"game":game.id,"mode":mode.key,"sample":sample,"catalog_sha256":catalog.fingerprint,"started_ms":now_ms()});
                            let journal=Journal::create(&path,&metadata)?;
                            let attempt=async {
                                let settings=catalog.settings_for(provider_id,&game,mode)?;
                                let ctx=RunContext::new(provider_id,journal.clone(),Duration::from_secs(config.timeout_seconds),config.max_steps)?;
                                executor.execute(&ctx,provider_id,&game,mode,&settings).await
                            };
                            let outcome=tokio::select! {biased; _=stop.wait_for(|s|*s)=>None, result=attempt=>Some(result)};
                            let (status,completion,error)=match outcome {
                                Some(Ok(c))=>(CaptureStatus::Complete,Some(c),None),
                                Some(Err(e))=>(classify_error(&e),None,Some(format!("{e:#}"))),
                                None=>(CaptureStatus::Interrupted,None,Some("shutdown during attempt; never auto-replayed".into())),
                            };
                            let result=json!({"status":status,"completion":completion,"error":error,"finished_ms":now_ms(),"attempt":metadata});
                            journal.finish(&result)?;summary.lock().unwrap().add(status_name(status));
                            log.emit(&json!({"event":status_name(status),"at_ms":now_ms(),"exit":exit.id,"provider":provider_id,"game":game.id,"mode":mode.key,"sample":sample,"reason":error,"capture":path}),status==CaptureStatus::UnknownEvent)?;
                        }}Ok(())
                    }.await;
                    if let Err(error)=result {errors.lock().unwrap().push(format!("{provider_id}/{}: {error:#}",game.id));}
                }
            }).await;
        }
    }).await;
    let counts = &summary.lock().unwrap().counts;
    let errors = errors.lock().unwrap();
    let ok = errors.is_empty()
        && [
            "unknown_event",
            "transport_error",
            "provider_error",
            "unsupported",
            "interrupted",
            "existing_incomplete",
            "cancelled_before_start",
        ]
        .iter()
        .all(|s| counts.get(*s).copied().unwrap_or(0) == 0);
    let result = json!({"event":"worker_finish","run_id":config.run_id,"exit":exit.id,"counts":counts,"infrastructure_errors":*errors,"ok":ok,"finished_ms":now_ms()});
    log.emit(&result, false)?;
    write_new_json(
        &dir.join(format!("summary-{}.json", uuid::Uuid::new_v4())),
        &result,
    )?;
    Ok(result)
}
pub async fn shutdown_signal() -> Result<()> {
    #[cfg(unix)]
    {
        let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
        tokio::select! {r=tokio::signal::ctrl_c()=>r?,_=term.recv()=>{}}
    }
    #[cfg(not(unix))]
    tokio::signal::ctrl_c().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    struct Fake {
        active: Mutex<BTreeMap<String, usize>>,
        max: Mutex<BTreeMap<String, usize>>,
        global: AtomicUsize,
        global_max: AtomicUsize,
    }
    #[async_trait]
    impl Executor for Fake {
        fn supports(&self, _: &str, _: &Game, _: &Mode) -> bool {
            true
        }
        async fn execute(
            &self,
            c: &RunContext,
            p: &str,
            g: &Game,
            _: &Mode,
            _: &Value,
        ) -> Result<Completion> {
            let exit = c
                .journal
                .path()
                .ancestors()
                .nth(5)
                .unwrap()
                .file_name()
                .unwrap()
                .to_str()
                .unwrap();
            let key = format!("{exit}/{p}");
            {
                let mut a = self.active.lock().unwrap();
                let n = a.entry(key.clone()).or_default();
                *n += 1;
                let mut m = self.max.lock().unwrap();
                let maximum = m.entry(key.clone()).or_default();
                *maximum = (*maximum).max(*n);
            }
            let n = self.global.fetch_add(1, Ordering::SeqCst) + 1;
            self.global_max.fetch_max(n, Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(15)).await;
            self.global.fetch_sub(1, Ordering::SeqCst);
            *self.active.lock().unwrap().get_mut(&key).unwrap() -= 1;
            c.note("fixture_response", &json!({"native_end":true}))?;
            if g.id == "novel" {
                return Err(crate::model::unknown_event("new native phase"));
            }
            Ok(Completion::new(None))
        }
    }
    #[tokio::test]
    async fn concurrency_is_per_provider_and_exit_and_unknown_does_not_stop_queue() -> Result<()> {
        let root = tempfile::tempdir()?;
        let cfg_path = root.path().join("config.json");
        fs::write(
            &cfg_path,
            serde_json::to_vec(
                &json!({"evidence_root":".","data_root":"data","run_id":"test","default_parallel_games":2,"providers":[],"exits":[{"id":"a"},{"id":"b"}]}),
            )?,
        )?;
        let config = Config::load(&cfg_path)?;
        let mut providers = vec![];
        for p in ["one", "two"] {
            let mut games = vec![];
            for g in ["first", "novel", "third", "fourth", "fifth"] {
                games.push(Game {
                    id: g.into(),
                    name: g.into(),
                    modes: vec![Mode {
                        key: "normal".into(),
                        kind: crate::model::ModeKind::Normal,
                        label: String::new(),
                        requires_prerequisites: Value::Null,
                    }],
                });
            }
            providers.push(crate::model::Provider {
                id: p.into(),
                name: p.into(),
                games,
            });
        }
        let catalog = Catalog {
            providers,
            settings: BTreeMap::new(),
            fingerprint: "fixture".into(),
            sources: vec![],
        };
        let fake = Arc::new(Fake {
            active: Mutex::new(BTreeMap::new()),
            max: Mutex::new(BTreeMap::new()),
            global: AtomicUsize::new(0),
            global_max: AtomicUsize::new(0),
        });
        let (_tx, stop) = watch::channel(false);
        let (a, b) = tokio::join!(
            run_exit(
                &config,
                &config.exits[0],
                &catalog,
                fake.clone(),
                stop.clone()
            ),
            run_exit(
                &config,
                &config.exits[1],
                &catalog,
                fake.clone(),
                stop.clone()
            )
        );
        for value in [a?, b?] {
            assert_eq!(value["counts"]["complete"], 8);
            assert_eq!(value["counts"]["unknown_event"], 2);
        }
        assert_eq!(fake.max.lock().unwrap().len(), 4);
        assert!(fake.max.lock().unwrap().values().all(|n| *n == 2));
        assert!(fake.global_max.load(Ordering::SeqCst) > 2);
        let resumed = run_exit(&config, &config.exits[0], &catalog, fake, stop).await?;
        assert_eq!(resumed["counts"]["existing_complete"], 8);
        assert_eq!(resumed["counts"]["existing_incomplete"], 2);
        Ok(())
    }
    #[test]
    fn partial_sample_is_not_replayed() -> Result<()> {
        let root = tempfile::tempdir()?;
        let p = root.path().join("sample");
        private_dir(&p.join("attempt-1"))?;
        assert_eq!(existing_sample(&p)?, Some(false));
        Ok(())
    }
}
