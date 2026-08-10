use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "task_status", rename_all = "lowercase")]
pub enum TaskStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
}

impl Default for TaskStatus {
    fn default() -> Self {
        TaskStatus::Pending
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub name: String,
    pub status: TaskStatus,
    pub owner: Option<String>,
    pub metadata: serde_json::Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone)]
pub struct TaskRegistry {
    pool: PgPool,
}

impl TaskRegistry {
    pub fn new(pool: PgPool) -> Self {
        TaskRegistry { pool }
    }

    pub async fn migrate(&self) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"CREATE TABLE IF NOT EXISTS tasks (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                status task_status NOT NULL DEFAULT 'pending',
                owner TEXT,
                metadata JSONB NOT NULL DEFAULT '{}',
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )"#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"CREATE TABLE IF NOT EXISTS task_events (
                id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
                task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
                event_type TEXT NOT NULL,
                level TEXT NOT NULL DEFAULT 'info',
                message TEXT,
                data JSONB,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )"#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_task_events_task_id ON task_events(task_id)")
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    pub async fn create(&self, id: &str, name: &str, owner: Option<&str>) -> Result<Task, sqlx::Error> {
        let _ = sqlx::query(
            "INSERT INTO tasks (id, name, owner) VALUES ($1, $2, $3)",
        )
        .bind(id)
        .bind(name)
        .bind(owner)
        .execute(&self.pool)
        .await?;

        self.get_by_id(id).await?.ok_or_else(|| {
            sqlx::Error::RowNotFound
        })
    }

    pub async fn get_by_id(&self, id: &str) -> Result<Option<Task>, sqlx::Error> {
        let row = sqlx::query_as::<_, TaskRow>(
            "SELECT id, name, status, owner, metadata, created_at, updated_at FROM tasks WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(Into::into))
    }

    pub async fn update_status(&self, id: &str, status: TaskStatus) -> Result<(), sqlx::Error> {
        sqlx::query(
            "UPDATE tasks SET status = $2, updated_at = NOW() WHERE id = $1",
        )
        .bind(id)
        .bind(status)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn set_container_id(&self, id: &str, container_id: &str) -> Result<(), sqlx::Error> {
        sqlx::query(
            "UPDATE tasks SET metadata = metadata || jsonb_build_object('containerId', $2), updated_at = NOW() WHERE id = $1",
        )
        .bind(id)
        .bind(container_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn get_container_id(&self, id: &str) -> Result<Option<String>, sqlx::Error> {
        let row: Option<(serde_json::Value,)> = sqlx::query_as(
            "SELECT metadata FROM tasks WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row
            .and_then(|(metadata,)| metadata.get("containerId").and_then(|v| v.as_str()).map(String::from)))
    }

    pub async fn list(&self, limit: i64) -> Result<Vec<Task>, sqlx::Error> {
        let rows = sqlx::query_as::<_, TaskRow>(
            "SELECT id, name, status, owner, metadata, created_at, updated_at FROM tasks ORDER BY created_at DESC LIMIT $1",
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.into_iter().map(Into::into).collect())
    }

    pub async fn delete(&self, id: &str) -> Result<(), sqlx::Error> {
        sqlx::query("DELETE FROM tasks WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn add_event(&self, task_id: &str, event_type: &str, message: &str) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT INTO task_events (task_id, event_type, message) VALUES ($1, $2, $3)",
        )
        .bind(task_id)
        .bind(event_type)
        .bind(message)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn get_logs(&self, task_id: &str, limit: i64) -> Result<Vec<TaskEvent>, sqlx::Error> {
        let rows = sqlx::query_as::<_, TaskEvent>(
            "SELECT id, task_id, event_type, level, message, data, created_at FROM task_events WHERE task_id = $1 ORDER BY created_at DESC LIMIT $2",
        )
        .bind(task_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }
}

#[derive(Debug, FromRow)]
struct TaskRow {
    id: String,
    name: String,
    status: TaskStatus,
    owner: Option<String>,
    metadata: serde_json::Value,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<TaskRow> for Task {
    fn from(row: TaskRow) -> Self {
        Task {
            id: row.id,
            name: row.name,
            status: row.status,
            owner: row.owner,
            metadata: row.metadata,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct TaskEvent {
    pub id: Uuid,
    pub task_id: String,
    pub event_type: String,
    pub level: String,
    pub message: Option<String>,
    pub data: Option<serde_json::Value>,
    pub created_at: DateTime<Utc>,
}

use sqlx::FromRow;
