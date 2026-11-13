//! Maestro — GitHub Projects kanban → team of specialised Claude Code agents.
//!
//! Env:
//!   MAESTRO_ADDR  listen address        (default 127.0.0.1:7878)
//!   MAESTRO_DATA  data directory        (default ./maestro-data)

mod agents;
mod api;
mod app;
mod claude;
mod db;
mod engine;
mod git;
mod mcp;
mod github;
mod model;

use app::App;
use parking_lot::Mutex;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{broadcast, Notify};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // .env in the working directory (secrets, MAESTRO_ADDR, MAESTRO_DATA...)
    let dotenv = dotenvy::dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "maestro=info".into()))
        .init();

    if let Some(p) = dotenv {
        tracing::info!("loaded {}", p.display());
    }
    let data = PathBuf::from(std::env::var("MAESTRO_DATA").unwrap_or_else(|_| "maestro-data".into()));
    std::fs::create_dir_all(&data)?;
    let data = data.canonicalize()?;
    let addr = std::env::var("MAESTRO_ADDR").unwrap_or_else(|_| "127.0.0.1:7878".into());

    let db = db::Db::open(&data.join("maestro.db"))?;
    let lib = agents::Library::new(&data)?;
    let (tx, _) = broadcast::channel(4096);
    let app = Arc::new(App {
        db,
        lib,
        data: data.clone(),
        events: tx,
        cancels: Mutex::new(HashMap::new()),
        busy: Mutex::new(HashSet::new()),
        project: Mutex::new(None),
        wake: Notify::new(),
        last_sync: Mutex::new(None),
        repo_locks: Mutex::new(HashMap::new()),
        base_url: format!("http://{}", addr.replace("0.0.0.0", "127.0.0.1").replace("[::]", "[::1]")),
        mcp_created: Mutex::new(HashMap::new()),
    });

    tokio::spawn(engine::sync_loop(app.clone()));
    tokio::spawn(engine::scheduler_loop(app.clone()));

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("Maestro on http://{addr}  (data: {})", data.display());
    axum::serve(listener, api::router(app)).with_graceful_shutdown(async {
        let _ = tokio::signal::ctrl_c().await;
    }).await?;
    Ok(())
}
