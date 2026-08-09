use tracing::info;

use agentic_armor::{
    ArmorContainerConfig, BollardRuntime, Config, ContainerRuntime, ExecRequest,
};
use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_target(false)
        .with_env_filter("agentic_armor=info")
        .init();

    let config = Arc::new(Config::default());

    let runtime = BollardRuntime::auto_detect(config.clone())?;
    info!("Container runtime: {}", runtime.runtime_name());

    runtime.ping().await?;
    info!("Connection OK");

    let container_config = ArmorContainerConfig {
        name: "armor-test".into(),
        image: "alpine:latest".into(),
        command: Some(vec!["sleep".into(), "infinity".into()]),
        ..Default::default()
    };

    info!("Creating container...");
    let id = runtime.create_container(&container_config).await?;

    runtime.start_container(&id).await?;
    info!("Container started: {}", id);

    info!("Running exec...");
    let result = runtime
        .exec_in_container(
            &id,
            &ExecRequest {
                command: vec!["echo".into(), "hello from container".into()],
                timeout_ms: Some(10_000),
                ..Default::default()
            },
        )
        .await?;

    info!("Exit code: {}", result.exit_code);
    info!("Stdout: {}", result.stdout.trim());

    runtime.destroy_container(&id).await?;
    info!("Cleaned up. Done!");

    Ok(())
}
