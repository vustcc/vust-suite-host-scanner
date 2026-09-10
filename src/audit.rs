//! 套件语义操作事件上报；Agent 不可用时只记录本地告警。

use axum::http::HeaderMap;
use std::sync::{Arc, OnceLock};
use vust_suite_runtime::{OperationEvent, RuntimeClient};

static CLIENT: OnceLock<Arc<RuntimeClient>> = OnceLock::new();

/// 从 Agent 代理注入的请求头读取可信操作上下文。
pub fn operation_context(headers: &HeaderMap) -> Option<String> {
    vust_suite_runtime::operation_context_from_header(
        headers
            .get(vust_suite_runtime::OPERATION_CONTEXT_HEADER)
            .and_then(|value| value.to_str().ok()),
    )
}

/// 将可选可信上下文绑定到事件构建器。
pub fn bind_context(
    builder: vust_suite_runtime::OperationEventBuilder,
    context_id: Option<&str>,
) -> vust_suite_runtime::OperationEventBuilder {
    match context_id {
        Some(value) => builder.operation_context_id(value),
        None => builder,
    }
}

/// 初始化 Agent Runtime 客户端；独立运行模式允许缺少运行时描述。
pub async fn init() {
    match RuntimeClient::from_environment("operation-logs.write").await {
        Ok(client) => {
            let _ = CLIENT.set(Arc::new(client));
        }
        Err(error) => tracing::warn!(%error, "operation audit runtime is unavailable"),
    }
}

/// 尽力提交事件，不改变已经完成的扫描业务结果。
pub async fn emit(event: OperationEvent) {
    let Some(client) = CLIENT.get() else {
        return;
    };
    if let Err(error) = client.submit_operation_event(&event).await {
        tracing::error!(event_id = %event.event_id, %error, "operation audit event was not accepted");
    }
}
