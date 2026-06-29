//! 集成测试：健康检查端点。
//!
//! 约定（契约）：GET /healthz 返回 200，Body 为纯文本 "ok"。
//! 这是服务存活探针的最小契约，后续探针/liveness 扩展在此之上叠加。

use axum::body::to_bytes;
use axum::http::{Request, StatusCode};
use tower::ServiceExt; // oneshot

// 直接复用 bin 内构建 router 的入口。为便于测试，router 暴露为 pub。
// bin crate 通过 `pub fn app_router()` 提供；测试经 `server::app_router` 取用。
// 但 bin crate 默认无 lib target —— 改为直接构造一个等价 router 进行黑盒测试。
async fn healthz_ok(app: axum::Router) {
    let res = app
        .oneshot(Request::builder().uri("/healthz").body(axum::body::Body::empty()).unwrap())
        .await
        .expect("请求未返回响应");
    assert_eq!(res.status(), StatusCode::OK, "/healthz 应返回 200");
    let bytes = to_bytes(res.into_body(), 1024).await.expect("读取 body 失败");
    assert_eq!(bytes.as_ref(), b"ok", "/healthz body 应为纯文本 \"ok\"");
}

#[tokio::test]
async fn healthz_returns_ok_via_app_router() {
    // 通过 bin 提供的 lib 入口取 router（见 src/lib.rs 的 app_router）。
    healthz_ok(server::app_router()).await;
}
