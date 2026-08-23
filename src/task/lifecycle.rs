use crate::error::{ArmorError, ArmorResult};
use crate::task::registry::{TaskRegistry, TaskStatus};
use std::sync::Arc;

pub struct TaskLifecycle {
    registry: Arc<TaskRegistry>,
}

impl TaskLifecycle {
    pub fn new(registry: Arc<TaskRegistry>) -> Self {
        TaskLifecycle { registry }
    }

    pub async fn create_task(&self, id: &str, name: &str, owner: Option<&str>) -> ArmorResult<crate::task::registry::Task> {
        self.registry.create(id, name, owner).await.map_err(|e| {
            ArmorError::Database(e.to_string())
        })?;

        if let Err(e) = self.registry.add_event(id, "created", &format!("Task '{}' created", name)).await {
            tracing::warn!("AUDIT WRITE FAILED for task {} (created): {} — audit trail is incomplete", id, e);
        }

        self.registry.get_by_id(id).await.map_err(|e| ArmorError::Database(e.to_string()))?
            .ok_or_else(|| ArmorError::TaskNotFound(id.into()))
    }

    pub async fn cancel_task(&self, id: &str) -> ArmorResult<crate::task::registry::Task> {
        self.registry.update_status(id, TaskStatus::Cancelled).await.map_err(|e| {
            ArmorError::Database(e.to_string())
        })?;
        if let Err(e) = self.registry.add_event(id, "cancelled", "Task cancelled").await {
            tracing::warn!("AUDIT WRITE FAILED for task {} (cancelled): {} — audit trail is incomplete", id, e);
        }
        self.get_task(id).await
    }

    pub async fn delete_task(&self, id: &str) -> ArmorResult<()> {
        let existed = self
            .registry
            .get_by_id(id)
            .await
            .map_err(|e| ArmorError::Database(e.to_string()))?
            .is_some();
        if existed {
            if let Err(e) = self.registry.add_event(id, "task_deleted", "Task deleted (events retained for audit)").await {
                tracing::warn!("AUDIT WRITE FAILED for task {} (task_deleted): {} — audit trail is incomplete", id, e);
            }
        }
        self.registry.delete(id).await.map_err(|e| ArmorError::Database(e.to_string()))
    }

    pub async fn get_task(&self, id: &str) -> ArmorResult<crate::task::registry::Task> {
        self.registry.get_by_id(id).await
            .map_err(|e| ArmorError::Database(e.to_string()))?
            .ok_or_else(|| ArmorError::TaskNotFound(id.into()))
    }

    pub async fn get_container_id(&self, id: &str) -> ArmorResult<String> {
        let _task = self.get_task(id).await?;
        self.registry.get_container_id(id).await
            .map_err(|e| ArmorError::Database(e.to_string()))?
            .ok_or_else(|| ArmorError::ContainerNotAssociated(id.into()))
    }
}
