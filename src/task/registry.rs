use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum TaskStatus {
    #[default]
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
}

impl std::fmt::Display for TaskStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", format!("{:?}", self).to_lowercase())
    }
}

impl std::str::FromStr for TaskStatus {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "pending" => Ok(TaskStatus::Pending),
            "running" => Ok(TaskStatus::Running),
            "completed" => Ok(TaskStatus::Completed),
            "failed" => Ok(TaskStatus::Failed),
            "cancelled" => Ok(TaskStatus::Cancelled),
            _ => Err(format!("Unknown status: {}", s)),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub name: String,
    pub status: String,
    pub owner: Option<String>,
    pub metadata: serde_json::Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskEvent {
    pub id: String,
    pub task_id: String,
    pub event_type: String,
    pub level: String,
    pub message: Option<String>,
    pub data: Option<serde_json::Value>,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone)]
pub struct TaskRegistry {
    pool: SqlitePool,
}

impl TaskRegistry {
    pub fn new(pool: SqlitePool) -> Self {
        TaskRegistry { pool }
    }

    pub async fn migrate(&self) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"CREATE TABLE IF NOT EXISTS tasks (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'pending',
                owner TEXT,
                metadata TEXT NOT NULL DEFAULT '{}',
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at TEXT NOT NULL DEFAULT (datetime('now'))
            )"#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"CREATE TABLE IF NOT EXISTS task_events (
                id TEXT PRIMARY KEY,
                task_id TEXT NOT NULL,
                event_type TEXT NOT NULL,
                level TEXT NOT NULL DEFAULT 'info',
                message TEXT,
                data TEXT,
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            )"#,
        )
        .execute(&self.pool)
        .await?;

        let legacy: bool = sqlx::query_scalar::<_, String>(
            "SELECT sql FROM sqlite_master WHERE type='table' AND name='task_events'",
        )
        .fetch_optional(&self.pool)
        .await?
        .map(|s| s.contains("CASCADE"))
        .unwrap_or(false);
        if legacy {
            sqlx::query(
                r#"BEGIN;
                CREATE TABLE task_events_new (
                    id TEXT PRIMARY KEY,
                    task_id TEXT NOT NULL,
                    event_type TEXT NOT NULL,
                    level TEXT NOT NULL DEFAULT 'info',
                    message TEXT,
                    data TEXT,
                    created_at TEXT NOT NULL DEFAULT (datetime('now'))
                );
                INSERT INTO task_events_new SELECT id, task_id, event_type, level, message, data, created_at FROM task_events;
                DROP TABLE task_events;
                ALTER TABLE task_events_new RENAME TO task_events;
                COMMIT;"#,
            )
            .execute(&self.pool)
            .await?;
        }

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_task_events_task_id ON task_events(task_id)")
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    pub async fn create(
        &self,
        id: &str,
        name: &str,
        owner: Option<&str>,
    ) -> Result<Task, sqlx::Error> {
        sqlx::query("INSERT INTO tasks (id, name, owner) VALUES ($1, $2, $3)")
            .bind(id)
            .bind(name)
            .bind(owner)
            .execute(&self.pool)
            .await?;

        self.get_by_id(id).await?.ok_or(sqlx::Error::RowNotFound)
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
        sqlx::query("UPDATE tasks SET status = $2, updated_at = datetime('now') WHERE id = $1")
            .bind(id)
            .bind(status.to_string())
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn set_container_id(&self, id: &str, container_id: &str) -> Result<(), sqlx::Error> {
        sqlx::query("UPDATE tasks SET metadata = json_set(metadata, '$.containerId', $2), updated_at = datetime('now') WHERE id = $1")
            .bind(id)
            .bind(container_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn get_container_id(&self, id: &str) -> Result<Option<String>, sqlx::Error> {
        let row: Option<(String,)> = sqlx::query_as("SELECT metadata FROM tasks WHERE id = $1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;

        Ok(row.and_then(|(metadata_str,)| {
            serde_json::from_str::<serde_json::Value>(&metadata_str)
                .ok()
                .and_then(|v| {
                    v.get("containerId")
                        .and_then(|c| c.as_str())
                        .map(String::from)
                })
        }))
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

    pub async fn add_event(
        &self,
        task_id: &str,
        event_type: &str,
        message: &str,
    ) -> Result<(), sqlx::Error> {
        let event_id = Uuid::new_v4().to_string();
        sqlx::query(
            "INSERT INTO task_events (id, task_id, event_type, message) VALUES ($1, $2, $3, $4)",
        )
        .bind(&event_id)
        .bind(task_id)
        .bind(event_type)
        .bind(message)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn get_logs(&self, task_id: &str, limit: i64) -> Result<Vec<TaskEvent>, sqlx::Error> {
        let rows = sqlx::query_as::<_, TaskEventRow>(
            "SELECT id, task_id, event_type, level, message, data, created_at FROM task_events WHERE task_id = $1 ORDER BY created_at DESC LIMIT $2",
        )
        .bind(task_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.into_iter().map(Into::into).collect())
    }
}

#[derive(Debug, sqlx::FromRow)]
struct TaskRow {
    id: String,
    name: String,
    status: String,
    owner: Option<String>,
    metadata: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<TaskRow> for Task {
    fn from(row: TaskRow) -> Self {
        let metadata: serde_json::Value =
            serde_json::from_str(&row.metadata).unwrap_or(serde_json::json!({}));
        Task {
            id: row.id,
            name: row.name,
            status: row.status,
            owner: row.owner,
            metadata,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}

#[derive(Debug, sqlx::FromRow)]
struct TaskEventRow {
    id: String,
    task_id: String,
    event_type: String,
    level: String,
    message: Option<String>,
    data: Option<String>,
    created_at: DateTime<Utc>,
}

impl From<TaskEventRow> for TaskEvent {
    fn from(row: TaskEventRow) -> Self {
        let data = row.data.and_then(|s| serde_json::from_str(&s).ok());
        TaskEvent {
            id: row.id,
            task_id: row.task_id,
            event_type: row.event_type,
            level: row.level,
            message: row.message,
            data,
            created_at: row.created_at,
        }
    }
}
