use std::time::Duration;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use wayfinder_control::{ControlClient, DEFAULT_ADDR};

#[derive(Debug, Parser)]
#[command(
    name = "wayfinder-agent",
    version,
    about = "Local telemetry agent for Wayfinder Slate"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Read one snapshot from Savage Wind's control server and exit.
    Probe {
        /// Control server address.
        #[arg(long, default_value = DEFAULT_ADDR)]
        addr: String,
        /// Print one combined JSON object instead of a human summary.
        #[arg(long)]
        json: bool,
    },
    /// Poll the control server once per second until interrupted.
    Watch {
        #[arg(long, default_value = DEFAULT_ADDR)]
        addr: String,
        #[arg(long, default_value_t = 1000)]
        interval_ms: u64,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "wayfinder_agent=info".into()),
        )
        .init();

    match Cli::parse().command {
        Command::Probe { addr, json } => probe(&addr, json).await,
        Command::Watch { addr, interval_ms } => {
            watch(&addr, Duration::from_millis(interval_ms.max(100))).await
        }
    }
}

async fn probe(addr: &str, json: bool) -> Result<()> {
    let client = ControlClient::from_addr(addr)
        .with_context(|| format!("invalid control address {addr:?}"))?;
    let mut connection = client.connect().await?;

    let version = connection.version().await?;
    let pos = connection.pos().await?;
    let state = connection.state().await?;

    if json {
        println!(
            "{}",
            serde_json::json!({
                "version": version,
                "pos": pos,
                "state": state,
            })
        );
    } else {
        println!("Savage Wind {}", version);
        println!(
            "Link: x={:.2} y={:.2} z={:.2} stamina={} place={}",
            pos.x,
            pos.y,
            pos.z,
            pos.stamina
                .map(|value| format!("{value:.2}"))
                .unwrap_or_else(|| "?".into()),
            pos.place.as_deref().unwrap_or("?")
        );
        println!(
            "State: frame={} uptime={:.1}s fps={} hour={}",
            state.frame,
            state.uptime_s,
            state
                .fps
                .map(|value| format!("{value:.1}"))
                .unwrap_or_else(|| "?".into()),
            state
                .hour
                .map(|value| format!("{value:.2}"))
                .unwrap_or_else(|| "?".into())
        );
    }
    Ok(())
}

async fn watch(addr: &str, interval: Duration) -> Result<()> {
    let client = ControlClient::from_addr(addr)
        .with_context(|| format!("invalid control address {addr:?}"))?;
    let mut connection = client.connect().await?;
    tracing::info!(%addr, "watching Savage Wind; press Ctrl+C to stop");

    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                tracing::info!("stopping");
                return Ok(());
            }
            _ = tokio::time::sleep(interval) => {
                match connection.pos().await {
                    Ok(pos) => tracing::info!(
                        x = pos.x,
                        y = pos.y,
                        z = pos.z,
                        stamina = pos.stamina,
                        place = pos.place.as_deref(),
                        "position"
                    ),
                    Err(error) => {
                        tracing::warn!(%error, "control request failed; reconnecting");
                        tokio::time::sleep(Duration::from_secs(2)).await;
                        connection = client.connect().await?;
                    }
                }
            }
        }
    }
}
