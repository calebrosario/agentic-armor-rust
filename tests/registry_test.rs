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
    let (_reg, pool) = fresh_registry_with_pool().await;
    let ddl = table_ddl(&pool, "task_events").await;
    assert!(
        !ddl.contains("CASCADE"),
        "fresh schema must not cascade: {}",
        ddl
    );
}

#[tokio::test]
async fn legacy_migration_preserves_rows_and_removes_cascade() {
    let (reg, pool) = legacy_registry_with_row().await;
    let ddl = table_ddl(&pool, "task_events").await;
    assert!(
        !ddl.contains("CASCADE"),
        "legacy CASCADE must be removed, got: {}",
        ddl
    );

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
    reg.add_event("t1", "exec_logged", "exec exit=0: echo hi")
        .await
        .expect("event");

    let lifecycle = TaskLifecycle::new(std::sync::Arc::new(reg.clone()));
    lifecycle.delete_task("t1").await.expect("delete");

    let events = reg.get_logs("t1", 100).await.expect("logs");
    assert!(
        events.iter().any(|e| e.event_type == "exec_logged"),
        "exec audit must survive deletion"
    );
    assert!(
        events.iter().any(|e| e.event_type == "task_deleted"),
        "terminal event recorded"
    );
}

#[tokio::test]
async fn delete_missing_task_is_ok_and_writes_no_phantom_event() {
    let (reg, _pool) = fresh_registry_with_pool().await;
    let lifecycle = TaskLifecycle::new(std::sync::Arc::new(reg.clone()));
    lifecycle
        .delete_task("never-existed")
        .await
        .expect("delete of missing task must be Ok");

    let events = reg.get_logs("never-existed", 100).await.expect("logs");
    assert!(
        events.is_empty(),
        "no phantom task_deleted event for unknown tasks"
    );
}

#[tokio::test]
async fn count_active_counts_only_pending_and_running() {
    let (registry, pool) = fresh_registry_with_pool().await;
    for (id, status) in [
        ("a", "pending"),
        ("b", "running"),
        ("c", "cancelled"),
        ("d", "completed"),
        ("e", "failed"),
    ] {
        sqlx::query("INSERT INTO tasks (id, name, status) VALUES ($1, 't', $2)")
            .bind(id)
            .bind(status)
            .execute(&pool)
            .await
            .expect("insert");
    }
    assert_eq!(
        registry.count_active().await.unwrap(),
        2,
        "cancelled/completed/failed must not consume the cap"
    );
}

#[tokio::test]
async fn set_container_id_errors_when_the_task_row_vanished() {
    let (registry, _pool) = fresh_registry_with_pool().await;
    assert!(
        registry.set_container_id("ghost", "abc").await.is_err(),
        "a zero-row update must error — Ok(()) here is what let the create/delete race orphan a running container"
    );
}

#[tokio::test]
async fn fresh_events_have_monotonic_seq() {
    let (registry, _pool) = fresh_registry_with_pool().await;
    registry
        .create("seq-task", "seq probe", None)
        .await
        .expect("create");
    for i in 0..3 {
        registry
            .add_event("seq-task", "info", &format!("event {}", i))
            .await
            .expect("add event");
    }
    let logs = registry.get_logs("seq-task", 10).await.expect("logs");
    assert_eq!(logs.len(), 3);
    assert!(
        logs[0].seq > logs[1].seq && logs[1].seq > logs[2].seq,
        "seq must be strictly monotonic in insertion order, got {:?}",
        logs.iter().map(|e| e.seq).collect::<Vec<_>>()
    );
    assert!(logs[0].seq >= 1, "seq starts at 1");
}

#[tokio::test]
async fn v1_events_table_upgraded_with_seq_preserving_rows() {
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
    .expect("tasks table");
    sqlx::query(
        "CREATE TABLE task_events (
            id TEXT PRIMARY KEY,
            task_id TEXT NOT NULL,
            event_type TEXT NOT NULL,
            level TEXT NOT NULL DEFAULT 'info',
            message TEXT,
            data TEXT,
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
        )",
    )
    .execute(&pool)
    .await
    .expect("v1 task_events");
    sqlx::query("INSERT INTO tasks (id, name) VALUES ('t1', 'legacy')")
        .execute(&pool)
        .await
        .expect("task row");
    for i in 0..3 {
        sqlx::query("INSERT INTO task_events (id, task_id, event_type, message) VALUES ($1, 't1', 'info', $2)")
            .bind(format!("e{}", i))
            .bind(format!("legacy {}", i))
            .execute(&pool)
            .await
            .expect("event row");
    }
    let registry = TaskRegistry::new(pool.clone());
    registry.migrate().await.expect("migrate upgrades v1");
    let ddl = table_ddl(&pool, "task_events").await;
    assert!(
        ddl.contains("AUTOINCREMENT"),
        "upgraded table must carry AUTOINCREMENT seq"
    );
    let logs = registry.get_logs("t1", 10).await.expect("logs");
    assert_eq!(logs.len(), 3, "all pre-migration rows survive");
    assert!(
        logs.windows(2).all(|w| w[0].seq > w[1].seq),
        "replayed rows get monotonic seq"
    );
    registry.migrate().await.expect("second migrate");
    let logs2 = registry.get_logs("t1", 10).await.expect("logs2");
    assert_eq!(logs2.len(), 3, "idempotent rebuild keeps rows");
}

#[tokio::test]
async fn user_version_records_schema_v2() {
    let (_registry, pool) = fresh_registry_with_pool().await;
    let version: i32 = sqlx::query_scalar("PRAGMA user_version")
        .fetch_one(&pool)
        .await
        .expect("user_version");
    assert_eq!(version, 2, "migrate must stamp user_version=2");
}

#[tokio::test]
async fn mark_running_moves_status_from_pending_to_running() {
    let (reg, _pool) = fresh_registry_with_pool().await;
    let lifecycle = TaskLifecycle::new(std::sync::Arc::new(reg.clone()));
    lifecycle
        .create_task("t-run", "task", None)
        .await
        .expect("create");
    assert_eq!(
        reg.get_by_id("t-run").await.unwrap().unwrap().status,
        "pending",
        "new tasks start pending"
    );

    lifecycle.mark_running("t-run").await.expect("mark running");
    assert_eq!(
        reg.get_by_id("t-run").await.unwrap().unwrap().status,
        "running",
        "mark_running must move the registry status so task_list stops lying"
    );
}
