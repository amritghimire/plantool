pub mod archive;
pub mod artifacts;
pub mod assets;
pub mod auth;
pub mod changes;
pub mod commits;
pub mod config;
pub mod database;
pub mod events;
pub mod execution;
pub mod git;
pub mod identities;
pub mod prompts;
pub mod providers;
pub mod registry;
pub mod routes;
pub mod runs;
pub mod sandbox;
pub mod security;
pub mod socket;
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
    pub auth: Arc<auth::Auth>,
    pub registry: Arc<Registry>,
    pub config: Arc<Config>,
    pub runs: Arc<runs::RunManager>,
    pub shutdown: tokio::sync::mpsc::Sender<()>,
}

pub async fn serve(config: Config) -> anyhow::Result<()> {
    let config = Arc::new(config);
    let registry = Arc::new(Registry::load(config.home.clone())?);
    let (shutdown_tx, mut shutdown_rx) = tokio::sync::mpsc::channel::<()>(1);
    let runs = Arc::new(runs::RunManager::load(&config.home)?);
    for s in registry.all() {
        runs.mark_orphans(&s);
    }
    let state = AppState {
        auth: Arc::new(auth::Auth::new(&config.home, config.port)?),
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

    let _watcher = watcher::start(registry.clone(), config.home.clone());

    let app = routes::router(state.clone());
    let addr = SocketAddr::from(([127, 0, 0, 1], config.port));
    let listener = tokio::net::TcpListener::bind(addr).await.map_err(|error| anyhow::anyhow!("Could not start plantool on {addr}: {error}. Run `plantool daemon status` to check the existing daemon, or choose a free port with PLANTOOL_PORT."))?;
    #[cfg(unix)]
    let (unix_listener, _socket_file) = socket::bind(&config.home).await?;
    store::write_daemon_info(
        &config.home,
        &store::DaemonInfo {
            pid: std::process::id(),
            port: config.port,
            protocol: plantool_core::PROTOCOL_VERSION,
            version: config.version.clone(),
        },
    )?;
    tracing::info!(
        "plantool daemon listening on http://localhost:{}",
        config.port
    );
    let (stop_tx, stop_rx) = tokio::sync::watch::channel(false);
    let mut http_stop = stop_rx.clone();
    let http = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                let _ = http_stop.changed().await;
            })
            .await
    });
    #[cfg(unix)]
    let unix = {
        let listener = unix_listener;
        let app = socket::router(state.clone());
        let mut stop = stop_rx;
        let task = tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async move {
                    let _ = stop.changed().await;
                })
                .await
        });
        task
    };
    let _ = shutdown_rx.recv().await;
    state.runs.tokens.revoke_all();
    let _ = stop_tx.send(true);
    // A watch client must not hold the daemon open during a restart.
    let http_abort = http.abort_handle();
    if tokio::time::timeout(std::time::Duration::from_secs(2), http)
        .await
        .is_err()
    {
        http_abort.abort();
    }
    #[cfg(unix)]
    {
        let abort = unix.abort_handle();
        if tokio::time::timeout(std::time::Duration::from_secs(2), unix)
            .await
            .is_err()
        {
            abort.abort();
        }
    }
    idle_sweep.abort();
    let _ = store::remove_daemon_info(&config.home);
    Ok(())
}
