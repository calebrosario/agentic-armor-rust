use agentic_armor::task::{TaskLifecycle, TaskRegistry};
use sqlx::sqlite::SqlitePoolOptions;

async fn fresh_registry_with_pool() -> (TaskRegistry, sqlx::SqlitePool) {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("in-memory pool");
    let registry = TaskRegistry::new(pool.clone());
    registry.migrate().await.expect("migrate");
    (registry, pool)
}

async fn legacy_registry_with_row() -> (TaskRegistry, sqlx::SqlitePool) {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("in-memory pool");
    sqlx::query(
        "CREATE TABLE tasks (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'pending',
            owner TEXT,
            metadata TEXT NOT NULL DEFAULT '{}',
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            updated_at TEXT NOT NULL DEFAULT (datetime('now'))
        )",
    )
    .execute(&pool)
    .await
    .expect("legacy tasks table");
    sqlx::query(
        "CREATE TABLE task_events (
            id TEXT PRIMARY KEY,
            task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
            event_type TEXT NOT NULL,
            level TEXT NOT NULL DEFAULT 'info',
            message TEXT,
            data TEXT,
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
        )",
    )
    .execute(&pool)
    .await
    .expect("legacy task_events table");
    sqlx::query("INSERT INTO tasks (id, name) VALUES ('legacy-1', 'legacy task')")
        .execute(&pool)
        .await
        .expect("legacy task row");
    sqlx::query("INSERT INTO task_events (id, task_id, event_type, message) VALUES ('e1', 'legacy-1', 'created', 'legacy event')")
        .execute(&pool)
        .await
        .expect("legacy event row");

    let registry = TaskRegistry::new(pool.clone());
    registry.migrate().await.expect("migrate legacy");
    (registry, pool)
}

async fn table_ddl(pool: &sqlx::SqlitePool, table: &str) -> String {
    sqlx::query_scalar::<_, String>("SELECT sql FROM sqlite_master WHERE type='table' AND name = ?")
        .bind(table)
        .fetch_one(pool)
        .await
        .expect("ddl")
}

#[tokio::test]
async fn fresh_migrate_creates_append_only_events() {
    let (reg, pool) = fresh_registry_with_pool().await;
    let ddl = table_ddl(&pool, "task_events").await;
    assert!(!ddl.contains("CASCADE"), "fresh schema must not cascade: {}", ddl);
}

#[tokio::test]
async fn legacy_migration_preserves_rows_and_removes_cascade() {
    let (reg, pool) = legacy_registry_with_row().await;
    let ddl = table_ddl(&pool, "task_events").await;
    assert!(!ddl.contains("CASCADE"), "legacy CASCADE must be removed, got: {}", ddl);

    let events = reg.get_logs("legacy-1", 100).await.expect("logs");
    assert_eq!(events.len(), 1, "legacy event row must survive migration");
    assert_eq!(events[0].event_type, "created");
}

#[tokio::test]
async fn legacy_migration_is_idempotent() {
    let (reg, _pool) = legacy_registry_with_row().await;
    reg.migrate().await.expect("second migrate");
    let events = reg.get_logs("legacy-1", 100).await.expect("logs");
    assert_eq!(events.len(), 1, "no duplication on re-migrate");
}

#[tokio::test]
async fn events_survive_task_deletion() {
    let (reg, _pool) = fresh_registry_with_pool().await;
    reg.create("t1", "task", None).await.expect("create");
    reg.add_event("t1", "exec_logged", "exec exit=0: echo hi").await.expect("event");

    let lifecycle = TaskLifecycle::new(std::sync::Arc::new(reg.clone()));
    lifecycle.delete_task("t1").await.expect("delete");

    let events = reg.get_logs("t1", 100).await.expect("logs");
    assert!(events.iter().any(|e| e.event_type == "exec_logged"), "exec audit must survive deletion");
    assert!(events.iter().any(|e| e.event_type == "task_deleted"), "terminal event recorded");
}

#[tokio::test]
async fn delete_missing_task_is_ok_and_writes_no_phantom_event() {
    let (reg, _pool) = fresh_registry_with_pool().await;
    let lifecycle = TaskLifecycle::new(std::sync::Arc::new(reg.clone()));
    lifecycle.delete_task("never-existed").await.expect("delete of missing task must be Ok");

    let events = reg.get_logs("never-existed", 100).await.expect("logs");
    assert!(events.is_empty(), "no phantom task_deleted event for unknown tasks");
}
