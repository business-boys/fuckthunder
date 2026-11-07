//! `fuckthunder` command-line client.

use std::time::Duration;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use fuckthunder_core::{MailboxName, SequenceSet};
use fuckthunder_imap::{ClientConfig, ImapClient};
use fuckthunder_imap_proto::{FetchItems, SearchQuery};

#[derive(Clone, Copy, Debug, ValueEnum)]
enum Tls {
    Implicit,
    Starttls,
    Plain,
}

#[derive(Parser, Debug)]
#[command(name = "fuckthunder", about = "IMAP + OAuth client built in safe Rust")]
struct Cli {
    /// Server host name.
    #[arg(long, env = "FUCKTHUNDER_HOST", default_value = "localhost")]
    host: String,
    /// Server port.
    #[arg(long, env = "FUCKTHUNDER_PORT")]
    port: Option<u16>,
    /// Transport security.
    #[arg(long, value_enum, default_value = "implicit")]
    tls: Tls,
    /// Account name.
    #[arg(long, env = "FUCKTHUNDER_USER", default_value = "")]
    user: String,
    /// Password (prefer the environment variable).
    #[arg(long, env = "FUCKTHUNDER_PASSWORD", default_value = "")]
    password: String,
    /// SOCKS5 proxy as `socks5://[user:password@]host:port`.
    #[arg(long, env = "FUCKTHUNDER_PROXY")]
    proxy: Option<String>,
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug, Clone)]
enum Commands {
    /// Print server capabilities.
    Capability,
    /// Authenticate and print the greeting.
    Login,
    /// List mailboxes.
    List {
        #[arg(default_value = "*")]
        pattern: String,
    },
    /// Select a mailbox and print its status.
    Select {
        #[arg(default_value = "INBOX")]
        mailbox: String,
    },
    /// Fetch message summaries.
    Fetch {
        #[arg(default_value = "1:*")]
        set: String,
        #[arg(long, default_value_t = false)]
        uid: bool,
    },
    /// Search with a raw IMAP criteria string.
    Search {
        #[arg(default_value = "ALL")]
        criteria: String,
    },
    /// Watch a mailbox with IDLE.
    Idle {
        #[arg(long, default_value_t = 30)]
        seconds: u64,
    },
    /// Build an OAuth authorization URL (and optionally run the device flow).
    Oauth {
        #[arg(long, value_enum, default_value = "google")]
        provider: ProviderArg,
        #[arg(long)]
        client_id: String,
        #[arg(long, default_value = "http://127.0.0.1:0/")]
        redirect_uri: String,
        #[arg(long, default_value_t = false)]
        device: bool,
    },
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum ProviderArg {
    Google,
    Microsoft,
    Yahoo,
}

impl From<ProviderArg> for fuckthunder_oauth::Provider {
    fn from(value: ProviderArg) -> Self {
        match value {
            ProviderArg::Google => Self::Google,
            ProviderArg::Microsoft => Self::Microsoft,
            ProviderArg::Yahoo => Self::Yahoo,
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command.clone() {
        Commands::Oauth {
            provider,
            client_id,
            redirect_uri,
            device,
        } => oauth(provider.into(), &client_id, &redirect_uri, device).await,
        other => imap_command(&cli, other).await,
    }
}

async fn imap_command(cli: &Cli, command: Commands) -> Result<()> {
    let port = cli.port.unwrap_or(match cli.tls {
        Tls::Implicit => 993,
        Tls::Starttls | Tls::Plain => 143,
    });
    let config = match cli.tls {
        Tls::Implicit => ClientConfig::tls(&cli.host),
        Tls::Starttls => ClientConfig::starttls(&cli.host),
        Tls::Plain => ClientConfig::plaintext(&cli.host, port),
    };
    let mut config = config.with_timeout(Duration::from_secs(30));
    config.port = port;
    if let Some(proxy) = &cli.proxy {
        let url = url::Url::parse(proxy).context("parse --proxy")?;
        anyhow::ensure!(url.scheme() == "socks5", "--proxy must be socks5://...");
        let host = url.host_str().context("proxy host missing")?.to_string();
        let port = url.port().unwrap_or(1080);
        config.proxy = Some(fuckthunder_imap::ProxyConfig {
            host,
            port,
            auth: if url.username().is_empty() {
                None
            } else {
                Some((
                    url.username().to_string(),
                    url.password().unwrap_or_default().to_string(),
                ))
            },
        });
    }

    let mut client = ImapClient::connect(config).await.context("connect")?;
    println!("greeting: {}", client.greeting());
    println!("capabilities: {}", client.capabilities());

    if !cli.user.is_empty() {
        client
            .login(&cli.user, &cli.password)
            .await
            .context("login")?;
        println!("authenticated as {}", cli.user);
    }

    match command {
        Commands::Capability | Commands::Login => {}
        Commands::List { pattern } => {
            for entry in client.list("", &pattern).await? {
                println!("{}\t{}", entry.name, entry.attributes.join(" "));
            }
        }
        Commands::Select { mailbox } => {
            let name = MailboxName::from_utf8(mailbox)?;
            let info = client.select(&name, false).await?;
            println!("{}", info.summary());
            println!("flags: {}", info.flags.to_parenthesised());
            if let Some(flags) = &info.permanent_flags {
                println!("permanent flags: {}", flags.to_parenthesised());
            }
        }
        Commands::Fetch { set, uid } => {
            let set: SequenceSet = set.parse()?;
            let items = FetchItems::summary();
            let messages = if uid {
                client.uid_fetch(&set, &items).await?
            } else {
                client.fetch(&set, &items).await?
            };
            for message in messages {
                let subject = message
                    .envelope
                    .as_ref()
                    .and_then(|e| e.subject.clone())
                    .unwrap_or_default();
                println!(
                    "#{}\tuid={:?}\tsize={:?}\t{}",
                    message.seq, message.uid, message.rfc822_size, subject
                );
            }
        }
        Commands::Search { criteria } => {
            let ids = client.uid_search(&SearchQuery::new()).await;
            let _ = ids;
            // The raw criteria string is validated by the protocol layer.
            let query: SearchQuery = criteria.parse().unwrap_or_else(|_| SearchQuery::all());
            println!("{:?}", client.uid_search(&query).await?);
        }
        Commands::Idle { seconds } => {
            for event in client.idle_for(Duration::from_secs(seconds)).await? {
                println!("{event:?}");
            }
        }
        Commands::Oauth { .. } => unreachable!("handled above"),
    }

    client.logout().await.ok();
    Ok(())
}

async fn oauth(
    provider: fuckthunder_oauth::Provider,
    client_id: &str,
    redirect_uri: &str,
    device: bool,
) -> Result<()> {
    let config = fuckthunder_oauth::OAuthConfig::new(provider, client_id);
    let client = fuckthunder_oauth::OAuthClient::new(config)?;
    if device {
        let device = client.device_authorization().await?;
        println!(
            "Open {} and enter {}",
            device.verification_uri, device.user_code
        );
        let tokens = client.wait_for_device_token(&device).await?;
        println!("access_token: {}", tokens.access_token);
        if let Some(refresh) = tokens.refresh_token {
            println!("refresh_token: {refresh}");
        }
    } else {
        let request = client.authorization_request(redirect_uri)?;
        println!("state: {}", request.state);
        println!("url: {}", request.url);
        if let Some(pkce) = request.pkce {
            println!("code_verifier: {}", pkce.verifier);
        }
    }
    Ok(())
}
