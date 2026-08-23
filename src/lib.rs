pub mod config;
pub mod docker;
pub mod error;
pub mod mcp;
pub mod task;

pub use config::{Config, RuntimeChoice};
pub use docker::{
    ArmorContainerConfig, BollardRuntime, ContainerRuntime, ContainerStats, DockerManager,
    ExecRequest, ExecResult, Mount, NetworkConfig,
};
pub use error::{ArmorError, ArmorResult};
pub use task::{Task, TaskEvent, TaskLifecycle, TaskRegistry, TaskStatus};
