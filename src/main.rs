mod audit;
mod db;
mod scan;

use axum::{
    Json, Router,
    body::Body,
    extract::{Path, State},
    http::{HeaderMap, HeaderValue, StatusCode, Uri, header},
    response::{
        IntoResponse, Response,
        sse::{Event, KeepAlive, Sse},
    },
    routing::{delete, get, post},
};
use futures_util::StreamExt;
use futures_util::stream::{self, BoxStream};
use rust_embed::RustEmbed;
use sqlx::SqlitePool;
use std::net::SocketAddr;
use tracing_subscriber::{EnvFilter, fmt};

#[derive(RustEmbed)]
#[folder = "frontend/dist/"]
struct Asset;

#[derive(serde::Serialize)]
struct HealthResponse {
    ok: bool,
}

#[derive(serde::Serialize)]
struct NetworkInfo {
    hostname: String,
    #[serde(rename = "containerIp")]
    container_ip: String,
    #[serde(rename = "defaultRoute")]
    default_route: String,
    #[serde(rename = "dnsServers")]
    dns_servers: Vec<String>,
    #[serde(rename = "capNetRaw")]
    cap_net_raw: bool,
    #[serde(rename = "networkMode")]
    network_mode: String,
}

#[derive(serde::Deserialize)]
struct CreateScanPayload {
    cidr: String,
    #[serde(rename = "scanType")]
    scan_type: String,
    ports: Option<String>,
    timeout: Option<f64>,
    #[serde(rename = "maxConcurrency")]
    max_concurrency: Option<usize>,
}

#[derive(serde::Serialize)]
struct CreateScanResponse {
    #[serde(rename = "taskId")]
    task_id: String,
}

#[derive(serde::Serialize)]
struct TaskDetailResponse {
    task: db::ScanTask,
    results: Vec<db::HostScanResult>,
}

#[derive(serde::Serialize)]
struct CancelTaskResponse {
    #[serde(rename = "taskId")]
    task_id: String,
    status: String,
}

#[tokio::main]
async fn main() {
    init_logging();

    // 初始化数据库
    let pool = match db::init_db().await {
        Ok(p) => p,
        Err(e) => {
            tracing::error!(error = %e, "database initialization failed");
            std::process::exit(1);
        }
    };
    audit::init().await;
    match db::fail_interrupted_tasks(&pool, &chrono::Local::now().to_rfc3339()).await {
        Ok(count) if count > 0 => {
            tracing::warn!(
                task_count = count,
                "marked interrupted scan tasks as failed"
            );
        }
        Ok(_) => {}
        Err(e) => {
            tracing::error!(error = %e, "failed to mark interrupted scan tasks");
        }
    }

    // 配置路由
    let app = Router::new()
        .route("/health", get(health_handler))
        .route("/api/runtime/network", get(network_handler))
        .route("/api/scan", post(create_scan_handler))
        .route("/api/tasks", get(list_tasks_handler))
        .route("/api/tasks/{task_id}", get(get_task_handler))
        .route("/api/tasks/{task_id}", delete(delete_task_handler))
        .route("/api/tasks/{task_id}/cancel", post(cancel_task_handler))
        .route("/api/tasks/{task_id}/progress", get(progress_handler))
        .fallback(static_handler)
        .with_state(pool);

    let port = std::env::args()
        .nth(1)
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(8080);

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    tracing::info!(address = %addr, "host scanner listening");

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .unwrap();
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {}
        _ = terminate => {}
    }

    scan::request_shutdown();
    tracing::info!("host scanner shutting down");
}

fn init_logging() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    fmt().with_env_filter(filter).init();
}

// 静态文件服务处理（Vue embed）
async fn static_handler(uri: Uri) -> impl IntoResponse {
    let mut path = uri.path().trim_start_matches('/').to_string();
    if path.is_empty() || path.ends_with('/') {
        path.push_str("index.html");
    }

    if path.starts_with("main/") {
        path = path.replacen("main/", "", 1);
    }

    match Asset::get(&path) {
        Some(content) => {
            let mut mime = mime_guess::from_path(&path)
                .first_or_octet_stream()
                .to_string();

            // 解决精简容器缺少 /etc/mime.types 时，导致 CSS/JS 以 octet-stream 传送从而被浏览器拒绝解析样式的 Bug
            if mime == "application/octet-stream" {
                if path.ends_with(".css") {
                    mime = "text/css".to_string();
                } else if path.ends_with(".js") {
                    mime = "application/javascript".to_string();
                } else if path.ends_with(".svg") {
                    mime = "image/svg+xml".to_string();
                } else if path.ends_with(".html") {
                    mime = "text/html; charset=utf-8".to_string();
                }
            }

            let body = Body::from(content.data.into_owned());
            Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, HeaderValue::from_str(&mime).unwrap())
                .body(body)
                .unwrap()
        }
        None => {
            // Vue 路由 fallback 到 index.html
            if path != "index.html" {
                match Asset::get("index.html") {
                    Some(content) => {
                        let body = Body::from(content.data.into_owned());
                        Response::builder()
                            .status(StatusCode::OK)
                            .header(
                                header::CONTENT_TYPE,
                                HeaderValue::from_static("text/html; charset=utf-8"),
                            )
                            .body(body)
                            .unwrap()
                    }
                    None => StatusCode::NOT_FOUND.into_response(),
                }
            } else {
                StatusCode::NOT_FOUND.into_response()
            }
        }
    }
}

// 健康检查
async fn health_handler() -> impl IntoResponse {
    Json(HealthResponse { ok: true })
}

// 获取网络环境信息
async fn network_handler() -> impl IntoResponse {
    let hostname = std::env::var("HOSTNAME").unwrap_or_else(|_| {
        std::fs::read_to_string("/proc/sys/kernel/hostname")
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|_| "unknown".to_string())
    });

    let container_ip = std::net::UdpSocket::bind("0.0.0.0:0")
        .and_then(|socket| {
            socket.connect("8.8.8.8:80")?;
            socket.local_addr()
        })
        .map(|addr| addr.ip().to_string())
        .unwrap_or_else(|_| "127.0.0.1".to_string());

    let default_route = std::fs::read_to_string("/proc/net/route")
        .ok()
        .and_then(|content| {
            for line in content.lines().skip(1) {
                let fields: Vec<&str> = line.split_whitespace().collect();
                if fields.len() >= 3 && fields[1] == "00000000" {
                    let gateway_hex = fields[2];
                    if let Ok(gateway_val) = u32::from_str_radix(gateway_hex, 16) {
                        let ip = std::net::Ipv4Addr::from(gateway_val.swap_bytes());
                        return Some(format!("{} via {}", fields[0], ip));
                    }
                }
            }
            None
        })
        .unwrap_or_else(|| "unknown".to_string());

    let mut dns_servers = Vec::new();
    if let Ok(content) = std::fs::read_to_string("/etc/resolv.conf") {
        for line in content.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() == 2 && parts[0] == "nameserver" {
                dns_servers.push(parts[1].to_string());
            }
        }
    }

    let cap_net_raw = std::fs::read_to_string("/proc/self/status")
        .ok()
        .map(|content| {
            for line in content.lines() {
                if line.starts_with("CapEff:") {
                    let mut parts = line.split_whitespace();
                    if let Some(val) = parts.nth(1).and_then(|s| u64::from_str_radix(s, 16).ok()) {
                        return (val & (1 << 13)) != 0; // CAP_NET_RAW 是第13位
                    }
                }
            }
            false
        })
        .unwrap_or(false);

    let network_mode = std::env::var("VUST_NETWORK_MODE").unwrap_or_else(|_| "bridge".to_string());

    Json(NetworkInfo {
        hostname,
        container_ip,
        default_route,
        dns_servers,
        cap_net_raw,
        network_mode,
    })
}

// 启动扫描任务
async fn create_scan_handler(
    State(pool): State<SqlitePool>,
    headers: HeaderMap,
    Json(payload): Json<CreateScanPayload>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let operation_context = audit::operation_context(&headers);
    // 验证网段
    let cidr = payload.cidr.trim().to_string();
    let net: ipnet::Ipv4Net = cidr
        .parse()
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("Invalid CIDR: {e}")))?;

    let total_hosts = net.hosts().count();
    if total_hosts == 0 {
        return Err((
            StatusCode::BAD_REQUEST,
            "The CIDR does not contain any usable host addresses".to_string(),
        ));
    }
    if total_hosts > 256 {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("Host count ({total_hosts}) exceeds the per-scan limit of 256"),
        ));
    }

    let scan_type = payload.scan_type.to_lowercase();
    if scan_type != "tcp" && scan_type != "icmp" {
        return Err((StatusCode::BAD_REQUEST, "Unsupported scan type".to_string()));
    }

    let ports_str = payload
        .ports
        .unwrap_or_else(|| "22,80,443,8080".to_string());
    let timeout = payload.timeout.unwrap_or(1.0).clamp(0.1, 5.0);
    let max_concurrency = payload.max_concurrency.unwrap_or(64).clamp(1, 256);

    // 生成任务ID
    let task_id = format!(
        "task_{}_{}",
        chrono::Local::now().format("%Y%m%d%H%M%S"),
        tokio::time::Instant::now().elapsed().as_micros() % 1000
    );

    let new_task = db::ScanTask {
        id: task_id.clone(),
        cidr,
        scan_type,
        ports: ports_str,
        timeout,
        status: "pending".to_string(),
        progress: 0,
        total_hosts: total_hosts as i32,
        scanned_hosts: 0,
        alive_hosts: 0,
        created_at: chrono::Local::now().to_rfc3339(),
        completed_at: None,
    };

    // 写入数据库
    if let Err(error) = db::create_task(&pool, &new_task).await {
        audit::emit(
            audit::bind_context(
                vust_suite_runtime::OperationEvent::builder(
                    "scan_submitted",
                    "提交扫描",
                    "Submit scan",
                    vust_suite_runtime::OperationOutcome::Failure,
                    vust_suite_runtime::OperationImpact::Error,
                ),
                operation_context.as_deref(),
            )
            .error("SCAN_CREATE_FAILED", error.to_string())
            .build()
            .expect("static scan audit event must be valid"),
        )
        .await;
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to create scan task: {error}"),
        ));
    }

    audit::emit(scan_audit_event(
        "scan_submitted",
        "提交扫描",
        "Submit scan",
        &new_task,
        vust_suite_runtime::OperationOutcome::Success,
        vust_suite_runtime::OperationImpact::Info,
        operation_context.as_deref(),
    ))
    .await;

    // 注册进度广播接收通道
    let _ = scan::register_progress_channel(&task_id);

    // 后台异步起扫描
    scan::start_background_scan(scan::BackgroundScan {
        task_id: task_id.clone(),
        cidr: new_task.cidr,
        scan_type: new_task.scan_type,
        ports: new_task.ports,
        timeout_secs: new_task.timeout,
        max_concurrency,
        pool,
        operation_context_id: operation_context,
    });

    Ok(Json(CreateScanResponse { task_id }))
}

// 获取历史任务列表
async fn list_tasks_handler(
    State(pool): State<SqlitePool>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let tasks = db::list_tasks(&pool).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to list scan tasks: {e}"),
        )
    })?;
    Ok(Json(tasks))
}

// 获取特定扫描详情及报告
async fn get_task_handler(
    Path(task_id): Path<String>,
    State(pool): State<SqlitePool>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let task = db::get_task(&pool, &task_id).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to read scan task metadata: {e}"),
        )
    })?;

    let task = match task {
        Some(t) => t,
        None => return Err((StatusCode::NOT_FOUND, "Scan task not found".to_string())),
    };

    let results = db::get_task_results(&pool, &task_id).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to read scan report results: {e}"),
        )
    })?;

    Ok(Json(TaskDetailResponse { task, results }))
}

// 删除扫描报告
async fn delete_task_handler(
    Path(task_id): Path<String>,
    State(pool): State<SqlitePool>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let operation_context = audit::operation_context(&headers);
    let _ = scan::cancel_task(&task_id);
    if let Err(error) = db::delete_task(&pool, &task_id).await {
        audit::emit(
            audit::bind_context(
                vust_suite_runtime::OperationEvent::builder(
                    "scan_deleted",
                    "删除扫描",
                    "Delete scan",
                    vust_suite_runtime::OperationOutcome::Failure,
                    vust_suite_runtime::OperationImpact::Error,
                ),
                operation_context.as_deref(),
            )
            .target(vust_suite_runtime::OperationTarget {
                kind: "scan_task".to_string(),
                id: task_id.clone(),
                display_name: None,
                ownership: None,
            })
            .task_id(task_id.clone())
            .error("SCAN_DELETE_FAILED", "Scan deletion failed")
            .build()
            .expect("static scan audit event must be valid"),
        )
        .await;
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to delete scan task: {error}"),
        ));
    }
    audit::emit(
        audit::bind_context(
            vust_suite_runtime::OperationEvent::builder(
                "scan_deleted",
                "删除扫描",
                "Delete scan",
                vust_suite_runtime::OperationOutcome::Success,
                vust_suite_runtime::OperationImpact::Warning,
            ),
            operation_context.as_deref(),
        )
        .target(vust_suite_runtime::OperationTarget {
            kind: "scan_task".to_string(),
            id: task_id.clone(),
            display_name: None,
            ownership: None,
        })
        .task_id(task_id)
        .build()
        .expect("static scan audit event must be valid"),
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

// 取消运行中的扫描任务
async fn cancel_task_handler(
    Path(task_id): Path<String>,
    State(pool): State<SqlitePool>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let operation_context = audit::operation_context(&headers);
    let task = db::get_task(&pool, &task_id).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to read task metadata: {}", e),
        )
    })?;

    let task = match task {
        Some(t) => t,
        None => return Err((StatusCode::NOT_FOUND, "Scan task not found".to_string())),
    };

    if is_terminal_task_status(&task.status) {
        return Ok(Json(CancelTaskResponse {
            task_id,
            status: task.status,
        }));
    }

    if scan::cancel_task(&task_id) {
        return Ok(Json(CancelTaskResponse {
            task_id,
            status: "canceling".to_string(),
        }));
    }

    if let Err(error) = db::finish_task_with_progress(
        &pool,
        &task_id,
        task.progress,
        task.scanned_hosts,
        "canceled",
        &chrono::Local::now().to_rfc3339(),
    )
    .await
    {
        audit::emit(
            audit::bind_context(
                vust_suite_runtime::OperationEvent::builder(
                    "scan_canceled",
                    "取消扫描",
                    "Cancel scan",
                    vust_suite_runtime::OperationOutcome::Failure,
                    vust_suite_runtime::OperationImpact::Error,
                ),
                operation_context.as_deref(),
            )
            .target(vust_suite_runtime::OperationTarget {
                kind: "scan_task".to_string(),
                id: task_id.clone(),
                display_name: Some(task.cidr.clone()),
                ownership: None,
            })
            .task_id(task_id.clone())
            .error("SCAN_CANCEL_FAILED", "Scan cancellation failed")
            .build()
            .expect("static scan audit event must be valid"),
        )
        .await;
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to cancel scan task: {error}"),
        ));
    }
    audit::emit(scan_audit_event(
        "scan_canceled",
        "取消扫描",
        "Cancel scan",
        &task,
        vust_suite_runtime::OperationOutcome::Canceled,
        vust_suite_runtime::OperationImpact::Info,
        operation_context.as_deref(),
    ))
    .await;

    Ok(Json(CancelTaskResponse {
        task_id,
        status: "canceled".to_string(),
    }))
}

fn scan_audit_event(
    code: &str,
    zh_cn: &str,
    en_us: &str,
    task: &db::ScanTask,
    outcome: vust_suite_runtime::OperationOutcome,
    impact: vust_suite_runtime::OperationImpact,
    operation_context_id: Option<&str>,
) -> vust_suite_runtime::OperationEvent {
    audit::bind_context(
        vust_suite_runtime::OperationEvent::builder(code, zh_cn, en_us, outcome, impact),
        operation_context_id,
    )
    .target(vust_suite_runtime::OperationTarget {
        kind: "scan_task".to_string(),
        id: task.id.clone(),
        display_name: Some(task.cidr.clone()),
        ownership: None,
    })
    .task_id(task.id.clone())
    .parameter(
        "hostCount",
        vust_suite_runtime::ParameterValue::Number(f64::from(task.total_hosts)),
    )
    .build()
    .expect("static scan audit event must be valid")
}

// SSE (Server-Sent Events) 实时进度推送
async fn progress_handler(
    Path(task_id): Path<String>,
    State(pool): State<SqlitePool>,
) -> Result<
    Sse<
        axum::response::sse::KeepAliveStream<
            BoxStream<'static, Result<Event, std::convert::Infallible>>,
        >,
    >,
    (StatusCode, String),
> {
    let task = db::get_task(&pool, &task_id).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to read task progress: {}", e),
        )
    })?;
    let task = match task {
        Some(t) => t,
        None => return Err((StatusCode::NOT_FOUND, "Scan task not found".to_string())),
    };

    let task_finished = is_terminal_task_status(&task.status);

    // 优先回放任务事件历史，避免客户端在任务启动后建立 SSE 时丢失早期主机状态。
    if let Some((history, rx)) = scan::subscribe_progress(&task_id) {
        let replay = stream::iter(history.into_iter().map(|update| {
            Ok::<Event, std::convert::Infallible>(Event::default().json_data(&update).unwrap())
        }));

        if task_finished {
            return Ok(Sse::new(replay.boxed()).keep_alive(KeepAlive::default()));
        }

        let live = stream::unfold(rx, move |mut rx| async move {
            match rx.recv().await {
                Ok(update) => {
                    let event = Event::default().json_data(&update).unwrap();
                    Some((Ok(event), rx))
                }
                Err(_) => None,
            }
        });
        return Ok(Sse::new(replay.chain(live).boxed()).keep_alive(KeepAlive::default()));
    }

    // 已结束且事件历史已过期时，仍返回数据库中的最终状态。
    if task_finished {
        let ev = Event::default()
            .json_data(scan::ScanProgressUpdate {
                task_id: task.id,
                progress: task.progress,
                scanned_hosts: task.scanned_hosts,
                total_hosts: task.total_hosts,
                current_host: "finished".to_string(),
                status: task.status,
                host_status: None,
                open_ports: Vec::new(),
                host_result: None,
            })
            .unwrap();
        let s = stream::once(async { Ok(ev) }).boxed();
        return Ok(Sse::new(s).keep_alive(KeepAlive::default()));
    }

    // 运行中但信道异常缺失时返回数据库快照，前端可据此进入最终同步流程。
    let event = Event::default()
        .json_data(scan::ScanProgressUpdate {
            task_id: task.id,
            progress: task.progress,
            scanned_hosts: task.scanned_hosts,
            total_hosts: task.total_hosts,
            current_host: "finished".to_string(),
            status: task.status,
            host_status: None,
            open_ports: Vec::new(),
            host_result: None,
        })
        .unwrap();
    let fallback = stream::once(async { Ok(event) }).boxed();
    Ok(Sse::new(fallback).keep_alive(KeepAlive::default()))
}

fn is_terminal_task_status(status: &str) -> bool {
    matches!(status, "completed" | "failed" | "canceled")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_event_contains_task_summary_without_scan_configuration() {
        let task = db::ScanTask {
            id: "task-1".to_string(),
            cidr: "192.0.2.0/24".to_string(),
            scan_type: "tcp".to_string(),
            ports: "22,80".to_string(),
            timeout: 1.0,
            status: "pending".to_string(),
            progress: 0,
            total_hosts: 254,
            scanned_hosts: 0,
            alive_hosts: 0,
            created_at: "2026-08-04T00:00:00Z".to_string(),
            completed_at: None,
        };
        let event = scan_audit_event(
            "scan_submitted",
            "提交扫描",
            "Submit scan",
            &task,
            vust_suite_runtime::OperationOutcome::Success,
            vust_suite_runtime::OperationImpact::Info,
            Some("context-1"),
        );
        let value = serde_json::to_value(event).unwrap();
        assert_eq!(value["taskId"], "task-1");
        assert_eq!(value["operationContextId"], "context-1");
        assert!(value.to_string().find("22,80").is_none());
    }
}
