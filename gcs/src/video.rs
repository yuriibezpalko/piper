// gcs/src/video.rs — H.265 video receiver
// Majestic sends H.265 over UDP using RTP (RFC 7798).
// RTP header is 12 bytes. After stripping it, each packet is an H.265 NAL unit,
// which may be: VPS(32), SPS(33), PPS(34), IDR(19/20), or FU(49).
// We forward raw NAL bytes to the browser WebSocket.
// The browser handles FU-A reassembly and decoding via WebCodecs.

use tokio::{net::UdpSocket, sync::broadcast};
use bytes::Bytes;

pub async fn run(tx: broadcast::Sender<Bytes>, bind_port: u16) {
    let addr = format!("0.0.0.0:{}", bind_port);
    let sock = UdpSocket::bind(&addr).await
        .unwrap_or_else(|e| panic!("Cannot bind video UDP {}: {}", addr, e));

    let raw_fd = { use std::os::unix::io::AsRawFd; sock.as_raw_fd() };
    unsafe {
        let buf_size: libc::c_int = 4 * 1024 * 1024;
        libc::setsockopt(raw_fd, libc::SOL_SOCKET, libc::SO_RCVBUF,
            &buf_size as *const _ as *const libc::c_void,
            std::mem::size_of::<libc::c_int>() as libc::socklen_t);
    }

    log::info!("Video listening on {} (4MB recv buffer)", addr);

    let mut buf       = vec![0u8; 65536];
    let mut pkt_count = 0u64;

    loop {
        let n = match sock.recv(&mut buf).await {
            Ok(n)  => n,
            Err(e) => { log::warn!("video recv: {}", e); continue; }
        };

        if n < 13 { continue; } // need at least RTP header + 1 byte NAL

        pkt_count += 1;

        // Log first 20 packets in full detail
        if pkt_count <= 20 {
            let hex: String = buf[..n.min(16)].iter()
                .map(|b| format!("{:02X}", b)).collect::<Vec<_>>().join(" ");
            log::info!("video pkt #{} {} bytes: {}", pkt_count, n, hex);
        }

        // Majestic RTP: version bits may not be standard 0x80
        // Detect RTP by checking byte[0] top 2 bits = 10 (version=2)
        // OR by checking if stripping 12 bytes gives valid H.265 NAL type
        let rtp_version = (buf[0] >> 6) & 0x03;
        let payload_start = if rtp_version == 2 {
            // Standard RTP — strip header
            let cc = (buf[0] & 0x0F) as usize;
            let has_ext = (buf[0] >> 4) & 1 == 1;
            let base = 12 + cc * 4;
            if has_ext && n > base + 4 {
                let ext_len = ((buf[base + 2] as usize) << 8 | buf[base + 3] as usize) * 4;
                base + 4 + ext_len
            } else {
                base
            }
        } else {
            // Not standard RTP version — try as raw NAL
            0
        };

        if payload_start >= n { continue; }
        let nal = &buf[payload_start..n];
        if nal.is_empty() { continue; }

        if pkt_count <= 20 {
            let nal_type = (nal[0] >> 1) & 0x3F;
            log::info!("  → payload_start={} nal[0]=0x{:02X} nal_type={}", payload_start, nal[0], nal_type);
        }

        if tx.receiver_count() > 0 {
            let _ = tx.send(Bytes::copy_from_slice(nal));
        }
    }
}
