mod config;
mod db;
mod http;
mod saves;

use std::sync::Arc;

use anyhow::Context;
use config::Config;
use http::AppState;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "wayfinder_server=info,tower_http=info".into()),
        )
        .init();

    let config = Config::from_env().map_err(anyhow::Error::msg)?;
    let bind = config.bind;
    let mode = config.mode;
    let save_dir = config.save_dir.clone();
    let data_dir = config.data_dir.clone();

    let db = db::open_pool(&config.data_dir)
        .await
        .context("could not prepare the SQLite database")?;

    let app = http::router(AppState {
        config: Arc::new(config),
        db,
    });
    let listener = tokio::net::TcpListener::bind(bind)
        .await
        .with_context(|| format!("could not bind {bind}"))?;

    tracing::info!(%bind, ?mode, ?save_dir, ?data_dir, "Wayfinder Slate listening");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("server error")?;
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    tracing::info!("shutting down");
}
