use agentic_armor::{
    ArmorError, BollardRuntime, Config, ContainerRuntime, TaskLifecycle, TaskRegistry,
};
use std::str::FromStr;
use std::sync::Arc;
use tracing::{error, info};

#[tokio::main]
async fn main() -> Result<(), ArmorError> {
    let _ = dotenvy::dotenv();

    tracing_subscriber::fmt()
        .with_target(false)
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "agentic_armor=info,mcp_sdk=info".into()),
        )
        .init();

    let config = Arc::new(Config::default());

    let runtime: Arc<dyn ContainerRuntime> = Arc::new(BollardRuntime::auto_detect(config.clone())?);
    info!("Container runtime: {}", runtime.runtime_name());
    if let Err(e) = runtime.ping().await {
        error!(
            "Cannot reach the {} daemon: {}. \
             If this is a permissions issue, add the current user to the docker group \
             (sudo usermod -aG docker $USER, then re-login) or set DOCKER_SOCKET/PODMAN_SOCKET.",
            runtime.runtime_name(),
            e
        );
        return Err(e);
    }

    if let Err(e) = agentic_armor::config::validate_database_url(&config.database_url) {
        return Err(ArmorError::Database(e));
    }

    if let Err(e) = std::fs::create_dir_all("./data") {
        tracing::warn!("Could not create ./data directory: {}", e);
    }
    let db_options = sqlx::sqlite::SqliteConnectOptions::from_str(&config.database_url)
        .map_err(|e| ArmorError::Database(e.to_string()))?
        .create_if_missing(true)
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
        .synchronous(sqlx::sqlite::SqliteSynchronous::Normal)
        .busy_timeout(std::time::Duration::from_millis(5000));
    let pool = sqlx::SqlitePool::connect_with(db_options)
        .await
        .map_err(|e| ArmorError::Database(e.to_string()))?;

    let registry = Arc::new(TaskRegistry::new(pool));
    registry
        .migrate()
        .await
        .map_err(|e| ArmorError::Database(e.to_string()))?;
    info!("Database ready: {}", config.database_url);

    let lifecycle = Arc::new(TaskLifecycle::new(registry.clone()));

    agentic_armor::mcp::start(config, runtime, registry, lifecycle).await?;

    Ok(())
}
