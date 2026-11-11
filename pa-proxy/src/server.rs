use std::net::SocketAddr;

use axum::{
    body::Body,
    extract::State,
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde_json::{json, Value};

use crate::proxy::{
    error::ProxyError,
    providers::{
        streaming_codex_chat::create_responses_sse_stream_from_chat_with_context,
        transform_codex_chat::{
            build_codex_tool_context_from_request, chat_completion_to_response_with_context,
            responses_to_chat_completions_with_reasoning,
        },
    },
};

#[derive(Clone)]
struct AppState {
    http: reqwest::Client,
    api_key: String,
    base_url: String,
    model: String,
}

pub async fn serve() {
    env_logger::init();

    let port: u16 = load_env_var("PA_PROXY_PORT")
        .and_then(|value| value.parse().ok())
        .unwrap_or(15722);

    let state = AppState {
        http: reqwest::Client::new(),
        api_key: load_env_var("DEEPSEEK_API_KEY").unwrap_or_default(),
        base_url: load_env_var("DEEPSEEK_BASE_URL")
            .unwrap_or_else(|| "https://api.deepseek.com".to_string()),
        model: load_env_var("PA_MODEL").unwrap_or_else(|| "deepseek-flash".to_string()),
    };

    let app = Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/v1/responses", post(responses))
        .route("/responses", post(responses))
        .route("/v1/responses/compact", post(responses))
        .route("/responses/compact", post(responses))
        .with_state(state);

    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .unwrap_or_else(|error| panic!("pa-proxy: failed to bind {addr}: {error}"));

    log::info!("pa-proxy listening on http://{addr}");
    axum::serve(listener, app)
        .await
        .unwrap_or_else(|error| panic!("pa-proxy server error: {error}"));
}

/// Read a setting from the process environment first, then from local `.env`
/// files. Values are never logged. `.env` is gitignored.
fn load_env_var(key: &str) -> Option<String> {
    if let Ok(value) = std::env::var(key) {
        let value = value.trim().to_string();
        if !value.is_empty() {
            return Some(value);
        }
    }

    let candidates = [
        std::env::var("PA_ENV_FILE").ok().map(std::path::PathBuf::from),
        Some(std::path::PathBuf::from(".env")),
        Some(std::path::PathBuf::from("../.env")),
        Some(std::path::PathBuf::from(".pa/.env")),
        Some(std::path::PathBuf::from("../.pa/.env")),
    ];

    for path in candidates.into_iter().flatten() {
        if let Some(value) = read_env_key(&path, key) {
            return Some(value);
        }
    }
    None
}

fn read_env_key(path: &std::path::Path, key: &str) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((name, value)) = line.split_once('=') else {
            continue;
        };
        if name.trim() != key {
            continue;
        }
        let value = value.trim().trim_matches('"').trim_matches('\'').to_string();
        if !value.is_empty() {
            return Some(value);
        }
    }
    None
}

async fn responses(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> Result<Response, ProxyError> {
    if state.api_key.trim().is_empty() {
        return Err(ProxyError::ConfigError(
            "DEEPSEEK_API_KEY is not set; set it before starting pa-proxy".to_string(),
        ));
    }

    let wants_stream = body
        .get("stream")
        .and_then(Value::as_bool)
        .unwrap_or(true);

    let tool_context = build_codex_tool_context_from_request(&body);
    let mut chat = responses_to_chat_completions_with_reasoning(body, None)?;
    chat["model"] = json!(state.model.clone());
    if wants_stream {
        chat["stream"] = json!(true);
        chat["stream_options"] = json!({ "include_usage": true });
    }

    let url = format!("{}/v1/chat/completions", state.base_url.trim_end_matches('/'));
    let upstream = state
        .http
        .post(&url)
        .bearer_auth(&state.api_key)
        .json(&chat)
        .send()
        .await
        .map_err(|error| ProxyError::ForwardFailed(format!("{url}: {error}")))?;

    if !upstream.status().is_success() {
        let status = upstream.status().as_u16();
        let body = upstream.text().await.ok();
        return Err(ProxyError::UpstreamError { status, body });
    }

    if wants_stream {
        let sse = create_responses_sse_stream_from_chat_with_context(
            upstream.bytes_stream(),
            tool_context,
        );
        let body = Body::from_stream(sse);
        Ok(Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "text/event-stream")
            .header(header::CACHE_CONTROL, "no-cache")
            .header(header::CONNECTION, "keep-alive")
            .body(body)
            .map_err(|error| ProxyError::Internal(error.to_string()))?)
    } else {
        let json = upstream
            .json::<Value>()
            .await
            .map_err(|error| ProxyError::ForwardFailed(format!("decode upstream: {error}")))?;
        let response = chat_completion_to_response_with_context(json, &tool_context)?;
        Ok(Json(response).into_response())
    }
}
