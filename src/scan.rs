use crate::db::{
    HostScanResult, complete_task, finish_task_with_progress, insert_scan_results,
    update_task_progress,
};
use futures_util::stream::{FuturesUnordered, StreamExt};
use ipnet::Ipv4Net;
use serde::Serialize;
use sqlx::SqlitePool;
use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::{Semaphore, broadcast, watch};

#[derive(Debug, Clone, Serialize)]
pub struct ScanProgressUpdate {
    pub task_id: String,
    pub progress: i32,
    pub scanned_hosts: i32,
    pub total_hosts: i32,
    pub current_host: String,
    pub status: String,
    pub host_status: Option<String>,
    pub open_ports: Vec<u16>,
    pub host_result: Option<ScanHostResult>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PortScanDetail {
    pub port: u16,
    pub status: String, // "open", "closed"
    pub banner: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ScanHostResult {
    pub host: String,
    pub status: String,
    pub ports: Vec<PortScanDetail>,
    pub detail: String,
}

struct ProgressChannel {
    sender: broadcast::Sender<ScanProgressUpdate>,
    history: Vec<ScanProgressUpdate>,
}

// 全局任务进度广播信道与短期事件历史。
type BroadcastMap = Mutex<HashMap<String, ProgressChannel>>;
type CancelMap = Mutex<HashMap<String, watch::Sender<bool>>>;
static PROGRESS_CHANNELS: OnceLock<BroadcastMap> = OnceLock::new();
static TASK_CANCEL_SIGNALS: OnceLock<CancelMap> = OnceLock::new();
static SHUTDOWN_SIGNAL: OnceLock<watch::Sender<bool>> = OnceLock::new();
const DB_RESULT_FLUSH_BATCH_SIZE: usize = 32;
const DB_PROGRESS_FLUSH_INTERVAL: i32 = 8;

fn get_progress_channels() -> &'static BroadcastMap {
    PROGRESS_CHANNELS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn get_cancel_signals() -> &'static CancelMap {
    TASK_CANCEL_SIGNALS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn shutdown_sender() -> &'static watch::Sender<bool> {
    SHUTDOWN_SIGNAL.get_or_init(|| {
        let (sender, _) = watch::channel(false);
        sender
    })
}

pub fn subscribe_shutdown() -> watch::Receiver<bool> {
    shutdown_sender().subscribe()
}

pub fn request_shutdown() {
    let _ = shutdown_sender().send(true);
    let mut map = get_progress_channels().lock().unwrap();
    map.clear();
    let mut cancel_map = get_cancel_signals().lock().unwrap();
    cancel_map.clear();
}

pub fn subscribe_progress(
    task_id: &str,
) -> Option<(
    Vec<ScanProgressUpdate>,
    broadcast::Receiver<ScanProgressUpdate>,
)> {
    let map = get_progress_channels().lock().unwrap();
    map.get(task_id)
        .map(|channel| (channel.history.clone(), channel.sender.subscribe()))
}

pub fn register_progress_channel(task_id: &str) -> broadcast::Receiver<ScanProgressUpdate> {
    let mut map = get_progress_channels().lock().unwrap();
    let (tx, rx) = broadcast::channel(100);
    map.insert(
        task_id.to_string(),
        ProgressChannel {
            sender: tx,
            history: Vec::new(),
        },
    );
    rx
}

pub fn remove_progress_channel(task_id: &str) {
    let mut map = get_progress_channels().lock().unwrap();
    map.remove(task_id);
}

pub fn cancel_task(task_id: &str) -> bool {
    let map = get_cancel_signals().lock().unwrap();
    let Some(sender) = map.get(task_id) else {
        return false;
    };
    sender.send(true).is_ok()
}

fn register_cancel_signal(task_id: &str) -> watch::Receiver<bool> {
    let mut map = get_cancel_signals().lock().unwrap();
    let (sender, receiver) = watch::channel(false);
    map.insert(task_id.to_string(), sender);
    receiver
}

fn remove_cancel_signal(task_id: &str) {
    let mut map = get_cancel_signals().lock().unwrap();
    map.remove(task_id);
}

fn publish_progress(task_id: &str, update: ScanProgressUpdate) {
    let mut map = get_progress_channels().lock().unwrap();
    let Some(channel) = map.get_mut(task_id) else {
        return;
    };
    channel.history.push(update.clone());
    if channel.history.len() > 1024 {
        channel.history.remove(0);
    }
    let _ = channel.sender.send(update);
}

// 高性能并发扫描逻辑
pub struct BackgroundScan {
    pub task_id: String,
    pub cidr: String,
    pub scan_type: String,
    pub ports: String,
    pub timeout_secs: f64,
    pub max_concurrency: usize,
    pub pool: SqlitePool,
    pub operation_context_id: Option<String>,
}

pub fn start_background_scan(request: BackgroundScan) {
    let BackgroundScan {
        task_id,
        cidr,
        scan_type,
        ports: ports_str,
        timeout_secs,
        max_concurrency,
        pool,
        operation_context_id,
    } = request;
    let cancel = register_cancel_signal(&task_id);
    tokio::spawn(async move {
        let channel_exists = {
            let map = get_progress_channels().lock().unwrap();
            map.contains_key(&task_id)
        };
        if !channel_exists {
            remove_cancel_signal(&task_id);
            return;
        }

        // 解析网段
        let net: Ipv4Net = match cidr.parse() {
            Ok(n) => n,
            Err(_) => {
                let _ = complete_task(
                    &pool,
                    &task_id,
                    "failed",
                    &chrono::Local::now().to_rfc3339(),
                )
                .await;
                emit_terminal_audit(
                    &task_id,
                    &cidr,
                    "failed",
                    0,
                    operation_context_id.as_deref(),
                )
                .await;
                remove_cancel_signal(&task_id);
                return;
            }
        };

        let hosts: Vec<Ipv4Addr> = net.hosts().collect();
        let total_hosts = hosts.len();

        if total_hosts == 0 {
            let _ = complete_task(
                &pool,
                &task_id,
                "completed",
                &chrono::Local::now().to_rfc3339(),
            )
            .await;
            emit_terminal_audit(
                &task_id,
                &cidr,
                "completed",
                0,
                operation_context_id.as_deref(),
            )
            .await;
            remove_cancel_signal(&task_id);
            return;
        }

        // 解析端口列表
        let ports: Vec<u16> = ports_str
            .split(',')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .filter_map(|s| s.parse::<u16>().ok())
            .collect();

        // 使用信号量限制最大并发率
        let semaphore = Arc::new(Semaphore::new(max_concurrency));
        let pool = Arc::new(pool);
        let scan_type = Arc::new(scan_type);
        let ports = Arc::new(ports);

        let mut scanned_count = 0;
        let mut last_progress_flush_scanned = 0;
        let mut pending_results: Vec<HostScanResult> = Vec::new();
        let mut hosts = hosts.into_iter();
        let mut running = FuturesUnordered::new();
        let shutdown = subscribe_shutdown();

        loop {
            while running.len() < max_concurrency && !is_stop_requested(&shutdown, &cancel) {
                let Some(host) = hosts.next() else {
                    break;
                };
                let sem = semaphore.clone();
                let host_str = host.to_string();
                let task_id = task_id.clone();
                let scan_type = scan_type.clone();
                let ports = ports.clone();

                publish_progress(
                    &task_id,
                    ScanProgressUpdate {
                        task_id: task_id.clone(),
                        progress: ((scanned_count as f32 / total_hosts as f32) * 100.0) as i32,
                        scanned_hosts: scanned_count,
                        total_hosts: total_hosts as i32,
                        current_host: host_str.clone(),
                        status: "scanning".to_string(),
                        host_status: Some("scanning".to_string()),
                        open_ports: Vec::new(),
                        host_result: None,
                    },
                );

                running.push(tokio::spawn(async move {
                    let _permit = sem.acquire().await.ok()?;
                    let result =
                        scan_single_host(&host_str, &scan_type, &ports, timeout_secs).await;
                    Some((host_str, result))
                }));
            }

            if running.is_empty() {
                break;
            }

            let Some(joined) = running.next().await else {
                break;
            };
            let Ok(Some((host_str, result))) = joined else {
                continue;
            };
            let (host_status, open_ports) = match &result {
                Some(result) => {
                    let open_ports = result
                        .ports
                        .iter()
                        .filter(|port| port.status == "open")
                        .map(|port| port.port)
                        .collect::<Vec<_>>();
                    let host_status = if open_ports.is_empty() {
                        "alive-no-port"
                    } else {
                        "alive-with-port"
                    };
                    (host_status.to_string(), open_ports)
                }
                None => ("offline".to_string(), Vec::new()),
            };

            let host_result = result.clone();
            if let Some(r) = result {
                pending_results.push(HostScanResult {
                    id: 0,
                    task_id: task_id.clone(),
                    host: r.host.clone(),
                    status: r.status.clone(),
                    ports: serde_json::to_string(&r.ports).unwrap_or_default(),
                    detail: r.detail.clone(),
                });
                if pending_results.len() >= DB_RESULT_FLUSH_BATCH_SIZE {
                    flush_scan_results(&pool, &mut pending_results).await;
                }
            }

            scanned_count += 1;
            let progress = ((scanned_count as f32 / total_hosts as f32) * 100.0) as i32;
            if should_flush_task_progress(
                scanned_count,
                total_hosts as i32,
                last_progress_flush_scanned,
            ) {
                if let Err(err) =
                    update_task_progress(&pool, &task_id, progress, scanned_count, "scanning").await
                {
                    tracing::error!(
                        error = %err,
                        task_id = %task_id,
                        progress,
                        scanned_hosts = scanned_count,
                        "failed to update scan progress"
                    );
                } else {
                    last_progress_flush_scanned = scanned_count;
                }
            }

            publish_progress(
                &task_id,
                ScanProgressUpdate {
                    task_id: task_id.clone(),
                    progress,
                    scanned_hosts: scanned_count,
                    total_hosts: total_hosts as i32,
                    current_host: host_str,
                    status: "scanning".to_string(),
                    host_status: Some(host_status),
                    open_ports,
                    host_result,
                },
            );
        }

        flush_scan_results(&pool, &mut pending_results).await;

        let total = total_hosts as i32;
        let is_canceled = *cancel.borrow();
        let final_status = if is_canceled { "canceled" } else { "completed" };
        let final_progress = if is_canceled {
            ((scanned_count as f32 / total_hosts as f32) * 100.0) as i32
        } else {
            100
        };
        let final_scanned_hosts = if is_canceled { scanned_count } else { total };

        if is_canceled {
            let _ = finish_task_with_progress(
                &pool,
                &task_id,
                final_progress,
                final_scanned_hosts,
                final_status,
                &chrono::Local::now().to_rfc3339(),
            )
            .await;
        } else {
            let _ = complete_task(
                &pool,
                &task_id,
                final_status,
                &chrono::Local::now().to_rfc3339(),
            )
            .await;
        }
        emit_terminal_audit(
            &task_id,
            &cidr,
            final_status,
            final_scanned_hosts,
            operation_context_id.as_deref(),
        )
        .await;

        // 发送最后一条进度广播
        publish_progress(
            &task_id,
            ScanProgressUpdate {
                task_id: task_id.clone(),
                progress: final_progress,
                scanned_hosts: final_scanned_hosts,
                total_hosts: total,
                current_host: "finished".to_string(),
                status: final_status.to_string(),
                host_status: None,
                open_ports: Vec::new(),
                host_result: None,
            },
        );

        // 为首次连接和短线重连保留事件历史，随后在独立任务中释放广播资源。
        let finished_task_id = task_id.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(60)).await;
            remove_progress_channel(&finished_task_id);
            remove_cancel_signal(&finished_task_id);
        });
    });
}

async fn emit_terminal_audit(
    task_id: &str,
    cidr: &str,
    status: &str,
    scanned_hosts: i32,
    operation_context_id: Option<&str>,
) {
    let (code, zh_cn, en_us, outcome, impact) = match status {
        "completed" => (
            "scan_succeeded",
            "扫描完成",
            "Scan completed",
            vust_suite_runtime::OperationOutcome::Success,
            vust_suite_runtime::OperationImpact::Info,
        ),
        "canceled" => (
            "scan_canceled",
            "取消扫描",
            "Cancel scan",
            vust_suite_runtime::OperationOutcome::Canceled,
            vust_suite_runtime::OperationImpact::Info,
        ),
        _ => (
            "scan_failed",
            "扫描失败",
            "Scan failed",
            vust_suite_runtime::OperationOutcome::Failure,
            vust_suite_runtime::OperationImpact::Error,
        ),
    };
    crate::audit::emit(
        crate::audit::bind_context(
            vust_suite_runtime::OperationEvent::builder(code, zh_cn, en_us, outcome, impact),
            operation_context_id,
        )
        .target(vust_suite_runtime::OperationTarget {
            kind: "scan_task".to_string(),
            id: task_id.to_string(),
            display_name: Some(cidr.to_string()),
            ownership: None,
        })
        .task_id(task_id)
        .parameter(
            "scannedHosts",
            vust_suite_runtime::ParameterValue::Number(f64::from(scanned_hosts)),
        )
        .build()
        .expect("static scan audit event must be valid"),
    )
    .await;
}

fn is_stop_requested(shutdown: &watch::Receiver<bool>, cancel: &watch::Receiver<bool>) -> bool {
    *shutdown.borrow() || *cancel.borrow()
}

fn should_flush_task_progress(scanned_count: i32, total_hosts: i32, last_flushed: i32) -> bool {
    scanned_count >= total_hosts || scanned_count - last_flushed >= DB_PROGRESS_FLUSH_INTERVAL
}

async fn flush_scan_results(pool: &SqlitePool, results: &mut Vec<HostScanResult>) {
    if results.is_empty() {
        return;
    }

    if let Err(err) = insert_scan_results(pool, results).await {
        tracing::error!(
            error = %err,
            result_count = results.len(),
            "failed to insert scan results"
        );
        return;
    }
    results.clear();
}

async fn scan_single_host(
    host: &str,
    scan_type: &str,
    ports: &[u16],
    timeout_secs: f64,
) -> Option<ScanHostResult> {
    let timeout_dur = Duration::from_secs_f64(timeout_secs);

    if scan_type == "icmp" {
        let (status, detail) = ping_host(host, timeout_secs).await;
        if status == "alive" {
            Some(ScanHostResult {
                host: host.to_string(),
                status,
                ports: Vec::new(),
                detail,
            })
        } else {
            None // ICMP 未响应的直接过滤不入库，符合精简资产发现
        }
    } else {
        // TCP 端口扫描及指纹获取
        let mut port_results = Vec::new();

        for &port in ports {
            let addr_str = format!("{}:{}", host, port);
            let socket_addr: Result<SocketAddr, _> = addr_str.parse();

            if let Ok(addr) = socket_addr {
                match tokio::time::timeout(timeout_dur, TcpStream::connect(addr)).await {
                    Ok(Ok(mut _stream)) => {
                        // 连接成功，尝试获取 Banner 指纹
                        let banner = grab_banner_with_timeout(host, port, timeout_dur).await;
                        port_results.push(PortScanDetail {
                            port,
                            status: "open".to_string(),
                            banner,
                        });
                    }
                    Ok(Err(e)) => {
                        if e.kind() == std::io::ErrorKind::ConnectionRefused {
                            // TCP RST 表示端口关闭，同时说明目标路径有响应。
                            port_results.push(PortScanDetail {
                                port,
                                status: "closed".to_string(),
                                banner: None,
                            });
                        }
                    }
                    Err(_) => {} // 超时
                }
            }
        }

        tcp_host_result(host, port_results)
    }
}

/// 根据开放和关闭端口组装 TCP 存活主机结果。
fn tcp_host_result(host: &str, port_results: Vec<PortScanDetail>) -> Option<ScanHostResult> {
    let open_count = port_results
        .iter()
        .filter(|port| port.status == "open")
        .count();
    let closed_count = port_results
        .iter()
        .filter(|port| port.status == "closed")
        .count();
    if open_count == 0 && closed_count == 0 {
        return None;
    }

    let open_port_label = if open_count == 1 { "port" } else { "ports" };
    let closed_port_label = if closed_count == 1 { "port" } else { "ports" };
    let detail = match (open_count, closed_count) {
        (open_count, 0) => format!("Found {open_count} open TCP {open_port_label}"),
        (0, closed_count) => format!(
            "Found {closed_count} closed TCP {closed_port_label} (RST received; the target path responded)"
        ),
        (open_count, closed_count) => format!(
            "Found {open_count} open TCP {open_port_label}; {closed_count} closed TCP {closed_port_label}"
        ),
    };
    Some(ScanHostResult {
        host: host.to_string(),
        status: "alive".to_string(),
        ports: port_results,
        detail,
    })
}

// 异步并发 Ping 逻辑（调用系统进程）
async fn ping_host(host: &str, timeout_secs: f64) -> (String, String) {
    // 限制超时时间至少为 1s
    let timeout_val = format!("{}", timeout_secs.max(1.0).round() as i32);

    let output = tokio::process::Command::new("ping")
        .args(["-c", "1", "-W", &timeout_val, host])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .await;

    match output {
        Ok(out) => {
            if out.status.success() {
                let stdout = String::from_utf8_lossy(&out.stdout);
                let lines: Vec<&str> = stdout.trim().lines().collect();
                let last_line = lines
                    .last()
                    .cloned()
                    .unwrap_or("Ping succeeded; host is online");
                ("alive".to_string(), last_line.to_string())
            } else {
                let stderr = String::from_utf8_lossy(&out.stderr);
                if stderr.contains("Operation not permitted") {
                    (
                        "error".to_string(),
                        "Ping failed: CAP_NET_RAW capability is required".to_string(),
                    )
                } else {
                    (
                        "timeout".to_string(),
                        "ICMP request timed out without a response".to_string(),
                    )
                }
            }
        }
        Err(e) => ("error".to_string(), format!("Failed to execute ping: {e}")),
    }
}

async fn grab_banner_with_timeout(host: &str, port: u16, timeout_dur: Duration) -> Option<String> {
    let addr_str = format!("{}:{}", host, port);
    let addr: SocketAddr = addr_str.parse().ok()?;

    if port == 80 || port == 8080 || port == 3000 || port == 8000 {
        let banner = grab_http_banner(addr, timeout_dur).await;
        if banner.is_some() {
            return banner;
        }
    }
    grab_tcp_banner(addr, timeout_dur).await
}

async fn grab_http_banner(addr: SocketAddr, timeout_dur: Duration) -> Option<String> {
    let mut stream = TcpStream::connect(addr).await.ok()?;
    let request = "GET / HTTP/1.0\r\nHost: localhost\r\nUser-Agent: vust-host-scanner\r\nConnection: close\r\n\r\n";
    let _ = tokio::time::timeout(timeout_dur, stream.write_all(request.as_bytes())).await;

    let mut response = Vec::new();
    let mut buf = [0u8; 1024];

    let read_result = tokio::time::timeout(timeout_dur, async {
        while let Ok(n) = stream.read(&mut buf).await {
            if n == 0 {
                break;
            }
            response.extend_from_slice(&buf[..n]);
            if response.len() > 8192 {
                break;
            } // 防止大文件把内存撑爆
        }
    })
    .await;

    if read_result.is_err() || response.is_empty() {
        return None;
    }

    let text = String::from_utf8_lossy(&response);

    // 解析 Server 头
    let mut server = None;
    for line in text.lines() {
        if line.to_lowercase().starts_with("server:") {
            server = Some(line["server:".len()..].trim().to_string());
            break;
        }
    }

    // 解析 HTML Title
    let mut title = None;
    let text_lower = text.to_lowercase();
    if let (Some(start_idx), Some(end_idx)) =
        (text_lower.find("<title>"), text_lower.find("</title>"))
        && end_idx > start_idx + 7
    {
        title = Some(text[start_idx + 7..end_idx].trim().to_string());
    }

    match (server, title) {
        (Some(s), Some(t)) => Some(format!("HTTP Title: \"{}\" | Server: {}", t, s)),
        (None, Some(t)) => Some(format!("HTTP Title: \"{}\"", t)),
        (Some(s), None) => Some(format!("HTTP Server: {}", s)),
        _ => None,
    }
}

async fn grab_tcp_banner(addr: SocketAddr, timeout_dur: Duration) -> Option<String> {
    let mut stream = TcpStream::connect(addr).await.ok()?;
    let mut buf = [0u8; 512];

    // 等待服务自动吐出 Banner（如 SSH 建立连接会自动吐版本号）
    let n = tokio::time::timeout(timeout_dur, stream.read(&mut buf))
        .await
        .ok()?
        .ok()?;
    if n > 0 {
        let banner = String::from_utf8_lossy(&buf[..n]).trim().to_string();
        // 过滤非打印字符，保留有用的文本
        let cleaned: String = banner
            .chars()
            .filter(|c| c.is_ascii_graphic() || c.is_whitespace())
            .collect();
        if !cleaned.is_empty() {
            return Some(cleaned);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{PortScanDetail, ScanHostResult, ScanProgressUpdate, tcp_host_result};

    fn port(port: u16, status: &str) -> PortScanDetail {
        PortScanDetail {
            port,
            status: status.to_string(),
            banner: None,
        }
    }

    #[test]
    fn closed_tcp_ports_are_preserved_for_report_verification() {
        let closed_ports = [22, 80, 443, 3389, 8080]
            .into_iter()
            .map(|value| port(value, "closed"))
            .collect();

        let result = tcp_host_result("192.0.2.10", closed_ports).unwrap();

        assert_eq!(result.ports.len(), 5);
        assert!(result.ports.iter().all(|port| port.status == "closed"));
        assert_eq!(
            result.detail,
            "Found 5 closed TCP ports (RST received; the target path responded)"
        );
    }

    #[test]
    fn mixed_tcp_results_keep_open_and_closed_port_statuses() {
        let result =
            tcp_host_result("192.0.2.11", vec![port(22, "open"), port(80, "closed")]).unwrap();

        assert_eq!(result.ports, vec![port(22, "open"), port(80, "closed")]);
        assert_eq!(result.detail, "Found 1 open TCP port; 1 closed TCP port");
    }

    #[test]
    fn tcp_result_requires_an_open_or_closed_response() {
        assert!(tcp_host_result("192.0.2.12", Vec::new()).is_none());
    }

    #[test]
    fn progress_update_serializes_the_complete_host_result() {
        let update = ScanProgressUpdate {
            task_id: "task-1".to_string(),
            progress: 50,
            scanned_hosts: 1,
            total_hosts: 2,
            current_host: "192.0.2.13".to_string(),
            status: "scanning".to_string(),
            host_status: Some("alive-with-port".to_string()),
            open_ports: vec![22],
            host_result: Some(ScanHostResult {
                host: "192.0.2.13".to_string(),
                status: "alive".to_string(),
                ports: vec![PortScanDetail {
                    port: 22,
                    status: "open".to_string(),
                    banner: Some("SSH-2.0-OpenSSH".to_string()),
                }],
                detail: "Found 1 open TCP port".to_string(),
            }),
        };

        let json = serde_json::to_value(update).unwrap();
        assert_eq!(json["host_result"]["host"], "192.0.2.13");
        assert_eq!(json["host_result"]["ports"][0]["status"], "open");
        assert_eq!(json["host_result"]["ports"][0]["banner"], "SSH-2.0-OpenSSH");
        assert_eq!(json["host_result"]["detail"], "Found 1 open TCP port");
    }
}
