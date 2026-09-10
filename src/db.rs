use sqlx::{
    Row, SqlitePool,
    sqlite::{
        SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteRow, SqliteSynchronous,
    },
};
use std::fs;
use std::path::Path;
use std::time::Duration;

#[derive(Debug, Clone, serde::Serialize)]
pub struct ScanTask {
    pub id: String,
    pub cidr: String,
    pub scan_type: String,
    pub ports: String,
    pub timeout: f64,
    pub status: String,
    pub progress: i32,
    pub total_hosts: i32,
    pub scanned_hosts: i32,
    /// 从当前任务扫描结果中聚合的去重存活主机数，不写入 `tasks` 表。
    pub alive_hosts: i32,
    pub created_at: String,
    pub completed_at: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct HostScanResult {
    pub id: i64,
    pub task_id: String,
    pub host: String,
    pub status: String,
    pub ports: String, // 存储为 JSON 字符串如 [{"port":22,"status":"open","banner":"..."}]
    pub detail: String,
}

pub async fn init_db() -> Result<SqlitePool, sqlx::Error> {
    // 自动判断存储目录并初始化
    let db_dir = "/data";
    let db_path = if Path::new(db_dir).exists() {
        format!("{}/host-scanner.db", db_dir)
    } else {
        "./host-scanner.db".to_string()
    };

    if let Some(parent) = Path::new(&db_path).parent() {
        let _ = fs::create_dir_all(parent);
    }

    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(sqlite_options(&db_path))
        .await?;

    init_schema(&pool).await?;

    Ok(pool)
}

async fn init_schema(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    // 创建任务表
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS tasks (
            id TEXT PRIMARY KEY,
            cidr TEXT NOT NULL,
            scan_type TEXT NOT NULL,
            ports TEXT NOT NULL,
            timeout REAL NOT NULL,
            status TEXT NOT NULL,
            progress INTEGER NOT NULL DEFAULT 0,
            total_hosts INTEGER NOT NULL DEFAULT 0,
            scanned_hosts INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL,
            completed_at TEXT
        )",
    )
    .execute(pool)
    .await?;

    // 创建结果表
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS scan_results (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            task_id TEXT NOT NULL,
            host TEXT NOT NULL,
            status TEXT NOT NULL,
            ports TEXT NOT NULL,
            detail TEXT NOT NULL
        )",
    )
    .execute(pool)
    .await?;

    // 创建索引加速查询
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_scan_results_task_id ON scan_results(task_id)")
        .execute(pool)
        .await?;

    Ok(())
}

fn sqlite_options(path: &str) -> SqliteConnectOptions {
    SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal)
        .foreign_keys(true)
        .auto_vacuum(sqlx::sqlite::SqliteAutoVacuum::Incremental)
        .busy_timeout(Duration::from_secs(5))
}

fn map_task(row: SqliteRow) -> ScanTask {
    ScanTask {
        id: row.get("id"),
        cidr: row.get("cidr"),
        scan_type: row.get("scan_type"),
        ports: row.get("ports"),
        timeout: row.get("timeout"),
        status: row.get("status"),
        progress: row.get("progress"),
        total_hosts: row.get("total_hosts"),
        scanned_hosts: row.get("scanned_hosts"),
        alive_hosts: row.get("alive_hosts"),
        created_at: row.get("created_at"),
        completed_at: row.get("completed_at"),
    }
}

fn map_result(row: SqliteRow) -> HostScanResult {
    HostScanResult {
        id: row.get("id"),
        task_id: row.get("task_id"),
        host: row.get("host"),
        status: row.get("status"),
        ports: row.get("ports"),
        detail: row.get("detail"),
    }
}

pub async fn create_task(pool: &SqlitePool, task: &ScanTask) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO tasks (id, cidr, scan_type, ports, timeout, status, progress, total_hosts, scanned_hosts, created_at, completed_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
    )
    .bind(&task.id)
    .bind(&task.cidr)
    .bind(&task.scan_type)
    .bind(&task.ports)
    .bind(task.timeout)
    .bind(&task.status)
    .bind(task.progress)
    .bind(task.total_hosts)
    .bind(task.scanned_hosts)
    .bind(&task.created_at)
    .bind(&task.completed_at)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn update_task_progress(
    pool: &SqlitePool,
    id: &str,
    progress: i32,
    scanned: i32,
    status: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE tasks SET progress = ?, scanned_hosts = ?, status = ? WHERE id = ?")
        .bind(progress)
        .bind(scanned)
        .bind(status)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn complete_task(
    pool: &SqlitePool,
    id: &str,
    status: &str,
    completed_at: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE tasks SET progress = 100, status = ?, completed_at = ? WHERE id = ?")
        .bind(status)
        .bind(completed_at)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn finish_task_with_progress(
    pool: &SqlitePool,
    id: &str,
    progress: i32,
    scanned: i32,
    status: &str,
    completed_at: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE tasks SET progress = ?, scanned_hosts = ?, status = ?, completed_at = ? WHERE id = ?",
    )
        .bind(progress)
        .bind(scanned)
        .bind(status)
        .bind(completed_at)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn fail_interrupted_tasks(
    pool: &SqlitePool,
    completed_at: &str,
) -> Result<u64, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE tasks SET status = 'failed', completed_at = ? WHERE status IN ('pending', 'scanning')",
    )
    .bind(completed_at)
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

pub async fn get_task(pool: &SqlitePool, id: &str) -> Result<Option<ScanTask>, sqlx::Error> {
    let row = sqlx::query(
        "SELECT tasks.*,
                (SELECT COUNT(DISTINCT scan_results.host)
                   FROM scan_results
                  WHERE scan_results.task_id = tasks.id) AS alive_hosts
           FROM tasks
          WHERE tasks.id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(map_task))
}

pub async fn list_tasks(pool: &SqlitePool) -> Result<Vec<ScanTask>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT tasks.*,
                (SELECT COUNT(DISTINCT scan_results.host)
                   FROM scan_results
                  WHERE scan_results.task_id = tasks.id) AS alive_hosts
           FROM tasks
          ORDER BY tasks.created_at DESC",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(map_task).collect())
}

pub async fn delete_task(pool: &SqlitePool, id: &str) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM tasks WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    sqlx::query("DELETE FROM scan_results WHERE task_id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn insert_scan_results(
    pool: &SqlitePool,
    results: &[HostScanResult],
) -> Result<(), sqlx::Error> {
    if results.is_empty() {
        return Ok(());
    }

    // 使用事务批量插入
    let mut tx = pool.begin().await?;
    for res in results {
        sqlx::query(
            "INSERT INTO scan_results (task_id, host, status, ports, detail) VALUES (?, ?, ?, ?, ?)"
        )
        .bind(&res.task_id)
        .bind(&res.host)
        .bind(&res.status)
        .bind(&res.ports)
        .bind(&res.detail)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

pub async fn get_task_results(
    pool: &SqlitePool,
    task_id: &str,
) -> Result<Vec<HostScanResult>, sqlx::Error> {
    let rows = sqlx::query("SELECT * FROM scan_results WHERE task_id = ? ORDER BY host")
        .bind(task_id)
        .fetch_all(pool)
        .await?;
    Ok(rows.into_iter().map(map_result).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn test_pool() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        init_schema(&pool).await.unwrap();
        pool
    }

    fn completed_task(id: &str) -> ScanTask {
        ScanTask {
            id: id.to_string(),
            cidr: "192.0.2.0/24".to_string(),
            scan_type: "tcp".to_string(),
            ports: "22,80".to_string(),
            timeout: 1.0,
            status: "completed".to_string(),
            progress: 100,
            total_hosts: 254,
            scanned_hosts: 254,
            alive_hosts: 0,
            created_at: "2026-08-17T00:00:00Z".to_string(),
            completed_at: Some("2026-08-17T00:01:00Z".to_string()),
        }
    }

    fn alive_result(task_id: &str, host: String) -> HostScanResult {
        HostScanResult {
            id: 0,
            task_id: task_id.to_string(),
            host,
            status: "alive".to_string(),
            ports: "[]".to_string(),
            detail: "host responded".to_string(),
        }
    }

    #[tokio::test]
    async fn task_queries_count_distinct_alive_results_instead_of_scanned_hosts() {
        let pool = test_pool().await;
        let task = completed_task("task-with-results");
        create_task(&pool, &task).await.unwrap();
        let mut results = (1..=93)
            .map(|suffix| alive_result(&task.id, format!("192.0.2.{suffix}")))
            .collect::<Vec<_>>();
        results.push(alive_result(&task.id, "192.0.2.1".to_string()));
        insert_scan_results(&pool, &results).await.unwrap();

        let listed = list_tasks(&pool).await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].scanned_hosts, 254);
        assert_eq!(listed[0].alive_hosts, 93);

        let detail = get_task(&pool, &task.id).await.unwrap().unwrap();
        assert_eq!(detail.scanned_hosts, 254);
        assert_eq!(detail.alive_hosts, 93);
    }

    #[tokio::test]
    async fn task_queries_return_zero_when_no_alive_results_exist() {
        let pool = test_pool().await;
        let task = completed_task("task-without-results");
        create_task(&pool, &task).await.unwrap();

        let listed = list_tasks(&pool).await.unwrap();
        assert_eq!(listed[0].alive_hosts, 0);

        let detail = get_task(&pool, &task.id).await.unwrap().unwrap();
        assert_eq!(detail.alive_hosts, 0);
    }
}
