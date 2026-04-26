// gcs/src/state.rs
use std::sync::Mutex;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct TelemetrySnapshot {
    // GPS
    pub lat:        f64,
    pub lon:        f64,
    pub altitude_m: f32,
    pub speed_kmh:  f32,
    pub heading:    f32,
    pub satellites: u8,
    pub gps_fix:    bool,
    // Battery
    pub voltage_v:   f32,
    pub current_a:   f32,
    pub used_mah:    u32,
    pub battery_pct: u8,
    pub cell_count:  u8,   // detected or configured S count
    pub cell_v:      f32,  // per-cell voltage
    pub batt_warn:   u8,   // 0=ok 1=low 2=critical (land now)
    // Attitude
    pub pitch_deg:  f32,
    pub roll_deg:   f32,
    pub yaw_deg:    f32,
    // Flight
    pub armed:       bool,
    pub flight_mode: String,
    pub vspeed_ms:   f32,
    // Link
    pub link_quality: u8,
    pub rssi_dbm:     i16,
    // Home
    pub home_lat:      f64,
    pub home_lon:      f64,
    pub home_set:      bool,
    pub home_dist_m:   f32,  // distance to home in metres
    pub home_bearing:  f32,  // bearing to home in degrees
    // Failsafe
    pub failsafe:   bool,
    pub last_rc_ms: u64,
}

impl Default for TelemetrySnapshot {
    fn default() -> Self {
        Self {
            lat: 0.0, lon: 0.0, altitude_m: 0.0, speed_kmh: 0.0,
            heading: 0.0, satellites: 0, gps_fix: false,
            voltage_v: 0.0, current_a: 0.0, used_mah: 0, battery_pct: 0,
            cell_count: 0, cell_v: 0.0, batt_warn: 0,
            pitch_deg: 0.0, roll_deg: 0.0, yaw_deg: 0.0,
            armed: false, flight_mode: "UNKNOWN".into(), vspeed_ms: 0.0,
            link_quality: 0, rssi_dbm: 0,
            home_lat: 0.0, home_lon: 0.0, home_set: false,
            home_dist_m: 0.0, home_bearing: 0.0,
            failsafe: false, last_rc_ms: 0,
        }
    }
}

// ── Haversine helpers ─────────────────────────────────────────────────────────

pub fn haversine_m(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f32 {
    let r = 6_371_000.0_f64;
    let d_lat = (lat2 - lat1).to_radians();
    let d_lon = (lon2 - lon1).to_radians();
    let a = (d_lat / 2.0).sin().powi(2)
        + lat1.to_radians().cos() * lat2.to_radians().cos()
        * (d_lon / 2.0).sin().powi(2);
    (r * 2.0 * a.sqrt().atan2((1.0 - a).sqrt())) as f32
}

pub fn bearing_deg(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f32 {
    let d_lon = (lon2 - lon1).to_radians();
    let y = d_lon.sin() * lat2.to_radians().cos();
    let x = lat1.to_radians().cos() * lat2.to_radians().sin()
        - lat1.to_radians().sin() * lat2.to_radians().cos() * d_lon.cos();
    let b = y.atan2(x).to_degrees();
    ((b + 360.0) % 360.0) as f32
}

// ── Battery helpers ───────────────────────────────────────────────────────────

/// Detect cell count from resting voltage (crude but works at startup).
/// Returns 0 if voltage is too low to determine.
pub fn detect_cells(voltage_v: f32) -> u8 {
    // Fully charged cell ~4.2V, resting ~3.7V, minimum ~3.3V
    // Use 3.7V/cell as detection threshold
    if      voltage_v > 4.2 * 6.0 { 0 } // can't determine > 6S
    else if voltage_v > 4.2 * 5.0 { 6 }
    else if voltage_v > 4.2 * 4.0 { 5 }
    else if voltage_v > 4.2 * 3.0 { 4 }
    else if voltage_v > 4.2 * 2.0 { 3 }
    else if voltage_v > 4.2 * 1.0 { 2 }
    else if voltage_v > 0.5        { 1 }
    else                           { 0 }
}

pub struct AppState {
    pub telem: Mutex<TelemetrySnapshot>,
}

impl AppState {
    pub fn new(cell_count: u8) -> Self {
        let mut t = TelemetrySnapshot::default();
        t.cell_count = cell_count;
        Self { telem: Mutex::new(t) }
    }

    pub fn snapshot(&self) -> TelemetrySnapshot {
        self.telem.lock().unwrap().clone()
    }
}
