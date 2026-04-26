// gcs/src/telemetry.rs
use std::sync::Arc;
use tokio::{net::UdpSocket, sync::broadcast};
use crsf_proto::{CrsfParser, CrsfFrame};
use crate::state::{AppState, haversine_m, bearing_deg, detect_cells};

pub async fn run(
    state:     Arc<AppState>,
    tx:        broadcast::Sender<String>,
    bind_port: u16,
) {
    let addr = format!("0.0.0.0:{}", bind_port);
    let sock = UdpSocket::bind(&addr).await
        .unwrap_or_else(|e| panic!("Cannot bind telemetry UDP {}: {}", addr, e));
    log::info!("Telemetry listening on {}", addr);

    let mut buf        = [0u8; 512];
    let mut parser     = CrsfParser::new();
    let mut frames     = Vec::with_capacity(4);
    let mut pkt_count  = 0u64;
    let mut frame_count = 0u64;

    loop {
        let n = match sock.recv(&mut buf).await {
            Ok(n)  => n,
            Err(e) => { log::warn!("telem recv: {}", e); continue; }
        };

        pkt_count += 1;

        // Log first few packets and every 100th to verify data arriving
        if pkt_count <= 5 || pkt_count % 100 == 0 {
            let hex: String = buf[..n.min(8)].iter()
                .map(|b| format!("{:02X}", b)).collect::<Vec<_>>().join(" ");
            log::info!("telem: pkt #{} {} bytes hex: {}", pkt_count, n, hex);
        }

        frames.clear();
        parser.push(&buf[..n], &mut frames);

        for frame in &frames {
            frame_count += 1;
            if frame_count <= 10 || frame_count % 50 == 0 {
                log::info!("telem: parsed frame #{}: {:?}", frame_count,
                    std::mem::discriminant(frame));
            }
            apply_frame(&state, frame);
        }

        if !frames.is_empty() {
            let snap = state.snapshot();
            if let Ok(json) = serde_json::to_string(&snap) {
                let _ = tx.send(json);
            }
        }
    }
}

fn apply_frame(state: &AppState, frame: &CrsfFrame) {
    let mut t = state.telem.lock().unwrap();
    match frame {
        CrsfFrame::Gps(g) => {
            let _prev_fix = t.gps_fix;
            t.lat        = g.lat;
            t.lon        = g.lon;
            t.altitude_m = g.altitude_m;
            t.speed_kmh  = g.ground_speed_kmh;
            t.heading    = g.heading_deg;
            t.satellites = g.satellites;
            t.gps_fix    = g.satellites >= 6;

            // Set home on first good fix after arm
            if t.gps_fix && t.armed && !t.home_set {
                t.home_lat = g.lat;
                t.home_lon = g.lon;
                t.home_set = true;
                log::info!("Home set: {:.6}, {:.6}", g.lat, g.lon);
            }

            // Update home distance + bearing every GPS update
            if t.home_set && t.gps_fix {
                t.home_dist_m  = haversine_m(t.lat, t.lon, t.home_lat, t.home_lon);
                t.home_bearing = bearing_deg(t.lat, t.lon, t.home_lat, t.home_lon);
            }
        }
        CrsfFrame::Battery(b) => {
            t.voltage_v   = b.voltage_v;
            // Cap current/mAh — FC sends 0xFFFFFF when no current sensor
            t.current_a   = if b.current_a > 500.0 { 0.0 } else { b.current_a };
            t.used_mah    = if b.used_mah > 100000  { 0   } else { b.used_mah };
            t.battery_pct = b.remaining_pct;

            // Auto-detect cell count on first valid reading
            if t.cell_count == 0 && b.voltage_v > 5.0 {
                t.cell_count = detect_cells(b.voltage_v);
                log::info!("Battery: {:.1}V → {}S detected", b.voltage_v, t.cell_count);
            }

            // Per-cell voltage
            if t.cell_count > 0 {
                t.cell_v = b.voltage_v / t.cell_count as f32;
                t.batt_warn = if t.cell_v < 3.3 { 2 }  // LAND NOW
                              else if t.cell_v < 3.5 { 1 }  // LOW
                              else { 0 };
            }
        }
        CrsfFrame::Attitude(a) => {
            t.pitch_deg = a.pitch_deg;
            t.roll_deg  = a.roll_deg;
            t.yaw_deg   = a.yaw_deg;
            if !t.gps_fix { t.heading = a.yaw_deg.rem_euclid(360.0); }
        }
        CrsfFrame::FlightMode(m) => {
            let was_armed = t.armed;
            // Betaflight appends '*' when disarmed, nothing when armed
            // '!' suffix means Airmode active — keep it for display but don't affect arm detection
            t.armed       = !m.ends_with('*');
            t.flight_mode = m.trim_end_matches('*').trim().to_string();

            if t.armed && !was_armed {
                t.home_set = false;
                log::info!("FC ARMED — flight mode: {}", t.flight_mode);
            }
            if !t.armed && was_armed {
                log::info!("FC DISARMED");
            }
        }
        CrsfFrame::LinkStats(ls) => {
            t.link_quality = ls.uplink_link_quality;
            t.rssi_dbm     = -(ls.uplink_rssi_1 as i16);
        }
        CrsfFrame::Vario(v)   => { t.vspeed_ms = v.vertical_speed_ms; }
        CrsfFrame::BaroAlt(b) => {
            if !t.gps_fix { t.altitude_m = b.altitude_m; }
            if t.vspeed_ms == 0.0 { t.vspeed_ms = b.vspeed_ms; }
        }
        _ => {}
    }
}
