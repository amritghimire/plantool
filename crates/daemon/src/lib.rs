pub mod archive;
pub mod assets;
pub mod changes;
pub mod commits;
pub mod config;
pub mod events;
pub mod git;
pub mod prompts;
pub mod providers;
pub mod registry;
pub mod routes;
pub mod runs;
pub mod security;
pub mod store;
pub mod watcher;

use std::net::SocketAddr;
use std::sync::Arc;

pub use config::Config;
pub use registry::Registry;

pub fn init_tracing() {
    use tracing_subscriber::EnvFilter;
    let filter = EnvFilter::try_from_env("PLANTOOL_LOG").unwrap_or_else(|_| EnvFilter::new("info"));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .try_init();
}

#[derive(Clone)]
pub struct AppState {
    pub registry: Arc<Registry>,
    pub config: Arc<Config>,
    pub runs: Arc<runs::RunManager>,
    pub shutdown: tokio::sync::mpsc::Sender<()>,
}

pub async fn serve(config: Config) -> anyhow::Result<()> {
    let config = Arc::new(config);
    let registry = Arc::new(Registry::load(config.home.clone())?);
    let (shutdown_tx, mut shutdown_rx) = tokio::sync::mpsc::channel::<()>(1);
    let runs = Arc::new(runs::RunManager::default());
    for s in registry.all() {
        runs.mark_orphans(&s);
        runs.settle_paused_milestone(&s).await;
    }
    let state = AppState {
        registry: registry.clone(),
        config: config.clone(),
        runs,
        shutdown: shutdown_tx,
    };

    let idle_state = state.clone();
    let idle_sweep = tokio::spawn(async move {
        let mut timer = tokio::time::interval(std::time::Duration::from_secs(30));
        loop {
            timer.tick().await;
            for session in idle_state.registry.all() {
                let minutes = git::git(
                    session.session().cwd(),
                    &["config", "--get", "plantool.idleMinutes"],
                )
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(30);
                idle_state.runs.sweep_idle(&session, minutes).await;
            }
        }
    });

    let _watcher = watcher::start(registry.clone(), config.home.clone(), state.runs.clone());

    let app = routes::router(state.clone());
    let wildcard = config.bind.filter(|ip| ip.is_unspecified());
    let addr = SocketAddr::new(
        wildcard.unwrap_or(std::net::Ipv4Addr::LOCALHOST.into()),
        config.port,
    );
    let listener = tokio::net::TcpListener::bind(addr).await.map_err(|error| anyhow::anyhow!("Could not start plantool on {addr}: {error}. Run `plantool daemon status` to check the existing daemon, or choose a free port with PLANTOOL_PORT."))?;
    let extra_listener = match config.bind.filter(|ip| !ip.is_unspecified()) {
        Some(ip) => {
            let extra_addr = SocketAddr::new(ip, config.port);
            let extra_listener =
                tokio::net::TcpListener::bind(extra_addr)
                    .await
                    .map_err(|error| {
                        anyhow::anyhow!(
                            "Could not listen on PLANTOOL_BIND address {extra_addr}: {error}"
                        )
                    })?;
            tracing::info!("plantool daemon also listening on http://{extra_addr}");
            Some(extra_listener)
        }
        None => None,
    };
    store::write_daemon_info(
        &config.home,
        &store::DaemonInfo {
            pid: std::process::id(),
            port: config.port,
            protocol: plantool_core::PROTOCOL_VERSION,
            version: config.version.clone(),
        },
    )?;
    tracing::info!("plantool daemon listening on http://{addr}");
    let extra = extra_listener.map(|extra_listener| {
        let extra_app = app.clone();
        tokio::spawn(async move {
            let _ = axum::serve(extra_listener, extra_app).await;
        })
    });
    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            let _ = shutdown_rx.recv().await;
            tracing::info!("shutdown requested");
        })
        .await?;
    idle_sweep.abort();
    if let Some(extra) = extra {
        extra.abort();
    }
    let _ = store::remove_daemon_info(&config.home);
    Ok(())
}
