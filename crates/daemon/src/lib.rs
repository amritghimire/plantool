pub mod archive;
pub mod artifacts;
pub mod assets;
pub mod auth;
pub mod automation;
pub mod changes;
pub mod commits;
pub mod config;
mod daemon_lock;
pub mod database;
pub mod dispatcher;
pub mod events;
pub mod execution;
pub mod git;
pub mod github;
pub mod identities;
pub mod instructions;
pub mod models;
pub mod prompts;
pub mod providers;
pub mod registry;
pub mod retention;
pub mod routes;
pub mod runs;
pub mod sandbox;
pub mod security;
pub mod socket;
pub mod store;
pub mod supervision;
pub mod watcher;
pub mod worker;
pub mod workflow;

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
    let _home_lock = daemon_lock::DaemonLock::acquire(&config.home)?;
    let config = Arc::new(config);
    database::initialize(&config.home)?;
    models::router::normalize_legacy_resets(&config.home)?;
    supervision::reconcile(&config.home)?;
    database::reconcile_personal(&config.home)?;
    dispatcher::reconcile(&config.home)?;
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

    let _worker_stop = state
        .runs
        .execution
        .register_hook(state.runs.worker.clone())?;
    let dispatch_state = state.clone();
    let dispatch_job = tokio::spawn(async move {
        let mut timer = tokio::time::interval(std::time::Duration::from_secs(1));
        loop {
            timer.tick().await;
            if let Err(error) = dispatcher::runtime::tick(&dispatch_state).await {
                tracing::warn!("dispatch recovery pending: {error}");
            }
        }
    });
    let scheduler_home = config.home.clone();
    let scheduler_job = tokio::spawn(async move {
        let mut timer = tokio::time::interval(std::time::Duration::from_secs(5));
        timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            timer.tick().await;
            if let Err(error) = scheduler::tick(&scheduler_home, chrono::Utc::now()) {
                tracing::warn!("Project scheduler paused: {error}");
            }
        }
    });
    let github_home = config.home.clone();
    let github_sync = tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            if let Err(e) = github::sync::tick(&github_home).await {
                tracing::warn!("GitHub sync: {e}");
            }
        }
    });
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

    let retention_state = state.clone();
    let retention_job = tokio::spawn(async move {
        let mut timer = tokio::time::interval(std::time::Duration::from_secs(86400));
        loop {
            timer.tick().await;
            let home = retention_state.config.home.clone();
            match tokio::task::spawn_blocking(move || retention::run(&home, database::unix_now()))
                .await
            {
                Ok(Ok(report)) => {
                    for key in report.cleaned_items {
                        if let Some(session) = retention_state.registry.get_key(&key) {
                            if let Err(e) = session.reload_metadata() {
                                tracing::warn!("retention refresh: {e}");
                            }
                        }
                    }
                    for warning in report.warnings {
                        tracing::warn!("retention: {warning}");
                    }
                }
                other => tracing::warn!("retention failed: {other:?}"),
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
    // Restore an already-installed worker after listeners are serving. Fresh
    // installations/upgrades still use the owner-authorized Repair action.
    let restore_state = state.clone();
    let worker_restore = tokio::spawn(async move {
        let mut timer = tokio::time::interval(std::time::Duration::from_secs(60));
        timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            timer.tick().await;
            if sandbox::supported()
                && !restore_state.runs.execution.status().stopped
                && worker::has_installed_worker(&restore_state.config.home)
                && !restore_state.runs.worker.availability().available
            {
                if let Err(error) = restore_state.runs.worker.repair(&restore_state).await {
                    tracing::warn!("Installed worker restore deferred; will check again in 60 seconds: {error}");
                }
            }
        }
    });
    let _ = shutdown_rx.recv().await;
    worker_restore.abort();
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
    github_sync.abort();
    scheduler_job.abort();
    idle_sweep.abort();
    dispatch_job.abort();
    retention_job.abort();
    let _ = store::remove_daemon_info(&config.home);
    Ok(())
}

pub mod workspace;

pub mod scheduler;

pub mod oversight;

pub mod loop_engine;

pub mod engineering_view;

pub mod recovery;

pub mod sprint_view;

pub mod graph_recovery;

mod keychain;

/// The CLI waits for shutdown ownership before launching a replacement daemon.
#[doc(hidden)]
pub fn home_available(home: &std::path::Path) -> anyhow::Result<bool> {
    daemon_lock::DaemonLock::available(home)
}
