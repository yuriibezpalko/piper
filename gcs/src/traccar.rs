// gcs/src/traccar.rs
// Posts GPS position to a Traccar server every 500ms.
// Traccar is open-source GPS tracking: https://www.traccar.org
//
// Protocol: OsmAnd HTTP (same as masina):
//   GET /?id=<device_id>&timestamp=<unix>&lat=<lat>&lon=<lon>&speed=<kmh>&altitude=<m>
//
// Enable by setting TRACCAR_URL env var:
//   TRACCAR_URL=http://your-traccar-server.com:8082
//
// Optional device ID:
//   TRACCAR_ID=skypulse-1   (default: "skypulse")

use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::time::sleep;
use crate::state::AppState;

pub async fn run(state: Arc<AppState>, url: String, device_id: String) {
    log::info!("Traccar: posting to {} as '{}'", url, device_id);

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .expect("Failed to build HTTP client");

    loop {
        sleep(Duration::from_millis(500)).await;

        let snap = state.snapshot();

        // Only post when we have a GPS fix
        if !snap.gps_fix { continue; }

        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let request_url = format!(
            "{}/?id={}&timestamp={}&lat={:.7}&lon={:.7}&speed={:.1}&altitude={:.1}",
            url, device_id, ts,
            snap.lat, snap.lon,
            snap.speed_kmh, snap.altitude_m,
        );

        match client.get(&request_url).send().await {
            Ok(_)  => log::debug!("Traccar: posted {:.6},{:.6}", snap.lat, snap.lon),
            Err(e) => log::warn!("Traccar: post failed: {}", e),
        }
    }
}
