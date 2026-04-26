// gcs/src/web.rs
use std::sync::Arc;
use axum::{
    Router,
    extract::{State, WebSocketUpgrade, ws::{WebSocket, Message}},
    response::{Html, Response},
    routing::get,
};
use tokio::sync::broadcast;
use bytes::Bytes;
use crate::state::AppState;

#[derive(Clone)]
struct WsState {
    app:   Arc<AppState>,
    telem: broadcast::Sender<String>,
    video: broadcast::Sender<Bytes>,
}

pub async fn run(
    app:      Arc<AppState>,
    telem_tx: broadcast::Sender<String>,
    video_tx: broadcast::Sender<Bytes>,
    port:     u16,
) {
    // Raise file descriptor limit — macOS default is 256 which is too low
    // for Axum + WebSocket connections + SDL + UDP sockets
    #[cfg(unix)]
    {
        let mut rl = libc::rlimit { rlim_cur: 0, rlim_max: 0 };
        unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut rl); }
        if rl.rlim_cur < 8192 {
            rl.rlim_cur = 8192.min(rl.rlim_max);
            unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &rl); }
            log::info!("web: raised RLIMIT_NOFILE to {}", rl.rlim_cur);
        }
    }

    let ws_state = WsState { app, telem: telem_tx, video: video_tx };

    let router = Router::new()
        .route("/",             get(serve_index))
        .route("/ws/telemetry", get(ws_telemetry))
        .route("/ws/video",     get(ws_video))
        .with_state(ws_state);

    let addr = format!("0.0.0.0:{}", port);
    let listener = tokio::net::TcpListener::bind(&addr).await
        .unwrap_or_else(|e| panic!("Cannot bind web server {}: {}", addr, e));

    log::info!("Web UI → http://localhost:{}", port);
    axum::serve(listener, router).await.unwrap();
}

async fn serve_index() -> Html<&'static str> {
    Html(include_str!("../static/index.html"))
}

async fn ws_telemetry(ws: WebSocketUpgrade, State(s): State<WsState>) -> Response {
    ws.on_upgrade(move |socket| handle_telem_ws(socket, s))
}

async fn handle_telem_ws(mut socket: WebSocket, s: WsState) {
    let mut rx = s.telem.subscribe();
    if let Ok(snap) = serde_json::to_string(&s.app.snapshot()) {
        let _ = socket.send(Message::Text(snap.into())).await;
    }
    loop {
        match rx.recv().await {
            Ok(json) => {
                if socket.send(Message::Text(json.into())).await.is_err() { break; }
            }
            Err(broadcast::error::RecvError::Lagged(n)) => {
                log::warn!("telem WS lagged {}", n);
            }
            Err(_) => break,
        }
    }
    let _ = socket.close().await;
}

async fn ws_video(ws: WebSocketUpgrade, State(s): State<WsState>) -> Response {
    ws.on_upgrade(move |socket| handle_video_ws(socket, s))
}

async fn handle_video_ws(mut socket: WebSocket, s: WsState) {
    let mut rx = s.video.subscribe();
    loop {
        match rx.recv().await {
            Ok(nal) => {
                if socket.send(Message::Binary(nal.to_vec().into())).await.is_err() {
                    break;
                }
            }
            Err(broadcast::error::RecvError::Lagged(n)) => {
                log::warn!("video WS lagged {}", n);
            }
            Err(_) => break,
        }
    }
    let _ = socket.close().await;
}
