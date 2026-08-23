use agentic_armor::{
    ArmorError, BollardRuntime, Config, ContainerRuntime, TaskLifecycle, TaskRegistry,
};
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

    let _ = std::fs::create_dir_all("./data");

    let pool = sqlx::SqlitePool::connect("sqlite://./data/agentic_armor.db?mode=rwc&busy_timeout=5000")
        .await
        .map_err(|e| ArmorError::Database(e.to_string()))?;

    let registry = Arc::new(TaskRegistry::new(pool));
    registry.migrate().await.map_err(|e| ArmorError::Database(e.to_string()))?;
    info!("Database ready: {}", config.database_url);

    let lifecycle = Arc::new(TaskLifecycle::new(registry.clone()));

    agentic_armor::mcp::start(config, runtime, registry, lifecycle).await?;

    Ok(())
}
