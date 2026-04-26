// crsf-proto/src/lib.rs
// Pure Rust CRSF protocol library.
// Used by GCS (encode RC frames, decode telemetry).

use std::string::String;

// ── Constants ────────────────────────────────────────────────────────────────

pub const CRSF_SYNC           : u8 = 0xC8;
pub const TYPE_RC_CHANNELS    : u8 = 0x16;
pub const TYPE_GPS            : u8 = 0x02;
pub const TYPE_BATTERY        : u8 = 0x08;
pub const TYPE_ATTITUDE       : u8 = 0x1E;
pub const TYPE_FLIGHT_MODE    : u8 = 0x21;
pub const TYPE_LINK_STATS     : u8 = 0x14;
pub const TYPE_VARIO          : u8 = 0x07;
pub const TYPE_BARO_ALT       : u8 = 0x09;
pub const TYPE_HEARTBEAT      : u8 = 0x0B;

pub const RC_MIN_US  : u16 = 988;
pub const RC_MID_US  : u16 = 1500;
pub const RC_MAX_US  : u16 = 2012;

// ── CRC ─────────────────────────────────────────────────────────────────────

pub fn crc8_dvb_s2(data: &[u8]) -> u8 {
    let mut crc: u8 = 0;
    for &b in data {
        crc ^= b;
        for _ in 0..8 {
            if crc & 0x80 != 0 { crc = (crc << 1) ^ 0xD5; }
            else                { crc <<= 1; }
        }
    }
    crc
}

// ── RC frame builder ─────────────────────────────────────────────────────────

/// Convert microseconds [988..2012] to 11-bit CRSF value [172..1811]
#[inline]
pub fn us_to_crsf(us: u16) -> u16 {
    let us = us.clamp(RC_MIN_US, RC_MAX_US) as u32;
    (((us - 988) * 1639) / 1024 + 172) as u16
}

/// Build a 26-byte CRSF RC_CHANNELS_PACKED frame.
/// `channels`: 16 values in microseconds [988..2012], mid=1500.
pub fn build_rc_frame(channels: &[u16; 16]) -> [u8; 26] {
    // Pack 16 × 11-bit values into 22 bytes
    let mut out = [0u8; 22];
    let mut bit_pos = 0usize;

    for &ch in channels.iter() {
        let v = us_to_crsf(ch) as u64;
        // spread across bytes
        let byte_idx = bit_pos / 8;
        let bit_off  = bit_pos % 8;
        // up to 3 bytes affected by an 11-bit field
        let chunk = v << bit_off;
        out[byte_idx]     |= (chunk & 0xFF) as u8;
        if byte_idx + 1 < 22 { out[byte_idx + 1] |= ((chunk >> 8) & 0xFF) as u8; }
        if byte_idx + 2 < 22 { out[byte_idx + 2] |= ((chunk >> 16) & 0xFF) as u8; }
        bit_pos += 11;
    }

    // Frame: [SYNC][LEN=24][TYPE][payload 22 bytes][CRC]
    let mut frame = [0u8; 26];
    frame[0] = CRSF_SYNC;
    frame[1] = 24; // len = type(1) + payload(22) + crc(1)
    frame[2] = TYPE_RC_CHANNELS;
    frame[3..25].copy_from_slice(&out);
    frame[25] = crc8_dvb_s2(&frame[2..25]);
    frame
}

/// Build neutral RC frame — all sticks center, throttle min.
pub fn build_neutral_frame() -> [u8; 26] {
    let mut ch = [RC_MID_US; 16];
    ch[2] = RC_MIN_US; // throttle channel 3 (0-indexed: 2)
    build_rc_frame(&ch)
}

/// Build failsafe frame — neutral sticks + CH5 high (ArduPilot FS trigger).
pub fn build_failsafe_frame() -> [u8; 26] {
    let mut ch = [RC_MID_US; 16];
    ch[2] = RC_MIN_US;   // throttle min
    ch[4] = 2012;        // CH5 high → ArduPilot sees FS
    build_rc_frame(&ch)
}

// ── Telemetry frame types ────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct GpsFrame {
    pub lat:              f64,
    pub lon:              f64,
    pub ground_speed_kmh: f32,
    pub heading_deg:      f32,
    pub altitude_m:       f32,
    pub satellites:       u8,
}

#[derive(Debug, Clone)]
pub struct BatteryFrame {
    pub voltage_v:     f32,
    pub current_a:     f32,
    pub used_mah:      u32,
    pub remaining_pct: u8,
}

#[derive(Debug, Clone)]
pub struct AttitudeFrame {
    pub pitch_deg: f32,
    pub roll_deg:  f32,
    pub yaw_deg:   f32,
}

#[derive(Debug, Clone)]
pub struct LinkStatsFrame {
    pub uplink_rssi_1:       u8,
    pub uplink_rssi_2:       u8,
    pub uplink_link_quality: u8,
    pub uplink_snr:          i8,
}

#[derive(Debug, Clone)]
pub struct VarioFrame     { pub vertical_speed_ms: f32 }

#[derive(Debug, Clone)]
pub struct BaroAltFrame   { pub altitude_m: f32, pub vspeed_ms: f32 }

#[derive(Debug, Clone)]
pub enum CrsfFrame {
    Gps(GpsFrame),
    Battery(BatteryFrame),
    Attitude(AttitudeFrame),
    FlightMode(String),
    LinkStats(LinkStatsFrame),
    Vario(VarioFrame),
    BaroAlt(BaroAltFrame),
    RcChannels([u16; 16]),
    Heartbeat,
    Unknown(u8),
}

// ── Streaming parser ─────────────────────────────────────────────────────────

/// Feed raw bytes, get frames out. Handles partial frames, sync loss.
pub struct CrsfParser {
    buf: [u8; 64],
    len: usize,
}

impl CrsfParser {
    pub fn new() -> Self { Self { buf: [0u8; 64], len: 0 } }

    pub fn push(&mut self, data: &[u8], out: &mut Vec<CrsfFrame>) {
        for &b in data {
            // Sync hunt — discard until we see 0xC8
            if self.len == 0 && b != CRSF_SYNC { continue; }
            if self.len < 64 { self.buf[self.len] = b; self.len += 1; }

            // Need at least 3 bytes to know frame length
            if self.len < 3 { continue; }

            let frame_len = self.buf[1] as usize; // bytes after sync+len
            let total     = 2 + frame_len;

            if total > 64 {
                // Invalid length — reset
                self.len = 0; continue;
            }
            if self.len < total { continue; }

            // Full frame in buffer — check CRC
            let crc_pos  = total - 1;
            let expected = crc8_dvb_s2(&self.buf[2..crc_pos]);
            if self.buf[crc_pos] == expected {
                if let Some(f) = parse_frame(&self.buf[..total]) {
                    out.push(f);
                }
            }
            // Consume frame, keep remainder
            let remainder = self.len - total;
            self.buf.copy_within(total..self.len, 0);
            self.len = remainder;
        }
    }
}

fn parse_frame(raw: &[u8]) -> Option<CrsfFrame> {
    let ftype   = raw[2];
    let payload = &raw[3..raw.len() - 1];

    match ftype {
        TYPE_GPS if payload.len() >= 14 => {
            let lat = i32::from_be_bytes([payload[0],payload[1],payload[2],payload[3]]);
            let lon = i32::from_be_bytes([payload[4],payload[5],payload[6],payload[7]]);
            let spd = u16::from_be_bytes([payload[8], payload[9]]);
            let hdg = u16::from_be_bytes([payload[10],payload[11]]);
            let alt = u16::from_be_bytes([payload[12],payload[13]]);
            Some(CrsfFrame::Gps(GpsFrame {
                lat:              lat as f64 / 1e7,
                lon:              lon as f64 / 1e7,
                ground_speed_kmh: spd as f32 / 10.0 * 3.6,
                heading_deg:      hdg as f32 / 100.0,
                altitude_m:       alt as f32 - 1000.0,
                satellites:       if payload.len() > 14 { payload[14] } else { 0 },
            }))
        }
        TYPE_BATTERY if payload.len() >= 8 => {
            let v = u16::from_be_bytes([payload[0],payload[1]]);
            let a = u16::from_be_bytes([payload[2],payload[3]]);
            let mah = u32::from_be_bytes([0, payload[4],payload[5],payload[6]]);
            Some(CrsfFrame::Battery(BatteryFrame {
                voltage_v:     v as f32 / 10.0,
                current_a:     a as f32 / 10.0,
                used_mah:      mah,
                remaining_pct: payload[7],
            }))
        }
        TYPE_ATTITUDE if payload.len() >= 6 => {
            let p = i16::from_be_bytes([payload[0],payload[1]]);
            let r = i16::from_be_bytes([payload[2],payload[3]]);
            let y = i16::from_be_bytes([payload[4],payload[5]]);
            Some(CrsfFrame::Attitude(AttitudeFrame {
                pitch_deg: p as f32 / 10000.0 * 57.2958,
                roll_deg:  r as f32 / 10000.0 * 57.2958,
                yaw_deg:   y as f32 / 10000.0 * 57.2958,
            }))
        }
        TYPE_FLIGHT_MODE => {
            // Null-terminated string
            let s: Vec<u8> = payload.iter()
                .take_while(|&&b| b != 0)
                .cloned().collect();
            Some(CrsfFrame::FlightMode(
                String::from_utf8_lossy(&s).into_owned()
            ))
        }
        TYPE_LINK_STATS if payload.len() >= 10 => {
            Some(CrsfFrame::LinkStats(LinkStatsFrame {
                uplink_rssi_1:       payload[0],
                uplink_rssi_2:       payload[1],
                uplink_link_quality: payload[2],
                uplink_snr:          payload[3] as i8,
            }))
        }
        TYPE_VARIO if payload.len() >= 2 => {
            let v = i16::from_be_bytes([payload[0],payload[1]]);
            Some(CrsfFrame::Vario(VarioFrame { vertical_speed_ms: v as f32 / 100.0 }))
        }
        TYPE_BARO_ALT if payload.len() >= 4 => {
            let a = u16::from_be_bytes([payload[0],payload[1]]);
            let v = i16::from_be_bytes([payload[2],payload[3]]);
            Some(CrsfFrame::BaroAlt(BaroAltFrame {
                altitude_m: a as f32 / 10.0,
                vspeed_ms:  v as f32 / 100.0,
            }))
        }
        TYPE_HEARTBEAT => Some(CrsfFrame::Heartbeat),
        TYPE_RC_CHANNELS if payload.len() >= 22 => {
            let mut ch = [0u16; 16];
            for i in 0..16 {
                let bit = i * 11;
                let byte = bit / 8;
                let off  = bit % 8;
                let raw_val = (payload[byte] as u32
                    | ((payload[byte+1] as u32) << 8)
                    | ((payload[byte+2] as u32) << 16)) >> off;
                ch[i] = (raw_val & 0x7FF) as u16;
            }
            Some(CrsfFrame::RcChannels(ch))
        }
        t => Some(CrsfFrame::Unknown(t)),
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn rc_frame_length() {
        let ch = [RC_MID_US; 16];
        let f = build_rc_frame(&ch);
        assert_eq!(f[0], CRSF_SYNC);
        assert_eq!(f.len(), 26);
    }

    #[test]
    fn rc_frame_crc_valid() {
        let ch = [RC_MID_US; 16];
        let f = build_rc_frame(&ch);
        let expected = crc8_dvb_s2(&f[2..25]);
        assert_eq!(f[25], expected);
    }

    #[test]
    fn us_to_crsf_midpoint() {
        let mid = us_to_crsf(1500);
        assert!((mid as i32 - 992).abs() < 5, "mid={}", mid);
    }

    #[test]
    fn parser_roundtrip() {
        let ch = [RC_MID_US; 16];
        let frame = build_rc_frame(&ch);
        let mut parser = CrsfParser::new();
        let mut out = alloc::vec::Vec::new();
        parser.push(&frame, &mut out);
        assert_eq!(out.len(), 1);
        if let CrsfFrame::RcChannels(decoded) = &out[0] {
            // mid 1500us → crsf 992 → verify roughly symmetric
            for &v in decoded.iter() {
                assert!((v as i32 - 992).abs() < 5, "v={}", v);
            }
        } else {
            panic!("expected RcChannels");
        }
    }
}
