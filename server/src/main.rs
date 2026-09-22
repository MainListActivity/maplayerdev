use anyhow::Result;
use clap::{Parser, Subcommand};
use maplayer_server::config::Config;
use maplayer_server::discovery;
use maplayer_server::net::Server;
use maplayer_server::profiles::ProfileManager;

#[derive(Parser)]
#[command(name = "maplayer-server", about = "Maplayer host daemon")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Run the daemon (pairing closed; only allowlisted clients connect).
    Serve,
    /// Run the daemon with a pairing window open; prints a PIN + ticket.
    Pair,
    /// Print this server's endpoint id and current addr.
    Id,
    /// Manage codex account profiles.
    Profiles {
        #[command(subcommand)]
        cmd: ProfileCmd,
    },
    /// List external (user-started) sessions.
    Sessions,
    /// Allow an endpoint id without pairing (writes allowlist).
    Allow {
        /// Endpoint id of the client to authorize.
        node: String,
    },
}

#[derive(Subcommand)]
enum ProfileCmd {
    List,
    New { name: String, credential: String },
    Default { name: String },
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    let cli = Cli::parse();
    let cfg = Config::load()?;
    cfg.ensure_dirs().await?;

    match cli.cmd {
        Cmd::Serve | Cmd::Pair => {
            let server = Server::bind(cfg).await?;
            println!("endpoint id: {}", server.endpoint_id());
            println!("addr: {}", serde_json::to_string(&server.endpoint.addr())?);
            if matches!(cli.cmd, Cmd::Pair) {
                let pin = random_pin();
                server.open_pairing(pin.clone());
                let ticket = server.pair_ticket().await?;
                println!("pairing PIN: {pin}");
                println!("pairing ticket (QR payload): {}", serde_json::to_string(&ticket)?);
                println!("pairing window is OPEN — accept only the device you expect.");
            }
            server.run().await?;
        }
        Cmd::Id => {
            let server = Server::bind(cfg).await?;
            println!("{}", server.endpoint_id());
        }
        Cmd::Profiles { cmd } => {
            let pm = ProfileManager::new(&cfg);
            match cmd {
                ProfileCmd::List => {
                    let profiles = pm.list().await?;
                    let default = cfg.server_config().await.default_profile;
                    println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                        "profiles": profiles, "default": default,
                    }))?);
                }
                ProfileCmd::New { name, credential } => {
                    let r = pm.create(&name, &credential).await?;
                    println!("{}", serde_json::to_string_pretty(&r)?);
                }
                ProfileCmd::Default { name } => {
                    pm.codex_home(Some(&name), &cfg).await?;
                    let mut sc = cfg.server_config().await;
                    sc.default_profile = Some(name);
                    cfg.save_server_config(&sc).await?;
                    println!("ok");
                }
            }
        }
        Cmd::Sessions => {
            let external = discovery::external_sessions();
            println!("{}", serde_json::to_string_pretty(&external)?);
        }
        Cmd::Allow { node } => {
            let id: iroh::EndpointId = node.parse()?;
            cfg.allow(&id, None).await?;
            println!("allowed {id}");
        }
    }
    Ok(())
}

fn random_pin() -> String {
    format!("{:06}", rand::random::<u32>() % 1_000_000)
}
