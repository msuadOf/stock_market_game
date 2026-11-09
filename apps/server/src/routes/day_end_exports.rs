//! 文件导出的 HTTP 读/确认与 WS 连接级资源回收。

use super::*;
use crate::actor::{DayEndExportRequest, DayEndExportResponse};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DayEndExportBody {
    pub session_id: String,
    pub generation: String,
    pub subscription: String,
    pub candidate: engine::session::protocol::SaveCandidateKey,
}

pub(super) struct DayEndExportConnection {
    pub(super) id: String,
    handles: Arc<crate::actor::SessionHandles>,
}

impl DayEndExportConnection {
    pub(super) fn new(handles: Arc<crate::actor::SessionHandles>) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            handles,
        }
    }
}

impl Drop for DayEndExportConnection {
    fn drop(&mut self) {
        self.handles.drop_day_end_export_connection(self.id.clone());
    }
}

pub async fn api_day_end_export(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Result<Json<DayEndExportBody>, JsonRejection>,
) -> Response {
    export_request(state, headers, body, false).await
}

pub async fn api_day_end_export_ack(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Result<Json<DayEndExportBody>, JsonRejection>,
) -> Response {
    export_request(state, headers, body, true).await
}

async fn export_request(
    state: AppState,
    headers: HeaderMap,
    body: Result<Json<DayEndExportBody>, JsonRejection>,
    acknowledge: bool,
) -> Response {
    let Json(body) = match body {
        Ok(body) => body,
        Err(error) => return invalid_json_response(error),
    };
    let (handles, subject) =
        match authorized_market_controller(&state, &body.session_id, authorization_token(&headers))
            .await
        {
            Ok(value) => value,
            Err(response) => return *response,
        };
    let generation = match parse_host_parity_generation(&body.generation) {
        Ok(value) => value,
        Err(response) => return *response,
    };
    let request = if acknowledge {
        DayEndExportRequest::Acknowledge {
            subscription: body.subscription,
            candidate: body.candidate,
        }
    } else {
        DayEndExportRequest::Read {
            subscription: body.subscription,
            candidate: body.candidate,
        }
    };
    match handles.day_end_export(generation, subject, request).await {
        Ok(DayEndExportResponse::Saved(slot)) if !acknowledge => {
            (StatusCode::OK, Json(*slot)).into_response()
        }
        Ok(DayEndExportResponse::Completed) if acknowledge => {
            StatusCode::NO_CONTENT.into_response()
        }
        Ok(_) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "DAY_END_EXPORT_PROTOCOL",
            "文件导出响应类型错误",
        ),
        Err(crate::actor::SendCommandError::ActorGone) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "ACTOR_GONE",
            "session actor gone",
        ),
        Err(crate::actor::SendCommandError::Rejected(reason))
            if reason.starts_with("STALE_SESSION_GENERATION") =>
        {
            api_error(StatusCode::CONFLICT, "STALE_SESSION_GENERATION", reason)
        }
        Err(error) => api_error(
            StatusCode::BAD_REQUEST,
            "DAY_END_EXPORT_REJECTED",
            error.to_string(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_body_uses_the_existing_exact_candidate_wire_shape() {
        let body = serde_json::json!({ "session_id": "market", "generation": "1", "subscription": "connection-token", "candidate": { "seq": 42, "settledDate": "2030-06-08" } });
        let parsed: DayEndExportBody = serde_json::from_value(body.clone()).unwrap();
        assert_eq!(parsed.candidate.seq, 42);
        let mut invalid = body.clone();
        invalid["candidate"]["settled_date"] = invalid["candidate"]["settledDate"].clone();
        invalid["candidate"]
            .as_object_mut()
            .unwrap()
            .remove("settledDate");
        assert!(serde_json::from_value::<DayEndExportBody>(invalid).is_err());
        let mut invalid = body;
        invalid["owner"] = serde_json::json!("forged-subject");
        assert!(serde_json::from_value::<DayEndExportBody>(invalid).is_err());
    }

    #[tokio::test]
    async fn export_http_requires_controller_and_current_generation() {
        use super::super::auth_tests::{request, setup};
        let app = crate::app_router();
        let (status, created) = request(
            app.clone(),
            "POST",
            "/api/new",
            Some(serde_json::json!({ "setup": setup(), "seed": "91" })),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{created}");
        let body = serde_json::json!({ "session_id": created["session_id"], "generation": created["generation"], "subscription": "forged-token", "candidate": { "seq": 42, "settledDate": "2030-06-08" } });
        for route in ["/api/day-end-export", "/api/day-end-export/ack"] {
            assert_eq!(
                request(app.clone(), "POST", route, Some(body.clone()), None)
                    .await
                    .0,
                StatusCode::UNAUTHORIZED
            );
            let (status, stranger) = request(
                app.clone(),
                "POST",
                "/api/auth/guest",
                Some(serde_json::json!({})),
                None,
            )
            .await;
            assert_eq!(status, StatusCode::CREATED);
            assert_eq!(
                request(
                    app.clone(),
                    "POST",
                    route,
                    Some(body.clone()),
                    stranger["token"].as_str()
                )
                .await
                .0,
                StatusCode::FORBIDDEN
            );
            let credential = created["test_credential"].as_str();
            assert_eq!(
                request(app.clone(), "POST", route, Some(body.clone()), credential)
                    .await
                    .0,
                StatusCode::BAD_REQUEST
            );
            let mut stale = body.clone();
            stale["generation"] = serde_json::json!("999");
            assert_eq!(
                request(app.clone(), "POST", route, Some(stale), credential)
                    .await
                    .0,
                StatusCode::CONFLICT
            );
        }
    }
}
