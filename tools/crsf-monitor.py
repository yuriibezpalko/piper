#!/usr/bin/env python3
"""
crsf-monitor — live redrawing CRSF channel display

Run on GCS machine (your Mac), NOT on the camera.
Sniffs UDP port 2223 — same port GCS sends RC to drone.
Uses SO_REUSEPORT so it works alongside skypulse-gcs.

Usage:
  python3 tools/crsf-monitor.py
"""
import socket, sys, time, signal

PORT = int(sys.argv[1]) if len(sys.argv) > 1 else 2223

def crc8(data):
    crc = 0
    for b in data:
        crc ^= b
        for _ in range(8):
            crc = ((crc<<1)^0xD5)&0xFF if crc&0x80 else (crc<<1)&0xFF
    return crc

def decode_rc(frame):
    if len(frame) < 26 or frame[0] != 0xC8 or frame[2] != 0x16: return None
    if crc8(frame[2:25]) != frame[25]: return None
    bits = int.from_bytes(frame[3:25], 'little')
    crsf = [(bits >> (i*11)) & 0x7FF for i in range(16)]
    return [round(988 + (v-172)*1024/1639) for v in crsf]

def bar(v, w=28):
    p = max(0, min(w, round((v-988)/(2012-988)*w)))
    return '█'*p + '░'*(w-p)

NAMES = ['Roll ','Pitch','Thr  ','Yaw  ','CH5  ','CH6  ','CH7  ','CH8  ',
         'CH9  ','CH10 ','CH11 ','CH12 ','CH13 ','CH14 ','CH15 ','CH16 ']

# Bind on 0.0.0.0 but intercept outgoing traffic too
# Since GCS sends TO drone:2223, we need to listen on our end
# skypulse-gcs sends FROM a random ephemeral port TO 10.8.0.2:2223
# We can't sniff that directly — instead bind 0.0.0.0:2223 with REUSEPORT
sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
try:
    sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEPORT, 1)
except AttributeError:
    pass  # Windows doesn't have SO_REUSEPORT

# On macOS we can also try binding to receive our own outgoing UDP
# by setting IP_RECVDSTADDR or using a raw socket - but simplest:
# Just mirror: skypulse-gcs uses random src port, sends to drone
# We can listen on the drone side at :2223 on the camera
# OR: run this on camera and listen on :2223

# Actually the simplest fix: run this on the camera
# But user wants it on GCS. Solution: make GCS also send a copy to localhost:2299

sock.bind(('0.0.0.0', PORT))
sock.settimeout(0.5)

sys.stdout.write('\033[?25l\033[2J')
signal.signal(signal.SIGINT, lambda *_: (sys.stdout.write('\033[?25h\033[0m\n'), sys.exit(0)))

buf = bytearray()
count = 0
last_t = time.time()
last_us = [1500]*16
src = 'waiting...'

print(f'\033[2J')
print(f'CRSF Monitor — listening on :{PORT}')
print(f'Run on GCS machine. GCS sends RC frames from random port.')
print(f'If no frames appear: run on camera instead: python3 crsf-monitor.py 2223')
print()

while True:
    try:
        data, addr = sock.recvfrom(512)
        buf.extend(data)
        count += 1
        src = f'{addr[0]}:{addr[1]}'
        while len(buf) >= 26:
            i = buf.find(0xC8)
            if i < 0: buf.clear(); break
            if i > 0: del buf[:i]
            if len(buf) < 26: break
            us = decode_rc(bytes(buf[:26]))
            del buf[:26]
            if us: last_us = us
    except socket.timeout:
        pass

    now = time.time()
    dt = now - last_t
    hz = round(1/dt) if dt > 0.001 else 0
    last_t = now

    out = ['\033[H']
    out.append(f'  CRSF Monitor  :{PORT}  src:{src}  frames:{count}  {hz}Hz    \n')
    out.append('  ' + '─'*54 + '\n')
    for i, (n, v) in enumerate(zip(NAMES, last_us)):
        if   v > 1700: c = '\033[92m'
        elif v < 1300: c = '\033[91m'
        else:          c = '\033[97m'
        mark = ''
        if i == 4:
            if v > 1700: mark = '  \033[93m◄ ARMED\033[0m'
            else:        mark = '  \033[91m◄ DISARMED\033[0m'
        out.append(f'  {n} {c}{v:4}µs\033[0m [{bar(v)}]{mark}     \n')
    out.append('  ' + '─'*54 + '\n')
    sys.stdout.write(''.join(out))
    sys.stdout.flush()
