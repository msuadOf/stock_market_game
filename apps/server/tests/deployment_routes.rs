#![cfg(feature = "web-ui")]

use axum::body::{to_bytes, Body};
use axum::http::{header, Method, Request, StatusCode};
use server::deployment::{deployment_router, Services};
use std::path::PathBuf;
use tower::ServiceExt;

struct WebFixture {
    directory: PathBuf,
}

impl WebFixture {
    fn new() -> Self {
        let directory = std::env::current_dir()
            .unwrap()
            .join("apps/server/.tmp")
            .join(uuid::Uuid::new_v4().to_string());
        std::fs::create_dir_all(directory.join("assets")).unwrap();
        std::fs::write(
            directory.join("index.html"),
            b"<!doctype html><title>Game</title>",
        )
        .unwrap();
        for (name, bytes) in [
            ("app.js", b"export const game = true;".as_slice()),
            ("app.css", b"body{color:red}".as_slice()),
            ("engine.wasm", b"\0asm\x01\0\0\0".as_slice()),
        ] {
            std::fs::write(directory.join("assets").join(name), bytes).unwrap();
        }
        Self { directory }
    }

    fn router(&self, services: Services) -> axum::Router {
        deployment_router(services, Some(&self.directory)).unwrap()
    }
}

impl Drop for WebFixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.directory).unwrap();
    }
}

async fn response(router: axum::Router, method: Method, path: &str) -> axum::response::Response {
    router
        .oneshot(
            Request::builder()
                .method(method)
                .uri(path)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap()
}

#[tokio::test]
async fn static_html_js_css_and_wasm_have_mime_and_isolation_headers() {
    let fixture = WebFixture::new();
    for (path, mime, expected) in [
        (
            "/",
            "text/html",
            b"<!doctype html><title>Game</title>".as_slice(),
        ),
        (
            "/assets/app.js",
            "text/javascript",
            b"export const game = true;".as_slice(),
        ),
        ("/assets/app.css", "text/css", b"body{color:red}".as_slice()),
        (
            "/assets/engine.wasm",
            "application/wasm",
            b"\0asm\x01\0\0\0".as_slice(),
        ),
    ] {
        let result = response(fixture.router(Services::WebUi), Method::GET, path).await;
        assert_eq!(result.status(), StatusCode::OK, "{path}");
        assert_eq!(
            result.headers()["cross-origin-opener-policy"],
            "same-origin"
        );
        assert_eq!(
            result.headers()["cross-origin-embedder-policy"],
            "require-corp"
        );
        assert_eq!(result.headers()[header::CONTENT_TYPE], mime);
        assert_eq!(
            to_bytes(result.into_body(), 1024).await.unwrap().as_ref(),
            expected
        );
    }
}

#[tokio::test]
async fn head_has_headers_without_body_and_static_post_is_rejected() {
    let fixture = WebFixture::new();
    let result = response(
        fixture.router(Services::WebUi),
        Method::HEAD,
        "/assets/engine.wasm",
    )
    .await;
    assert_eq!(result.status(), StatusCode::OK);
    assert_eq!(result.headers()[header::CONTENT_TYPE], "application/wasm");
    assert!(to_bytes(result.into_body(), 1024).await.unwrap().is_empty());
    assert_eq!(
        response(fixture.router(Services::WebUi), Method::POST, "/")
            .await
            .status(),
        StatusCode::METHOD_NOT_ALLOWED
    );
}

#[tokio::test]
async fn webui_only_exposes_no_game_api_or_ws() {
    let fixture = WebFixture::new();
    for (method, path) in [
        (Method::POST, "/api/new"),
        (Method::GET, "/api/snapshot"),
        (Method::GET, "/ws"),
        (Method::GET, "/healthz"),
    ] {
        assert_eq!(
            response(fixture.router(Services::WebUi), method, path)
                .await
                .status(),
            StatusCode::NOT_FOUND,
            "{path}"
        );
    }
}

#[tokio::test]
async fn combined_keeps_health_auth_and_ws_handlers_on_same_origin() {
    let fixture = WebFixture::new();
    for (path, expected) in [
        ("/", StatusCode::OK),
        ("/healthz", StatusCode::OK),
        ("/api/snapshot?session_id=missing", StatusCode::UNAUTHORIZED),
        ("/ws", StatusCode::BAD_REQUEST),
    ] {
        assert_eq!(
            response(fixture.router(Services::All), Method::GET, path)
                .await
                .status(),
            expected,
            "{path}"
        );
    }
}

#[tokio::test]
async fn combined_exposes_identity_routes_and_protects_identity_lookup() {
    let fixture = WebFixture::new();
    let guest = fixture
        .router(Services::All)
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/auth/guest")
                .header("content-type", "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(guest.status(), StatusCode::CREATED);
    let guest_body: serde_json::Value = serde_json::from_slice(
        &to_bytes(guest.into_body(), 4096).await.unwrap(),
    )
    .unwrap();
    assert!(guest_body["token"].as_str().is_some());
    assert!(guest_body["subject"]["subject_id"].as_str().is_some());

    let me = fixture
        .router(Services::All)
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/api/auth/me")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(me.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn unknown_api_ws_and_missing_assets_never_return_index() {
    let fixture = WebFixture::new();
    std::fs::create_dir(fixture.directory.join("api")).unwrap();
    std::fs::write(fixture.directory.join("api/secret.txt"), "must not serve").unwrap();
    std::fs::write(fixture.directory.join("ws"), "must not serve").unwrap();
    for mode in [Services::WebUi, Services::All] {
        for path in [
            "/api",
            "/api/no-such-route",
            "/api/secret.txt",
            "/ws/no-such-route",
            "/%61pi/secret.txt",
            "/assets/missing.js",
            "/assets/app.js/missing",
            "/missing",
        ] {
            let result = response(fixture.router(mode), Method::GET, path).await;
            assert_eq!(result.status(), StatusCode::NOT_FOUND, "{path}");
            assert!(
                !String::from_utf8(to_bytes(result.into_body(), 1024).await.unwrap().to_vec())
                    .unwrap()
                    .contains("Game")
            );
        }
    }
}

#[cfg(unix)]
#[tokio::test]
async fn unreadable_resource_is_an_explicit_error_not_a_missing_file() {
    use std::os::unix::fs::PermissionsExt;
    let fixture = WebFixture::new();
    let file = fixture.directory.join("assets/app.js");
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o000)).unwrap();
    assert_eq!(
        std::fs::File::open(&file).unwrap_err().kind(),
        std::io::ErrorKind::PermissionDenied
    );
    let result = response(
        fixture.router(Services::WebUi),
        Method::GET,
        "/assets/app.js",
    )
    .await;
    assert_eq!(result.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let body =
        String::from_utf8(to_bytes(result.into_body(), 1024).await.unwrap().to_vec()).unwrap();
    assert!(body.contains("WebUI resource access failed"));
    assert!(body.contains("service operator"));
    assert!(!body.contains(fixture.directory.to_str().unwrap()));
}

#[tokio::test]
async fn traversal_and_symlinks_cannot_expose_host_files() {
    let fixture = WebFixture::new();
    std::fs::write(
        fixture.directory.join(".env"),
        "private deployment settings",
    )
    .unwrap();
    std::fs::create_dir(fixture.directory.join("assets/.private")).unwrap();
    std::fs::write(
        fixture.directory.join("assets/.private/config"),
        "private deployment settings",
    )
    .unwrap();
    for path in [
        "/../Cargo.toml",
        "/%2e%2e/Cargo.toml",
        "/assets/%2e%2e/%2e%2e/Cargo.toml",
        "/assets/%5c..%5cCargo.toml",
        "/assets/%2fetc/passwd",
        "/assets/%00",
        "/assets/C:/secret",
        "/.env",
        "/%2eenv",
        "/assets/.private/config",
    ] {
        let result = response(fixture.router(Services::WebUi), Method::GET, path).await;
        assert_eq!(result.status(), StatusCode::NOT_FOUND, "{path}");
        let body =
            String::from_utf8(to_bytes(result.into_body(), 1024).await.unwrap().to_vec()).unwrap();
        assert!(!body.contains(fixture.directory.to_str().unwrap()));
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(
            std::env::current_dir().unwrap().join("Cargo.toml"),
            fixture.directory.join("assets/outside.txt"),
        )
        .unwrap();
        assert_eq!(
            response(
                fixture.router(Services::WebUi),
                Method::GET,
                "/assets/outside.txt"
            )
            .await
            .status(),
            StatusCode::NOT_FOUND
        );
        std::os::unix::fs::symlink(
            fixture.directory.join(".env"),
            fixture.directory.join("assets/alias.txt"),
        )
        .unwrap();
        assert_eq!(
            response(
                fixture.router(Services::WebUi),
                Method::GET,
                "/assets/alias.txt"
            )
            .await
            .status(),
            StatusCode::NOT_FOUND
        );
    }
}

#[test]
fn startup_requires_valid_root_index_and_nonempty_assets() {
    assert!(deployment_router(Services::WebUi, None).is_err());
    let fixture = WebFixture::new();
    let missing = fixture.directory.join("missing");
    assert!(deployment_router(Services::WebUi, Some(&missing))
        .unwrap_err()
        .to_string()
        .contains("web-root"));
    std::fs::remove_file(fixture.directory.join("index.html")).unwrap();
    assert!(deployment_router(Services::WebUi, Some(&fixture.directory))
        .unwrap_err()
        .to_string()
        .contains("index.html"));
    std::fs::write(fixture.directory.join("index.html"), "").unwrap();
    assert!(deployment_router(Services::WebUi, Some(&fixture.directory)).is_err());
    std::fs::write(fixture.directory.join("index.html"), "valid").unwrap();
    std::fs::remove_dir_all(fixture.directory.join("assets")).unwrap();
    assert!(deployment_router(Services::WebUi, Some(&fixture.directory))
        .unwrap_err()
        .to_string()
        .contains("assets"));
    std::fs::create_dir(fixture.directory.join("assets")).unwrap();
    assert!(deployment_router(Services::WebUi, Some(&fixture.directory)).is_err());
    std::fs::remove_dir(fixture.directory.join("assets")).unwrap();
    std::fs::write(fixture.directory.join("assets"), "not a directory").unwrap();
    assert!(deployment_router(Services::WebUi, Some(&fixture.directory)).is_err());
}
