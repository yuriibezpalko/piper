# SkyPulse GCS — Deployment Guide

> OpenIPC SSC30KQ · ArduPilot · WireGuard · Starlink

---

## 1. System Overview

```
Gamepad → GCS (CRSF/UDP) → WireGuard VPS → OpenIPC camera → UART → ArduPilot FC
ArduPilot FC → UART → OpenIPC camera → WireGuard VPS → GCS → Browser OSD
Majestic H.265 → UDP → WireGuard VPS → GCS → WebSocket → Browser
```

| Component | Description |
|---|---|
| `skypulse-drone` | C++ binary on OpenIPC SSC30KQ — CRSF/UART bridge + failsafe FSM |
| `skypulse-gcs` | Rust binary on laptop — SDL2 gamepad + Axum web server |
| Browser UI | `index.html` — H.265 WebCodecs + Canvas OSD + Leaflet GPS map |
| `crsf-proto` | Rust library — CRSF encode/decode |
| WireGuard | VPN mesh: camera `10.0.0.1` ↔ VPS `10.0.0.254` ↔ GCS `10.0.0.2` |

---

## 2. Prerequisites

**Hardware**
- OpenIPC camera with SSC30KQ SoC
- ArduPilot flight controller — UART TX/RX exposed
- VPS with public IP for WireGuard relay
- Starlink or 4G on drone side
- Xbox / PlayStation / RC USB controller on GCS
- GCS machine: Linux, macOS, or Windows + Chrome

**GCS machine**
- Rust: https://rustup.rs
- SDL2 dev library (see §7)
- WireGuard: https://www.wireguard.com/install

**Build machine (for drone binary)**
```bash
# Download OpenIPC toolchain from:
# https://github.com/openipc/firmware/releases/tag/latest
# Extract to /opt/openipc-toolchain and add to PATH
```

---

## 3. WireGuard Setup

Your VPS is already running **wg-easy**. Network layout:

| Node | WireGuard IP |
|---|---|
| VPS (wg-easy) | `10.8.0.1` |
| Camera | `10.8.0.2` |
| GCS (your laptop) | `10.8.0.3` |

### Add camera peer in wg-easy UI

Open wg-easy web UI → Add client → name it `skypulse` → download config.
You get a `.conf` with the camera private key and VPS public key.

### Camera — `/etc/wireguard.conf`
```ini
[Interface]
PrivateKey = <from wg-easy download>

[Peer]
PublicKey    = s8RoICQwmwrkDyIcDPTwWKr3S5yUeUsLMN4WJHarplI=
PresharedKey = jNroFS2/NwU7Xik/+MeEP54ZlJ9pXidkBqCSHtfr960=
AllowedIPs   = 10.8.0.0/24
Endpoint     = <YOUR_VPS_IP>:51820
PersistentKeepalive = 25
```

The `drop/overlay/` files set camera IP to `10.8.0.2` and ping `10.8.0.1` at boot.

### GCS — add peer in wg-easy UI

Add a second client named `gcs` → download config → install WireGuard → import.

```bash
# macOS
brew install wireguard-tools
wg-quick up ~/Downloads/gcs.conf

# Verify
ping 10.8.0.1   # VPS
ping 10.8.0.2   # camera
```

### Enable IP forwarding on VPS

wg-easy PostUp rules already handle this, but verify:
```bash
sysctl net.ipv4.ip_forward  # should be 1
```

---

## 4. Build & Deploy Drone Binary

### Build
```bash
cd skypulse/drone-client
make
file skypulse-drone
# → ELF 32-bit LSB executable, ARM, EABI5, statically linked
```

### Deploy
```bash
make deploy IP=10.0.0.1
# Copies binary + S95skypulse + skypulse.conf to camera
```

### Camera config — `/etc/skypulse.conf`
```ini
host=10.0.0.2           # GCS WireGuard IP
control_port=2223       # RC frames GCS → camera
caminfo_port=2224       # telemetry camera → GCS
uart_dev=/dev/ttyS1     # SSC30KQ UART to FC
uart_baud=420000        # exact rate via termios2/BOTHER
STABILIZE_TIMEOUT=250   # ms — neutral sticks after link glitch
FAILSAFE_TIMEOUT=5000   # ms — CH5 high → ArduPilot RTH/Land
LOCAL_TIMEOUT=300000    # ms — GPIO → local ELRS takeover
ELRS_SWITCH_PIN=0       # GPIO pin for ELRS switchover
```

### Drop overlay files
```bash
# Copy overlay to camera
scp -r drone-client/drop/overlay/* root@10.0.0.1:/
ssh root@10.0.0.1 chmod +x /etc/init.d/S97skypulse /etc/rc.local
```

Then edit `/etc/wireguard.conf` on the camera and fill in your actual keys.

> **Important:** `rc.local` waits for WireGuard to connect before restarting Majestic. This ensures video streams to the correct GCS IP. Do not skip this file.

---

## 5. Majestic Video Config

Edit `/etc/majestic.yaml` on the camera:

```yaml
video0:
  enabled: true
  codec: h265
  size: 1280x720
  fps: 30
  bitrate: 2048      # kbps
  gopSize: 1         # keyframe every second — REQUIRED for WebCodecs
  rcMode: cbr

outgoing:
  enabled: true
  url: rtp://10.0.0.2:5600   # GCS WireGuard IP
  codec: h265
```

```bash
killall -1 majestic
```

> **`gopSize: 1` is critical.** Without it Chrome's WebCodecs decoder may wait 10+ seconds for the first keyframe before showing video.

---

## 6. ArduPilot Parameters

Set in Mission Planner or QGroundControl. Adjust `SERIAL1` to whichever port is wired to the camera.

### CRSF receiver
| Parameter | Value | Notes |
|---|---|---|
| `SERIAL1_PROTOCOL` | `23` | RCIN — CRSF |
| `SERIAL1_BAUD` | `460` | 460800 baud |
| `RC_PROTOCOLS` | `536` | Enable CRSF + auto |

### Failsafe
| Parameter | Value | Notes |
|---|---|---|
| `FS_THR_ENABLE` | `3` | RTL on RC failsafe (recommended) |
| `FS_THR_VALUE` | `975` | Below 988µs = failsafe |
| `FS_GCS_ENABLE` | `0` | Disable — we handle FS via CRSF |

ArduPilot automatically sends CRSF telemetry (GPS, battery, attitude, flight mode) back on the same UART when it detects a CRSF receiver. No extra config needed.

---

## 7. Build & Run GCS

### Install SDL2
```bash
# Ubuntu / Debian
sudo apt install libsdl2-dev

# macOS
brew install sdl2
# If cargo still can't find it, add to ~/.cargo/config.toml:
# [env]
# LIBRARY_PATH = "/opt/homebrew/lib"

# Windows (MSYS2 — easiest)
pacman -S mingw-w64-x86_64-SDL2
# Or download SDL2-devel-x.x.x-mingw.zip from https://libsdl.org
# Extract and set SDL2_DIR env var pointing to the mingw64 folder
```

> Do NOT add `features = ["static-link"]` to the sdl2 dependency — it requires `SDL2main` as a separate static lib which is not available on most systems. Dynamic linking (the default) works on all platforms.

### Build
```bash
cd skypulse
cargo build --release -p skypulse-gcs
```

### Run
```bash
DRONE_IP=10.8.0.2 ./target/release/skypulse-gcs

# All options:
DRONE_IP=10.8.0.2  \
RC_PORT=2223        \
TELEM_PORT=2224     \
VIDEO_PORT=5600     \
WEB_PORT=8080       \
CELL_COUNT=4        \   # optional: set battery cell count (0 = auto-detect)
TRACCAR_URL=http://your-traccar.com:8082  \  # optional: enable GPS tracking
TRACCAR_ID=skypulse-1  \                     # optional: device ID in Traccar
  ./target/release/skypulse-gcs
```

### Open UI
```
http://localhost:8080
```

> **Must use Chrome or Edge.** Firefox does not support H.265 in the WebCodecs API.

---

## 8. Verification

```bash
# 1. WireGuard mesh
ping 10.0.0.1    # camera reachable from GCS
ping 10.0.0.254  # VPS reachable

# 2. Telemetry flowing (should see CRSF bytes: C8 xx xx ...)
nc -ul 2224 | xxd | head -5

# 3. Video arriving
sudo tcpdump -i wg0 udp port 5600 -c 5

# 4. RC reaching camera (run on camera, move gamepad)
nc -ul 2223 | xxd | head -3

# 5. Drone binary running on camera
ssh root@10.0.0.1 'ps | grep skypulse'
```

In Chrome DevTools console on `localhost:8080` you should see:
```
H.265 decoder ready
```

---

## 9. Failsafe Behaviour

| Gap since last RC packet | State | What happens |
|---|---|---|
| < 250ms | NORMAL | Last valid RC frame forwarded to FC |
| 250ms – 5s | STABILIZE | Neutral sticks, throttle min, CH5 normal. ArduPilot holds altitude. |
| 5s – 5min | FAILSAFE | CH5 high. ArduPilot triggers FS_THR action (RTL or Land). |
| > 5min | LOCAL | GPIO clear. Switches to local ELRS receiver as backup. |

---

## 10. Troubleshooting

**No video in browser**
- Check Majestic running: `ssh root@10.0.0.1 'ps | grep majestic'`
- Verify `gopSize: 1` in majestic.yaml
- Check UDP arriving: `tcpdump -i wg0 udp port 5600`
- Must use Chrome/Edge, not Firefox
- Check DevTools console for WebCodecs errors

**No telemetry / OSD blank**
- Check skypulse-drone running: `ssh root@10.0.0.1 'ps | grep skypulse'`
- Check UART wiring: camera TX → FC RX, camera RX → FC TX
- Verify `SERIAL1_PROTOCOL=23` and `SERIAL1_BAUD=460` in ArduPilot
- Test: `nc -ul 2224 | xxd | head`

**Gamepad not detected**
- Check SDL2 installed
- Linux: `sudo chmod a+rw /dev/input/js0`
- Test gamepad: `jstest /dev/input/js0`

**WireGuard not connecting**
- Check VPS firewall allows UDP 51820
- Verify keys — one wrong character breaks everything
- Camera: `wg show` to see handshake status

**Serial / baud rate error on camera**
- SSC30KQ requires `termios2 + BOTHER` for 420000 baud — standard termios won't work
- Verify asm-generic headers present in OpenIPC sysroot
- Check compile output for `TCGETS2`/`TCSETS2` errors

---

## 11. Port Reference

| Port | Direction | Purpose |
|---|---|---|
| UDP 2223 | GCS → camera | RC control — CRSF frames |
| UDP 2224 | camera → GCS | Telemetry — CRSF frames |
| UDP 5600 | camera → GCS | Video — H.265 RTP |
| TCP 8080 | localhost | Web UI — browser interface |
| UDP 51820 | VPS | WireGuard relay (must be open in firewall) |
| WS `/ws/telemetry` | GCS → browser | JSON telemetry stream |
| WS `/ws/video` | GCS → browser | Binary H.265 NAL units |

---

## 12. SSC30KQ UART Notes

The FC RCIN connection is on `/dev/ttyS3` at 420000 baud.

**Critical — disable OpenIPC serial console on ttyS3 before running:**

```bash
# Check if anything is using ttyS3
fuser /dev/ttyS3

# If getty/console is running on it, kill it and disable in inittab
sed -i 's/.*ttyS3.*/#&/' /etc/inittab
kill $(fuser /dev/ttyS3 2>/dev/null) 2>/dev/null

# The S97skypulse init script does this automatically on start
```

**Verify UART is working:**
```bash
# On camera — start skypulse-drone manually, watch output
/usr/bin/skypulse-drone /etc/skypulse.conf

# Should see:
# [serial] /dev/ttyS3 @ 420000 baud OK
# [status] serial_rx=NNN bytes ...   ← NNN > 0 means FC is sending telemetry
```

**WireGuard IPs (your setup):**

| Node | IP |
|---|---|
| VPS | `10.8.0.1` |
| Camera (drone) | `10.8.0.2` |
| GCS (laptop) | `10.8.0.3` |

**Run GCS (no DRONE_IP needed — 10.8.0.2 is now the default):**
```bash
./target/release/skypulse-gcs
```
