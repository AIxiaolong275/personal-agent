use std::{convert::Infallible, path::PathBuf, process::Stdio};

use axum::{
    extract::State,
    http::StatusCode,
    response::{
        sse::{Event, KeepAlive, Sse},
        Html, IntoResponse, Response,
    },
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::json;
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Command,
    sync::mpsc,
};
use tokio_stream::wrappers::ReceiverStream;

const INDEX_HTML: &str = include_str!("../ui/index.html");

#[derive(Clone)]
struct AppState {
    pa_bin: PathBuf,
    codex_home: PathBuf,
    default_cwd: PathBuf,
}

#[derive(Deserialize)]
struct ChatRequest {
    prompt: String,
    thread_id: Option<String>,
    cwd: Option<String>,
}

#[tokio::main]
async fn main() {
    let state = AppState {
        pa_bin: std::env::var("PA_BIN")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(r"C:\personal agent\bin\pa.exe")),
        codex_home: std::env::var("PA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(r"C:\personal agent\.pa")),
        default_cwd: std::env::var("PA_DEFAULT_CWD")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(r"C:\personal agent")),
    };

    let port: u16 = std::env::var("PA_WEB_PORT")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(15723);

    let app = Router::new()
        .route("/", get(|| async { Html(INDEX_HTML) }))
        .route("/health", get(|| async { "ok" }))
        .route("/api/chat", post(chat))
        .with_state(state);

    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .unwrap_or_else(|error| panic!("pa-web: failed to bind {addr}: {error}"));
    println!("pa-web listening on http://{addr}");
    axum::serve(listener, app)
        .await
        .unwrap_or_else(|error| panic!("pa-web server error: {error}"));
}

async fn chat(State(state): State<AppState>, Json(request): Json<ChatRequest>) -> Response {
    if request.prompt.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "prompt is empty" })),
        )
            .into_response();
    }

    let cwd = request
        .cwd
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| state.default_cwd.clone());

    let mut args: Vec<String> = vec![
        "exec".into(),
        "--json".into(),
        "--skip-git-repo-check".into(),
        "--approve-for-me".into(),
        "--cd".into(),
        cwd.display().to_string(),
    ];

    if let Some(thread_id) = request
        .thread_id
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        args.push("resume".into());
        args.push(thread_id.to_string());
    }
    args.push(request.prompt);

    let mut command = Command::new(&state.pa_bin);
    command
        .args(&args)
        .env("CODEX_HOME", &state.codex_home)
        .current_dir(&cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": format!("failed to start {}: {error}", state.pa_bin.display())
                })),
            )
                .into_response();
        }
    };

    let stdout = child.stdout.take().expect("piped stdout");
    let stderr = child.stderr.take().expect("piped stderr");
    let (tx, rx) = mpsc::channel::<Result<Event, Infallible>>(128);

    let stdout_tx = tx.clone();
    tokio::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let line = line.trim().to_string();
            if line.is_empty() {
                continue;
            }
            if stdout_tx.send(Ok(Event::default().data(line))).await.is_err() {
                break;
            }
        }
    });

    let stderr_tx = tx.clone();
    tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let line = line.trim().to_string();
            if line.is_empty() {
                continue;
            }
            let payload = json!({ "type": "stderr", "message": line }).to_string();
            if stderr_tx
                .send(Ok(Event::default().data(payload)))
                .await
                .is_err()
            {
                break;
            }
        }
    });

    tokio::spawn(async move {
        let code = child
            .wait()
            .await
            .ok()
            .and_then(|status| status.code())
            .unwrap_or(-1);
        let payload = json!({ "type": "process.exit", "code": code }).to_string();
        let _ = tx.send(Ok(Event::default().data(payload))).await;
    });

    Sse::new(ReceiverStream::new(rx))
        .keep_alive(KeepAlive::default())
        .into_response()
}
