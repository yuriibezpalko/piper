// drone-client/main.cpp — skypulse-drone for OpenIPC SSC30KQ
//
// Flow:
//   Serial RX (ArduPilot CRSF telem) → UDP sendto GCS telem port (2224)
//   UDP recvfrom GCS rc port (2223)  → FSM → write CRSF to serial
//   Every 20ms: write current crsfPacket to FC
//
// Failsafe FSM (masina pattern):
//   <250ms   NORMAL     — forward live RC from GCS
//   250ms-5s STABILIZE  — neutral sticks
//   5s-5min  FAILSAFE   — CH5 high (ArduPilot RTH/Land)
//   >5min    LOCAL      — gpio clear → ELRS takeover

#include <iostream>
#include <fstream>
#include <sstream>
#include <string>
#include <cstring>
#include <cstdlib>
#include <thread>
#include <chrono>

#include <unistd.h>
#include <fcntl.h>
#include <errno.h>
#include <sys/socket.h>
#include <sys/ioctl.h>
#include <netinet/in.h>
#include <arpa/inet.h>
#include <netdb.h>

#include <asm/termbits.h>
#include <asm/ioctls.h>

// ── Config ────────────────────────────────────────────────────────────────────

static std::string g_host           = "10.8.0.3"; // GCS WireGuard IP
static int         g_rc_port        = 2223;        // GCS→camera RC
static int         g_telem_port     = 2224;        // camera→GCS telemetry
static int         g_local_timeout  = 300000;
static int         g_fs_timeout     = 5000;
static int         g_stab_timeout   = 250;
static int         g_elrs_pin       = 0;
static std::string g_uart_dev       = "/dev/ttyS3";
static int         g_uart_baud      = 420000;
static bool        g_verbose        = true;        // verbose logging

static void readConfig(const std::string &path) {
    std::ifstream f(path);
    if (!f.is_open()) {
        printf("[config] %s not found, using defaults\n", path.c_str());
        return;
    }
    printf("[config] loading %s\n", path.c_str());
    std::string line;
    while (std::getline(f, line)) {
        if (line.empty() || line[0] == '#') continue;
        std::istringstream iss(line);
        std::string key, val;
        if (std::getline(iss, key, '=') && std::getline(iss, val)) {
            // trim whitespace
            while (!key.empty() && isspace(key.back())) key.pop_back();
            while (!val.empty() && isspace(val.front())) val.erase(val.begin());
            if      (key == "host")              g_host          = val;
            else if (key == "control_port")      g_rc_port       = std::stoi(val);
            else if (key == "caminfo_port")      g_telem_port    = std::stoi(val);
            else if (key == "LOCAL_TIMEOUT")     g_local_timeout = std::stoi(val);
            else if (key == "FAILSAFE_TIMEOUT")  g_fs_timeout    = std::stoi(val);
            else if (key == "STABILIZE_TIMEOUT") g_stab_timeout  = std::stoi(val);
            else if (key == "ELRS_SWITCH_PIN")   g_elrs_pin      = std::stoi(val);
            else if (key == "uart_dev")          g_uart_dev      = val;
            else if (key == "uart_baud")         g_uart_baud     = std::stoi(val);
            printf("[config]   %s = %s\n", key.c_str(), val.c_str());
        }
    }
}

// ── Serial ────────────────────────────────────────────────────────────────────

static int openSerial(const std::string &dev, int baud) {
    int fd = open(dev.c_str(), O_RDWR);
    if (fd < 0) {
        perror(("open " + dev).c_str());
        printf("[serial] ERROR: cannot open %s — check uart_dev in config\n", dev.c_str());
        printf("[serial] Available ports:\n");
        system("ls /dev/ttyS* /dev/ttyAMA* 2>/dev/null");
        return -1;
    }

    struct termios2 tio;
    if (ioctl(fd, TCGETS2, &tio) != 0) {
        perror("TCGETS2");
        close(fd);
        return -1;
    }

    tio.c_cflag = 7344;       // CS8|CREAD|CLOCAL|HUPCL — masina verified value
    tio.c_cflag &= ~CBAUD;
    tio.c_cflag |= BOTHER;    // custom baud
    tio.c_ispeed = baud;
    tio.c_ospeed = baud;
    tio.c_iflag  = 0;
    tio.c_oflag  = 0;
    tio.c_lflag  = 0;
    tio.c_cc[VTIME] = 0;      // no timeout — pure non-blocking
    tio.c_cc[VMIN]  = 0;      // return immediately with whatever is available

    if (ioctl(fd, TCSETS2, &tio) != 0) {
        perror("TCSETS2");
        close(fd);
        return -1;
    }

    int flags = fcntl(fd, F_GETFL, 0);
    fcntl(fd, F_SETFL, flags | O_NONBLOCK);

    printf("[serial] %s @ %d baud OK (fd=%d)\n", dev.c_str(), baud, fd);
    return fd;
}

// ── UDP ───────────────────────────────────────────────────────────────────────

static int makeSendSock(const std::string &host, int port, sockaddr_in &out) {
    int fd = socket(AF_INET, SOCK_DGRAM, 0);
    memset(&out, 0, sizeof(out));
    out.sin_family = AF_INET;
    out.sin_port   = htons(port);
    if (inet_pton(AF_INET, host.c_str(), &out.sin_addr) != 1) {
        struct addrinfo hints{}, *res;
        hints.ai_family   = AF_INET;
        hints.ai_socktype = SOCK_DGRAM;
        if (getaddrinfo(host.c_str(), nullptr, &hints, &res) == 0) {
            out = *(sockaddr_in*)res->ai_addr;
            out.sin_port = htons(port);
            freeaddrinfo(res);
        }
    }
    char ipstr[INET_ADDRSTRLEN];
    inet_ntop(AF_INET, &out.sin_addr, ipstr, sizeof(ipstr));
    printf("[udp] send socket → %s:%d (fd=%d)\n", ipstr, port, fd);
    int fl = fcntl(fd, F_GETFL, 0);
    fcntl(fd, F_SETFL, fl | O_NONBLOCK);
    return fd;
}

static int makeRecvSock(int port) {
    int fd = socket(AF_INET, SOCK_DGRAM, 0);
    sockaddr_in addr{};
    addr.sin_family      = AF_INET;
    addr.sin_port        = htons(port);
    addr.sin_addr.s_addr = INADDR_ANY;
    if (bind(fd, (sockaddr*)&addr, sizeof(addr)) < 0) {
        perror(("bind port " + std::to_string(port)).c_str());
        return -1;
    }
    int fl = fcntl(fd, F_GETFL, 0);
    fcntl(fd, F_SETFL, fl | O_NONBLOCK);
    printf("[udp] recv socket listening on :%d (fd=%d)\n", port, fd);
    return fd;
}

// ── Hardcoded failsafe CRSF packets (masina-verified) ────────────────────────

static const uint8_t CRSF_STABILIZE[26] = {
    200,24,22,224,3,31,248,192,39,112,129,203,66,
    22,224,3,31,248,192,7,62,240,129,15,124,173
};
static const uint8_t CRSF_FAILSAFE[26] = {
    200,24,22,224,3,31,248,192,39,112,129,203,66,
    224,224,3,31,248,192,7,62,240,129,15,124,73
};

// ── Camera info thread ────────────────────────────────────────────────────────

static int getCpuTemp() {
    std::ifstream f("/sys/devices/virtual/mstar/msys/TEMP_R");
    if (!f.is_open()) return -1;
    std::string line; std::getline(f, line);
    size_t pos = line.find("Temperature ");
    if (pos == std::string::npos) return -1;
    try { return std::stoi(line.substr(pos + 12)); } catch (...) { return -1; }
}

struct NetUsage { unsigned long rx, tx; };
static NetUsage getNetUsage() {
    std::ifstream f("/proc/net/dev");
    std::string line; std::getline(f, line); std::getline(f, line);
    NetUsage u{0,0};
    while (std::getline(f, line)) {
        std::istringstream iss(line); std::string iface; iss >> iface;
        iface.pop_back();
        if (iface == "lo") continue;
        unsigned long rx, tx;
        iss >> rx;
        for (int i = 0; i < 7; i++) iss >> tx;
        iss >> tx;
        u.rx += rx; u.tx += tx;
    }
    return u;
}

static void caminfoThread(int sock, sockaddr_in addr) {
    printf("[caminfo] thread started\n");
    while (true) {
        auto u1 = getNetUsage();
        std::this_thread::sleep_for(std::chrono::milliseconds(500));
        auto u2 = getNetUsage();
        unsigned long rx_kb = (u2.rx - u1.rx) / 512;
        unsigned long tx_kb = (u2.tx - u1.tx) / 512;
        int temp = getCpuTemp();
        std::string msg = "Temp:" + std::to_string(temp)
            + " R:" + std::to_string(rx_kb)
            + " T:" + std::to_string(tx_kb) + "\n";
        sendto(sock, msg.c_str(), msg.size(), 0, (const sockaddr*)&addr, sizeof(addr));
    }
}

// ── Main ──────────────────────────────────────────────────────────────────────

int main() {
    readConfig("/root/config.txt");
    readConfig("/etc/skypulse.conf");

    printf("\n=== skypulse-drone ===\n");
    printf("  GCS host:  %s\n", g_host.c_str());
    printf("  RC port:   %d (recv from GCS)\n", g_rc_port);
    printf("  Telem port:%d (send to GCS)\n",   g_telem_port);
    printf("  UART:      %s @ %d baud\n",        g_uart_dev.c_str(), g_uart_baud);
    printf("  Failsafe:  stab=%dms fs=%dms local=%dms\n",
        g_stab_timeout, g_fs_timeout, g_local_timeout);
    printf("======================\n\n");

    // Open serial to FC
    int serial = openSerial(g_uart_dev, g_uart_baud);
    if (serial < 0) {
        printf("[FATAL] Cannot open serial port — exiting\n");
        return 1;
    }

    // Telemetry send socket → GCS telem port (2224)
    sockaddr_in telemAddr;
    int telemSock = makeSendSock(g_host, g_telem_port, telemAddr);

    // RC receive socket ← GCS rc port (2223)
    int rcSock = makeRecvSock(g_rc_port);
    if (rcSock < 0) return 1;

    // Announce to GCS
    const char* init = "SKYPULSE_INIT";
    sendto(telemSock, init, strlen(init), 0, (sockaddr*)&telemAddr, sizeof(telemAddr));
    printf("[udp] SKYPULSE_INIT sent to GCS\n");

    // Camera info thread (sends to telem port so GCS sees it too)
    sockaddr_in caminfoAddr;
    int caminfoSock = makeSendSock(g_host, g_telem_port, caminfoAddr);
    std::thread camThr(caminfoThread, caminfoSock, caminfoAddr);
    camThr.detach();

    // State
    uint8_t crsfPacket[26];
    memcpy(crsfPacket, CRSF_STABILIZE, 26);

    auto lastValidPayload = std::chrono::high_resolution_clock::now();
    auto lastSentPayload  = std::chrono::high_resolution_clock::now();
    auto lastLog          = std::chrono::high_resolution_clock::now();

    uint8_t serialBuf[256];
    uint8_t rxBuf[128];

    // Counters for verbose logging
    unsigned long serial_rx_bytes  = 0;
    unsigned long udp_rx_packets   = 0;
    unsigned long udp_tx_packets   = 0;
    unsigned long serial_tx_frames = 0;
    // FSM state tracking
    std::string fsName = "NORMAL";

    printf("[main] entering main loop\n");

    while (true) {
        usleep(1000); // 1ms tick

        // ── 1. Serial RX → UDP to GCS (telemetry relay) ──────────────────
        int n = read(serial, serialBuf, sizeof(serialBuf));
        if (n > 0) {
            serial_rx_bytes += n;
            // Forward raw bytes to GCS telem port
            ssize_t sent = sendto(telemSock, serialBuf, n, 0,
                (sockaddr*)&telemAddr, sizeof(telemAddr));
            if (sent > 0) udp_tx_packets++;
            if (g_verbose && serial_rx_bytes % 1000 < (size_t)n) {
                printf("[serial→udp] %d bytes read, sent=%zd (total_rx=%lu)\n",
                    n, sent, serial_rx_bytes);
                // Print first 6 bytes as hex for debugging
                printf("[serial→udp] hex:");
                for (int i = 0; i < n && i < 6; i++)
                    printf(" %02X", serialBuf[i]);
                printf("\n");
            }
        } else if (n < 0 && errno != EAGAIN && errno != EWOULDBLOCK) {
            printf("[serial] read error: %s\n", strerror(errno));
        }

        // ── 2. UDP RC from GCS → FSM ──────────────────────────────────────
        sockaddr_in clientAddr{};
        socklen_t addrLen = sizeof(clientAddr);
        ssize_t got = recvfrom(rcSock, rxBuf, sizeof(rxBuf), 0,
            (sockaddr*)&clientAddr, &addrLen);

        auto now = std::chrono::high_resolution_clock::now();
        auto ms_since_rc = std::chrono::duration_cast<std::chrono::milliseconds>(
            now - lastValidPayload).count();

        if (got > 0) {
            udp_rx_packets++;
            if (got == 26 && rxBuf[0] == 0xC8) {
                // Valid CRSF RC frame
                lastValidPayload = now;
                memcpy(crsfPacket, rxBuf, 26);
                if (udp_rx_packets % 50 == 1) {
                    printf("[udp←GCS] RC frame #%lu received (26 bytes, 0xC8)\n",
                        udp_rx_packets);
                }
            } else {
                printf("[udp←GCS] non-CRSF packet: %zd bytes, first=0x%02X\n",
                    got, rxBuf[0]);
            }
        } else if (got < 0 && errno != EAGAIN && errno != EWOULDBLOCK) {
            printf("[udp] recvfrom error: %s\n", strerror(errno));
        }

        // Failsafe FSM
        std::string newFsName;
        if (ms_since_rc >= g_local_timeout) {
            newFsName = "LOCAL";
            memcpy(crsfPacket, CRSF_FAILSAFE, 26);
            std::system(("gpio clear " + std::to_string(g_elrs_pin)).c_str());
        } else if (ms_since_rc >= g_fs_timeout) {
            newFsName = "FAILSAFE";
            memcpy(crsfPacket, CRSF_FAILSAFE, 26);
        } else if (ms_since_rc >= g_stab_timeout) {
            newFsName = "STABILIZE";
            memcpy(crsfPacket, CRSF_STABILIZE, 26);
        } else {
            newFsName = "NORMAL";
        }
        if (newFsName != fsName) {
            printf("[FSM] %s → %s (gap=%ldms)\n", fsName.c_str(), newFsName.c_str(), ms_since_rc);
            fsName = newFsName;
        }

        // ── 3. Write CRSF to FC every 20ms ───────────────────────────────
        auto ms20 = std::chrono::duration_cast<std::chrono::milliseconds>(
            now - lastSentPayload).count();
        if (ms20 >= 20) {
            ssize_t w = write(serial, crsfPacket, 26);
            if (w == 26) {
                serial_tx_frames++;
            } else if (w < 0) {
                printf("[serial] write error: %s\n", strerror(errno));
            }
            lastSentPayload = now;
        }

        // ── 4. Status log every 5 seconds ─────────────────────────────────
        auto log_ms = std::chrono::duration_cast<std::chrono::milliseconds>(
            now - lastLog).count();
        if (log_ms >= 5000) {
            printf("[status] serial_rx=%lu bytes  udp_rx=%lu pkts  udp_tx=%lu pkts  fc_writes=%lu  FSM=%s  gap=%ldms\n",
                serial_rx_bytes, udp_rx_packets, udp_tx_packets,
                serial_tx_frames, fsName.c_str(), ms_since_rc);
            lastLog = now;
        }
    }

    close(serial);
    close(telemSock);
    close(rcSock);
    return 0;
}
