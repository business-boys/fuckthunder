//! Headless batch account checker.
//!
//! Reads an `email:password` file and checks every account, optionally
//! through a proxy, printing the verdict for each row.

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result};
use clap::Parser;
use fuckthunder_gui::{check_accounts, parse_accounts, parse_proxy, CheckStatus, SessionOptions};

/// Checks IMAP accounts from an `email:password` file.
#[derive(Parser, Debug)]
#[command(name = "fuckthunder-check", about = "Batch IMAP account checker")]
struct Cli {
    /// Path to the accounts file, one `email:password` per line.
    accounts: PathBuf,
    /// `socks5://[user:pass@]host:port` or `http://[user:pass@]host:port`.
    #[arg(long)]
    proxy: Option<String>,
    /// Parallel checks.
    #[arg(long, default_value_t = 10)]
    concurrency: usize,
    /// Per-operation timeout, seconds.
    #[arg(long, default_value_t = 30)]
    timeout: u64,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let input = std::fs::read_to_string(&cli.accounts)
        .with_context(|| format!("cannot read {}", cli.accounts.display()))?;
    let accounts = parse_accounts(&input);
    anyhow::ensure!(!accounts.is_empty(), "no valid email:password rows");

    let proxy = cli
        .proxy
        .as_deref()
        .map(parse_proxy)
        .transpose()
        .context("invalid --proxy URL")?;
    let options = SessionOptions {
        proxy,
        timeout: Duration::from_secs(cli.timeout),
    };

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let indexed: Vec<_> = accounts.into_iter().enumerate().collect();
    let total = indexed.len();
    let checker = tokio::spawn(check_accounts(indexed, options, cli.concurrency, tx));

    let mut done = 0usize;
    while let Some(result) = rx.recv().await {
        done += 1;
        let verdict = match &result.status {
            CheckStatus::Valid { messages } => format!("VALID ({messages} msgs)"),
            CheckStatus::Invalid { reason } => format!("INVALID: {reason}"),
            CheckStatus::Error { reason } => format!("ERROR: {reason}"),
            CheckStatus::Idle | CheckStatus::Checking => continue,
        };
        println!("[{done}/{total}] #{} {verdict}", result.index);
    }
    checker.await?;
    Ok(())
}
