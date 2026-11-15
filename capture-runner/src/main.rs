use anyhow::{Context, Result};
use capture_runner::{
    capture_bank, catalog::Catalog, config::Config, engine, replay, ruby_import, supervisor,
    wireguard,
};
use clap::{Parser, Subcommand};
use serde_json::{Value, json};
use std::{fs, io::BufReader, path::PathBuf, sync::Arc};
#[derive(Parser)]
#[command(
    version,
    about = "Collect native demo bets from evidence, across providers and WireGuard exits"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    /// Import supplied RubyPlay native JSON captures into a portable offline bank.
    ImportRuby {
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        output: PathBuf,
    },
    /// Export native captures, including partial attempts, to a portable local bank.
    Export {
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        output: PathBuf,
    },
    /// Inspect or replay a local bank. Does not connect to providers.
    Replay {
        #[arg(long)]
        bank: PathBuf,
        #[arg(long, required_if_eq("requests", "-"))]
        attempt: Option<String>,
        /// JSONL of exact outbound messages; use - to read standard input.
        #[arg(long, requires = "attempt")]
        requests: Option<PathBuf>,
        /// Emit recorded outbound messages as JSONL, suitable for --requests.
        #[arg(long, requires = "attempt", conflicts_with = "requests")]
        emit_requests: bool,
    },
    /// Read evidence and report actual connector coverage and per-exit parallelism. No network calls.
    Plan {
        #[arg(long)]
        config: PathBuf,
        #[arg(long)]
        details: bool,
    },
    /// Start one isolated worker per configured exit; namespaces must already be up.
    Run {
        #[arg(long)]
        config: PathBuf,
    },
    /// Read accumulated status without launching games.
    Status {
        #[arg(long)]
        config: PathBuf,
    },
    /// Inspect or manage dedicated WireGuard namespaces (up/down need root).
    Wg {
        #[command(subcommand)]
        command: WgCommand,
    },
    #[command(hide = true)]
    Worker {
        #[arg(long)]
        config: PathBuf,
        #[arg(long)]
        exit: String,
    },
}
#[derive(Subcommand)]
enum WgCommand {
    Plan {
        #[arg(long)]
        config: PathBuf,
    },
    Up {
        #[arg(long)]
        config: PathBuf,
    },
    Down {
        #[arg(long)]
        config: PathBuf,
    },
    Status {
        #[arg(long)]
        config: PathBuf,
    },
}
fn print(value: &Value) {
    println!(
        "{}",
        serde_json::to_string_pretty(value).expect("JSON output")
    );
}
#[tokio::main(worker_threads = 2)]
async fn main() {
    match run().await {
        Ok(true) => {}
        Ok(false) => std::process::exit(2),
        Err(e) => {
            eprintln!("{}", json!({"event":"error","message":format!("{e:#}")}));
            std::process::exit(1);
        }
    }
}
async fn run() -> Result<bool> {
    match Cli::parse().command {
        Command::ImportRuby { input, output } => {
            let bank = ruby_import::import(&input, &output)?;
            print(&capture_bank::summary(&bank));
            Ok(true)
        }
        Command::Export { input, output } => {
            let bank = capture_bank::export(&input, &output)?;
            print(&capture_bank::summary(&bank));
            Ok(true)
        }
        Command::Replay {
            bank,
            attempt,
            requests,
            emit_requests,
        } => {
            let index = capture_bank::load(&bank)?;
            let Some(id) = attempt else {
                print(&json!({"summary":capture_bank::summary(&index),"attempts":index.attempts}));
                return Ok(true);
            };
            let attempt = index
                .attempts
                .iter()
                .find(|a| a.id == id)
                .context("unknown bank attempt ID")?;
            if emit_requests {
                replay::emit_requests(&bank, attempt, std::io::stdout().lock())?;
                return Ok(true);
            }
            match requests {
                None => {
                    print(&replay::inspect(&bank, attempt)?);
                    Ok(true)
                }
                Some(path) if path.as_os_str() == "-" => replay::run(
                    &bank,
                    attempt,
                    std::io::stdin().lock(),
                    std::io::stdout().lock(),
                ),
                Some(path) => replay::run(
                    &bank,
                    attempt,
                    BufReader::new(fs::File::open(path)?),
                    std::io::stdout().lock(),
                ),
            }
        }
        Command::Plan { config, details } => {
            let cfg = Config::load(&config)?;
            let catalog = Catalog::load(&cfg.evidence_root)?;
            print(&catalog.plan(&cfg, details)?);
            Ok(true)
        }
        Command::Run { config } => {
            let cfg = Config::load(&config)?;
            let catalog = Catalog::load(&cfg.evidence_root)?;
            print(&catalog.plan(&cfg, false)?);
            supervisor::run(&config, &cfg).await
        }
        Command::Worker { config, exit } => {
            let cfg = Config::load(&config)?;
            let exit = cfg
                .exits
                .iter()
                .find(|e| e.id == exit)
                .context("unknown exit")?;
            supervisor::validate_worker_exit(&cfg, exit)?;
            let catalog = Catalog::load(&cfg.evidence_root)?;
            let (tx, stop) = tokio::sync::watch::channel(false);
            tokio::spawn(async move {
                if engine::shutdown_signal().await.is_ok() {
                    let _ = tx.send(true);
                }
            });
            let summary =
                engine::run_exit(&cfg, exit, &catalog, Arc::new(engine::NativeExecutor), stop)
                    .await?;
            print(&summary);
            Ok(summary["ok"] == true)
        }
        Command::Wg { command } => {
            let value = match command {
                WgCommand::Plan { config } => wireguard::plan(&Config::load(&config)?)?,
                WgCommand::Up { config } => wireguard::up(&Config::load(&config)?)?,
                WgCommand::Down { config } => wireguard::down(&Config::load(&config)?)?,
                WgCommand::Status { config } => wireguard::status(&Config::load(&config)?)?,
            };
            let ok = value.get("ok").is_none_or(|v| v == true);
            print(&value);
            Ok(ok)
        }
        Command::Status { config } => {
            let cfg = Config::load(&config)?;
            let mut exits = vec![];
            for exit in &cfg.exits {
                let dir = engine::exit_dir(&cfg, exit);
                let mut counts = std::collections::BTreeMap::<String, u64>::new();
                let events = dir.join("events.jsonl");
                if events.exists() {
                    for line in fs::read_to_string(events)?.lines() {
                        if let Ok(value) = serde_json::from_str::<Value>(line)
                            && let Some(event) = value["event"].as_str()
                        {
                            *counts.entry(event.into()).or_default() += 1;
                        }
                    }
                }
                exits.push(json!({"exit":exit.id,"events":counts,"alerts_file":dir.join("alerts.jsonl"),"directory":dir}));
            }
            print(&json!({"run_id":cfg.run_id,"exits":exits}));
            Ok(true)
        }
    }
}
