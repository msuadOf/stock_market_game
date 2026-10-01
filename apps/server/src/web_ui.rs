use crate::deployment::DeploymentError;
use axum::extract::{Path, Request, State};
use axum::http::{header, HeaderValue, Method, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::any;
use axum::Router;
use std::path::{Component, Path as FilePath, PathBuf};
use tower_http::services::ServeFile;

#[derive(Clone)]
struct WebState {
    root: PathBuf,
}

fn startup_error(path: &FilePath, reason: impl std::fmt::Display) -> DeploymentError {
    DeploymentError(format!("web-root validation failed at {}: {reason}; provide a complete WebUI package or set --web-root PATH", path.display()))
}

pub(crate) fn static_router(root: &FilePath) -> Result<Router, DeploymentError> {
    let root = std::fs::canonicalize(root).map_err(|error| startup_error(root, error))?;
    if !root.is_dir() {
        return Err(startup_error(&root, "expected a directory"));
    }
    let index = root.join("index.html");
    let index = std::fs::canonicalize(&index).map_err(|error| startup_error(&index, error))?;
    if !index.starts_with(&root) || !index.is_file() {
        return Err(startup_error(
            &index,
            "index.html must be a file within web-root",
        ));
    }
    if std::fs::read(&index)
        .map_err(|error| startup_error(&index, error))?
        .is_empty()
    {
        return Err(startup_error(&index, "index.html must not be empty"));
    }
    let assets = root.join("assets");
    let assets = std::fs::canonicalize(&assets).map_err(|error| startup_error(&assets, error))?;
    if !assets.starts_with(&root) || !assets.is_dir() {
        return Err(startup_error(
            &assets,
            "assets must be a directory within web-root",
        ));
    }
    let mut entries = std::fs::read_dir(&assets).map_err(|error| startup_error(&assets, error))?;
    match entries.next() {
        Some(Ok(_)) => {}
        Some(Err(error)) => return Err(startup_error(&assets, error)),
        None => return Err(startup_error(&assets, "assets must not be empty")),
    }
    Ok(Router::new()
        .route("/", any(index_file))
        .route("/*path", any(static_file))
        .with_state(WebState { root })
        .layer(middleware::from_fn(isolation_headers)))
}

async fn isolation_headers(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    response.headers_mut().insert(
        "cross-origin-opener-policy",
        HeaderValue::from_static("same-origin"),
    );
    response.headers_mut().insert(
        "cross-origin-embedder-policy",
        HeaderValue::from_static("require-corp"),
    );
    response.headers_mut().insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    response
}

async fn index_file(State(state): State<WebState>, request: Request) -> Response {
    serve_file(state, "index.html", request).await
}

async fn static_file(
    State(state): State<WebState>,
    Path(path): Path<String>,
    request: Request,
) -> Response {
    if matches!(path.split('/').next(), Some("api" | "ws" | "healthz")) {
        return not_found();
    }
    serve_file(state, &path, request).await
}

fn not_found() -> Response {
    (StatusCode::NOT_FOUND, "WebUI resource not found").into_response()
}

fn file_error(error: std::io::Error, path: &str) -> Response {
    if matches!(
        error.kind(),
        std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
    ) {
        return not_found();
    }
    tracing::error!(request_path = path, %error, "WebUI resource access failed");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        format!("WebUI resource access failed ({:?}); contact the service operator with this request path", error.kind()),
    )
        .into_response()
}

async fn serve_file(state: WebState, path: &str, request: Request) -> Response {
    if path.split('/').any(|segment| {
        segment.is_empty() || segment.starts_with('.') || segment.contains(['\\', ':', '\0'])
    }) {
        return not_found();
    }
    if request.method() != Method::GET && request.method() != Method::HEAD {
        let mut response = (
            StatusCode::METHOD_NOT_ALLOWED,
            "WebUI resources support GET and HEAD only",
        )
            .into_response();
        response
            .headers_mut()
            .insert(header::ALLOW, HeaderValue::from_static("GET, HEAD"));
        return response;
    }
    let file = match tokio::fs::canonicalize(state.root.join(path)).await {
        Ok(file) => file,
        Err(error) => return file_error(error, path),
    };
    let relative = match file.strip_prefix(&state.root) {
        Ok(relative) => relative,
        Err(_) => return not_found(),
    };
    if relative.components().any(|component| match component {
        Component::Normal(name) => name.to_string_lossy().starts_with('.'),
        _ => true,
    }) {
        return not_found();
    }
    match tokio::fs::metadata(&file).await {
        Ok(metadata) if metadata.is_file() => {}
        Ok(_) => return not_found(),
        Err(error) => return file_error(error, path),
    }
    match ServeFile::new(&file).try_call(request).await {
        Ok(response) if response.status() == StatusCode::NOT_FOUND => {
            match tokio::fs::File::open(file).await {
                Ok(_) => file_error(
                    std::io::Error::other(
                        "static file service returned 404 for an existing readable file",
                    ),
                    path,
                ),
                Err(error) => file_error(error, path),
            }
        }
        Ok(response) => response.map(axum::body::Body::new),
        Err(error) => file_error(error, path),
    }
}
