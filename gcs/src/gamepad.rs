// gcs/src/gamepad.rs
//
// Axis mapping matches masina/quadrofleet exactly:
//   axis[0] = Right stick X → Roll    (CH1)
//   axis[1] = Right stick Y → Pitch   (CH2)
//   axis[2] = Left  stick Y → Throttle (CH3)
//   axis[3] = Left  stick X → Yaw     (CH4)
//   axis[4] = SA switch     → ARM     (CH5)
//   axis[5] = SB switch     → Mode    (CH6)
//   axis[6] = SC/SD switch  → CH7
//
// This works for RadioMaster Boxer in joystick mode.
// Left stick Y physically stays at bottom (-32768) = min throttle.
//
// For PS4 gamepad:
//   Left stick Y returns to center → throttle will be at mid (1500µs)
//   Betaflight must be configured: "arm_on_all_throttle = ON" OR
//   use "Angle" mode with center throttle = hover
//   OR set GAMEPAD_MODE=1 to use L2/R2 triggers for throttle
//
// ARM behavior:
//   TX/Boxer: axis[4] switch — low = disarm, high = arm (same as real TX)
//   PS4 gamepad: R1 button toggles arm (press once = arm, press again = disarm)
//
// Env vars:
//   AXIS_ROLL AXIS_PITCH AXIS_THROTTLE AXIS_YAW AXIS_CH5
//   AXIS_THROTTLE_INV=1
//   GAMEPAD_MODE=1  force PS4 trigger throttle mode

use std::net::UdpSocket;
use std::sync::Arc;
use std::time::{Duration, Instant};
use crsf_proto::{build_rc_frame, RC_MIN_US, RC_MID_US, RC_MAX_US};
use crate::state::AppState;

const RC_HZ: u64  = 50;
const TICK: Duration = Duration::from_millis(1000 / RC_HZ);

fn env_u32(name: &str, default: u32) -> u32 {
    std::env::var(name).ok().and_then(|s| s.parse().ok()).unwrap_or(default)
}

pub fn run(_state: Arc<AppState>, drone_ip: &str, rc_port: u16) {
    let drone_addr = format!("{}:{}", drone_ip, rc_port);
    let sock = UdpSocket::bind("0.0.0.0:0").expect("RC UDP socket");
    sock.set_nonblocking(true).ok();

    let sdl    = sdl2::init().expect("SDL2 init");
    let js_sys = sdl.joystick().expect("SDL2 joystick subsystem");
    let mut events = sdl.event_pump().expect("SDL2 event pump");

    let mut joystick: Option<sdl2::joystick::Joystick> = None;
    let mut open_id:  Option<u32> = None;

    let mut channels     = [RC_MID_US; 16];
    channels[2]          = RC_MIN_US; // throttle starts at min
    let mut armed        = false;
    let mut arm_btn_prev = false;
    let mut tick         = Instant::now();
    let mut last_dump    = Instant::now();
    let mut frame_count  = 0u64;

    log::info!("gamepad: waiting for controller (plug in any time)");
    log::info!("gamepad: axis mapping: [0]=Roll [1]=Pitch [2]=Throttle [3]=Yaw [4]=ARM");
    log::info!("gamepad: for PS4 set GAMEPAD_MODE=1 (L2=throttle, R1=arm toggle)");

    let force_gamepad = std::env::var("GAMEPAD_MODE").as_deref() == Ok("1");

    loop {
        // SDL event pump — required for hotplug
        for event in events.poll_iter() {
            use sdl2::event::Event;
            match event {
                Event::JoyDeviceAdded { which, .. } => {
                    if joystick.is_none() {
                        if let Ok(js) = js_sys.open(which) {
                            log::info!("gamepad: '{}' connected — {} axes {} buttons",
                                js.name(), js.num_axes(), js.num_buttons());
                            open_id  = Some(js.instance_id());
                            joystick = Some(js);
                        }
                    }
                }
                Event::JoyDeviceRemoved { which, .. } => {
                    if open_id == Some(which) {
                        log::warn!("gamepad: disconnected — RC stopped");
                        joystick = None; open_id = None;
                        armed = false; arm_btn_prev = false;
                        channels[2] = RC_MIN_US;
                    }
                }
                _ => {}
            }
        }

        if joystick.is_none() {
            if js_sys.num_joysticks().unwrap_or(0) > 0 {
                if let Ok(js) = js_sys.open(0) {
                    log::info!("gamepad: '{}' found — {} axes {} buttons",
                        js.name(), js.num_axes(), js.num_buttons());
                    open_id  = Some(js.instance_id());
                    joystick = Some(js);
                }
            } else {
                let e = tick.elapsed();
                if e < TICK { std::thread::sleep(TICK - e); }
                tick = Instant::now();
                continue;
            }
        }

        let js       = joystick.as_ref().unwrap();
        let num_axes = js.num_axes();
        let num_btns = js.num_buttons();

        let get = |idx: u32| -> i16 {
            if idx < num_axes { js.axis(idx).unwrap_or(0) } else { 0 }
        };

        // ── Axis dump every 2s ─────────────────────────────────────────────
        if last_dump.elapsed() >= Duration::from_secs(2) {
            let mut s = String::from("axes:");
            for i in 0..num_axes.min(8) {
                s.push_str(&format!(" [{}]={:6}", i, js.axis(i).unwrap_or(0)));
            }
            log::info!("gamepad {} armed={} thr={}µs", s, armed, channels[2]);
            last_dump = Instant::now();
        }

        let thr_inv = std::env::var("AXIS_THROTTLE_INV").as_deref() == Ok("1");

        if force_gamepad {
            // ── PS4/Xbox gamepad mode ──────────────────────────────────────
            // Sticks: right=roll/pitch, left X=yaw
            // L2 trigger (axis 4) = throttle, rests at -32768 = min ✓
            // R1 button (btn 5) = arm toggle
            let ax_roll     = env_u32("AXIS_ROLL",     2); // right stick X
            let ax_pitch    = env_u32("AXIS_PITCH",    3); // right stick Y
            let ax_throttle = env_u32("AXIS_THROTTLE", 4); // L2 trigger
            let ax_yaw      = env_u32("AXIS_YAW",      0); // left stick X

            channels[0] = axis_to_us(get(ax_roll),      false);
            channels[1] = axis_to_us(get(ax_pitch),     true);
            channels[2] = axis_thr_trigger(get(ax_throttle), thr_inv);
            channels[3] = axis_to_us(get(ax_yaw),       false);

            // R1 = btn 5 on PS4 — toggle arm on press
            let r1 = if 5 < num_btns { js.button(5).unwrap_or(false) } else { false };
            if r1 && !arm_btn_prev {
                armed = !armed;
                if !armed { channels[2] = RC_MIN_US; }
                log::info!("gamepad: {} (R1 toggle)", if armed { "ARMED" } else { "DISARMED" });
            }
            arm_btn_prev = r1;
            channels[4] = if armed { RC_MAX_US } else { RC_MIN_US };

            // L1 = btn 4 → CH6, Triangle = btn 3 → CH7
            channels[5] = if 4 < num_btns && js.button(4).unwrap_or(false) { RC_MAX_US } else { RC_MIN_US };
            channels[6] = if 3 < num_btns && js.button(3).unwrap_or(false) { RC_MAX_US } else { RC_MIN_US };

        } else {
            // ── TX/Boxer mode (default) — matches masina axis layout ───────
            let ax_roll     = env_u32("AXIS_ROLL",     0);
            let ax_pitch    = env_u32("AXIS_PITCH",    1);
            let ax_throttle = env_u32("AXIS_THROTTLE", 2);
            let ax_yaw      = env_u32("AXIS_YAW",      3);
            let ax_ch5      = env_u32("AXIS_CH5",      4);

            channels[0] = axis_to_us(get(ax_roll),      false); // Roll: right = positive
            channels[1] = axis_to_us(get(ax_pitch),     false); // Pitch: up = negative SDL = low µs (correct)
            channels[2] = axis_thr_stick(get(ax_throttle), !thr_inv); // Throttle: down = +32767 on Boxer → invert by default
            channels[3] = axis_to_us(get(ax_yaw),       false); // Yaw: right = positive

            // CH5 from switch axis — matches masina SA switch behavior
            if ax_ch5 < num_axes {
                let v = js.axis(ax_ch5).unwrap_or(-32768);
                armed       = v > 0;
                channels[4] = if v > 0 { RC_MAX_US } else { RC_MIN_US };
            }

            // CH6, CH7 from next switch axes
            // Use axis_to_us so 3-pos switches get proper 988/1500/2012µs values
            for aux in 0..2u32 {
                let idx = ax_ch5 + 1 + aux;
                if idx < num_axes {
                    let v = js.axis(idx).unwrap_or(0);
                    // 3-pos switch: -32768 = 988, 0 = 1500, +32767 = 2012
                    channels[5 + aux as usize] = axis_switch(v);
                }
            }
        }

        // ── Send CRSF to drone ─────────────────────────────────────────────
        let frame = build_rc_frame(&channels);
        match sock.send_to(&frame, &drone_addr) {
            Ok(_) => {
                frame_count += 1;
                if frame_count % 500 == 1 {
                    log::info!("gamepad: {} frames CH[1-4]={} {} {} {} arm={}",
                        frame_count, channels[0], channels[1], channels[2], channels[3], armed);
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(e) => log::warn!("gamepad: send: {}", e),
        }
        // Mirror to localhost:2299 for crsf-monitor
        sock.send_to(&frame, "127.0.0.1:2299").ok();

        let elapsed = tick.elapsed();
        if elapsed < TICK { std::thread::sleep(TICK - elapsed); }
        tick = Instant::now();
    }
}

/// Stick axis: center → 1500µs, with deadband for hardware offset
fn axis_to_us(val: i16, invert: bool) -> u16 {
    let v = if invert { -(val as i32) } else { val as i32 };
    let v = if v.abs() < 2000 { 0 } else { v };
    (1500 + (v * 512) / 32767).clamp(RC_MIN_US as i32, RC_MAX_US as i32) as u16
}

/// Switch axis: maps full range to 988/1500/2012µs
/// Works for both 2-pos (-32768/+32767) and 3-pos (-32768/0/+32767) switches
fn axis_switch(val: i16) -> u16 {
    let v = val as i32;
    if v < -10000 { RC_MIN_US as u16 }       // low position
    else if v > 10000 { RC_MAX_US as u16 }   // high position
    else { RC_MID_US as u16 }                 // middle position
}

/// Trigger throttle: -32768 = 988µs min, +32767 = 2012µs max
fn axis_thr_trigger(val: i16, invert: bool) -> u16 {
    let v = if invert { -(val as i32 + 32768) } else { val as i32 + 32768 };
    (988 + (v * 1024) / 65535).clamp(RC_MIN_US as i32, RC_MAX_US as i32) as u16
}

/// TX throttle stick: physically stays at bottom
/// Boxer: stick full down = +32767, full up = -32768
/// With default invert=true: down(+32767) → negated(-32767) → 988µs ✓
fn axis_thr_stick(val: i16, invert: bool) -> u16 {
    let v = if invert { -(val as i32) } else { val as i32 };
    (1500 + (v * 512) / 32767).clamp(RC_MIN_US as i32, RC_MAX_US as i32) as u16
}
