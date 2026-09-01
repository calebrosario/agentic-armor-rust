use thiserror::Error;

#[derive(Debug, Error)]
pub enum ArmorError {
    #[error("Docker error: {0}")]
    Docker(String),

    #[error("Database error: {0}")]
    Database(String),

    #[error("Task not found: {0}")]
    TaskNotFound(String),

    #[error("Container not associated with task: {0}")]
    ContainerNotAssociated(String),

    #[error("Invalid path: {0}")]
    InvalidPath(String),

    #[error("Path restricted: must be under /tmp/, /home/opencode/, or /workspace/")]
    PathRestricted,

    #[error("Forbidden mount: {0}")]
    ForbiddenMount(String),

    #[error("Host network denied: set ALLOW_HOST_NETWORK=true to enable")]
    HostNetworkDenied,

    #[error("Invalid network mode: {0}")]
    InvalidNetworkMode(String),

    #[error("Invalid mount config: {0}")]
    InvalidMountConfig(String),

    #[error("Invalid user namespace mode: {0} — expected a lowercase runtime mode such as 'auto' (Podman) or a daemon remap name (Docker userns-remap)")]
    InvalidUsernsMode(String),

    #[error("Container creation failed: {0}")]
    ContainerCreateFailed(String),

    #[error("Docker connection failed: {0}")]
    DockerConnectionFailed(String),

    #[error("MCP error: {0}")]
    Mcp(String),
}

impl ArmorError {
    pub fn code(&self) -> &str {
        match self {
            ArmorError::TaskNotFound(_) => "TASK_NOT_FOUND",
            ArmorError::ContainerNotAssociated(_) => "CONTAINER_NOT_ASSOCIATED",
            ArmorError::InvalidPath(_) => "INVALID_PATH",
            ArmorError::PathRestricted => "PATH_RESTRICTED",
            ArmorError::ForbiddenMount(_) => "FORBIDDEN_MOUNT",
            ArmorError::HostNetworkDenied => "HOST_NETWORK_DENIED",
            ArmorError::InvalidNetworkMode(_) => "INVALID_NETWORK_MODE",
            ArmorError::InvalidMountConfig(_) => "INVALID_MOUNT_CONFIG",
            ArmorError::InvalidUsernsMode(_) => "INVALID_USERNS_MODE",
            ArmorError::ContainerCreateFailed(_) => "CONTAINER_CREATE_FAILED",
            ArmorError::DockerConnectionFailed(_) => "DOCKER_CONNECTION_FAILED",
            ArmorError::Database(_) => "DATABASE_ERROR",
            ArmorError::Docker(_) => "DOCKER_ERROR",
            ArmorError::Mcp(_) => "MCP_ERROR",
        }
    }
}

impl From<bollard::errors::Error> for ArmorError {
    fn from(e: bollard::errors::Error) -> Self {
        ArmorError::Docker(e.to_string())
    }
}

impl From<sqlx::Error> for ArmorError {
    fn from(e: sqlx::Error) -> Self {
        ArmorError::Database(e.to_string())
    }
}

impl From<serde_json::Error> for ArmorError {
    fn from(e: serde_json::Error) -> Self {
        ArmorError::Mcp(e.to_string())
    }
}

pub type ArmorResult<T> = Result<T, ArmorError>;
