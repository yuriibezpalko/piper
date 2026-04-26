# Piper firmware build

OpenIPC firmware drop for SigmaStar SSC338Q / SSC30KQ. Same approach as
`quadrofleet-masina/client/drop` — files in this directory are layered on top
of a clone of `OpenIPC/firmware`, and upstream's Makefile drives the build.

## What gets baked in

| Component | Source in this tree | Installed at |
|---|---|---|
| `skypulse-drone` (compiled from `drone-client/main.cpp`) | `general/package/skypulse/` | `/usr/bin/skypulse-drone` |
| `S97skypulse` init | `general/package/skypulse/files/` | `/etc/init.d/S97skypulse` |
| `skypulse.conf` | `general/package/skypulse/files/` | `/etc/skypulse.conf` |
| `msposd` (prebuilt `msposd_star6e`) | `general/package/msposd/` | `/usr/bin/msposd` |
| INAV fonts | downloaded by `msposd-prebuilt.mk` | `/usr/share/fonts/font.png`, `font_hd.png` |
| `S96msposd` init | `general/package/msposd/files/` | `/etc/init.d/S96msposd` |
| `msposd.conf` | `general/package/msposd/files/` | `/etc/msposd.conf` |
| `wireguard.conf` (placeholder) | `general/overlay/etc/` | `/etc/wireguard.conf` |
| `wg0` / `wwan0` interface defs | `general/overlay/etc/network/interfaces.d/` | same path |
| `rc.local` boot orchestration | `general/overlay/etc/` | `/etc/rc.local` |
| `setup_wwan.sh` (4G QMI bring-up) | `general/overlay/usr/bin/` | `/usr/bin/setup_wwan.sh` |
| `ssc338q_skypulse` / `ssc30kq_skypulse` defconfigs | `br-ext-chip-sigmastar/configs/` | merged into upstream tree |

Boot order: `rc.local` brings up the uplink (`wwan0` or `usb0` or `eth0`),
ifups `wg0`, waits for the VPS to respond, then SIGHUPs `majestic`. Init.d
slots `S96msposd` and `S97skypulse` start the daemons after networking.

## Build with Docker (recommended)

```bash
# from piper/drone-client/drop/
./build.sh ssc338q_skypulse        # or ssc30kq_skypulse
./build.sh                         # interactive whiptail picker
```

Builds a Debian-based container with the buildroot toolchain, runs the firmware
build inside it, and writes artefacts to `./output/images/`. The expensive
state — `.openipc/` clone and `output/` build tree — is bind-mounted from the
host so subsequent runs are incremental.

## Build natively (Linux)

```bash
make deps                          # one-time apt install
make BOARD=ssc338q_skypulse        # → output/images/openipc.ssc338q-nand-fpv.tgz
make list                          # all available board configs
```

`make` with no `BOARD=` opens upstream's interactive whiptail picker (after
staging).

## How the staging works

The top-level `Makefile` here is a thin wrapper:

1. `prepare` — `git clone --depth 1` of `OpenIPC/firmware` into `.openipc/` (cached).
2. `stage` — `cp -rf` our `general/`, `br-ext-chip-sigmastar/`, and the live
   `../main.cpp` into the appropriate spots in `.openipc/`. Appends our two
   `Config.in` source lines to `.openipc/general/package/Config.in` so the
   skypulse and msposd packages are visible to Kconfig.
3. `make -C .openipc BOARD=...` — hands off to upstream's Makefile.

This means **upstream Makefile changes Just Work** — we don't fork it. The only
files we ever touch in `.openipc/` are additive overlays and the Config.in
source lines (which are idempotently appended).

## Customising

| What | Where |
|---|---|
| WireGuard keys | `general/overlay/etc/wireguard.conf` (replace `<WG_*>` placeholders) |
| GCS IP / failsafe timings | `general/package/skypulse/files/skypulse.conf` |
| msposd args | `general/package/msposd/files/msposd.conf` |
| 4G APN | `general/overlay/usr/bin/setup_wwan.sh` (`APN=` line) |
| Add another board | drop a defconfig into `br-ext-chip-sigmastar/configs/` |

## Targets cheatsheet

```bash
make BOARD=<name>              # full firmware build
make stage                     # stage drop into .openipc/, no build
make list                      # available boards
make clean                     # buildroot clean (keeps .openipc/)
make distclean                 # nuke .openipc/
make -C .openipc br-skypulse   # rebuild only skypulse
make -C .openipc br-msposd-prebuilt
```
