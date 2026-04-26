// gcs/src/main.rs
mod state;
mod telemetry;
mod gamepad;
mod web;
mod traccar;

use std::sync::Arc;
use tokio::sync::broadcast;
use state::AppState;

#[tokio::main]
async fn main() {
    env_logger::builder()
        .filter_level(log::LevelFilter::Info)
        .init();

    let config = Config::from_env();
    log::info!("skypulse-gcs starting — drone={} web=localhost:{}",
        config.drone_ip, config.web_port);
    if config.cell_count > 0 {
        log::info!("Battery: {}S configured", config.cell_count);
    } else {
        log::info!("Battery: cell count will be auto-detected from voltage");
    }

    let state = Arc::new(AppState::new(config.cell_count));

    let (telem_tx, _) = broadcast::channel::<String>(64);

    // Telemetry UDP receiver
    {
        let state = state.clone();
        let tx    = telem_tx.clone();
        tokio::spawn(async move {
            telemetry::run(state, tx, config.telem_port).await;
        });
    }

    // Traccar GPS tracking (optional — only if TRACCAR_URL is set)
    if let Some(traccar_url) = config.traccar_url.clone() {
        let state     = state.clone();
        let device_id = config.traccar_id.clone();
        tokio::spawn(async move {
            traccar::run(state, traccar_url, device_id).await;
        });
    }

    // SDL2 gamepad loop (blocking thread)
    {
        let state    = state.clone();
        let drone_ip = config.drone_ip.clone();
        std::thread::spawn(move || {
            gamepad::run(state, &drone_ip, config.rc_port);
        });
    }

    log::info!("Press Ctrl+C to stop");

    // Run web server — exit cleanly on Ctrl+C or SIGTERM
    tokio::select! {
        _ = web::run(state, telem_tx, config.web_port) => {
            log::warn!("Web server exited unexpectedly");
        },
        _ = tokio::signal::ctrl_c() => {
            log::info!("Ctrl+C received — shutting down");
        },
    }

    log::info!("Goodbye.");
    std::process::exit(0);
}

#[derive(Clone)]
pub struct Config {
    pub drone_ip:    String,
    pub rc_port:     u16,
    pub telem_port:  u16,
    pub web_port:    u16,
    pub cell_count:  u8,    // 0 = auto-detect from voltage
    pub traccar_url: Option<String>,
    pub traccar_id:  String,
}

impl Config {
    fn from_env() -> Self {
        Self {
            drone_ip:    std::env::var("DRONE_IP").unwrap_or("10.8.0.2".into()),
            rc_port:     std::env::var("RC_PORT").ok()
                             .and_then(|s| s.parse().ok()).unwrap_or(2223),
            telem_port:  std::env::var("TELEM_PORT").ok()
                             .and_then(|s| s.parse().ok()).unwrap_or(2224),
            web_port:    std::env::var("WEB_PORT").ok()
                             .and_then(|s| s.parse().ok()).unwrap_or(8080),
            cell_count:  std::env::var("CELL_COUNT").ok()
                             .and_then(|s| s.parse().ok()).unwrap_or(0),
            traccar_url: std::env::var("TRACCAR_URL").ok(),
            traccar_id:  std::env::var("TRACCAR_ID").unwrap_or("skypulse".into()),
        }
    }
}
