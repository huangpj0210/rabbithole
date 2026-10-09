#!/usr/bin/env bash
# rabbithole 负载测试:100 并发 TCP + 持续运行验证。
# 用法: tests/load.sh [二进制目录] [持续秒数(默认 600)]
set -u
BIN_DIR="${1:-$(dirname "$0")/../target/release}"
DURATION="${2:-600}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SERVER="$ROOT/$BIN_DIR/rh-server"
CLIENT="$ROOT/$BIN_DIR/rh-client"
WORK=$(mktemp -d /tmp/rh-load.XXXX)
API_PORT=19580; CTL_PORT=19600; VHOST_HTTP=19581; TCPMUX_PORT=19582
ECHO_TCP=19591; P_TCP=19601
PIDS=()

cleanup() {
  for p in "${PIDS[@]:-}"; do kill "$p" 2>/dev/null; done
  cp -r "$WORK" /tmp/rh-load-last 2>/dev/null
  rm -rf "$WORK"
}
trap cleanup EXIT

echo "== rabbithole 负载测试: 并发=100 持续=${DURATION}s ($WORK)=="

# echo 目标(多线程)
python3 - "$ECHO_TCP" <<'EOF' &
import socket, threading, sys
s = socket.socket(); s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
s.bind(("127.0.0.1", int(sys.argv[1]))); s.listen(256)
def echo(c):
    try:
        while True:
            d = c.recv(65536)
            if not d: break
            c.sendall(d)
    except OSError: pass
    finally: c.close()
while True:
    c, _ = s.accept()
    threading.Thread(target=echo, args=(c,), daemon=True).start()
EOF
PIDS+=($!)

cat > "$WORK/server.toml" <<EOF
bind_addr = "127.0.0.1"
bind_protocol = "tcp"
bind_port = $CTL_PORT
multiplex = true
[web]
bind_addr = "127.0.0.1"
port = $API_PORT
[vhost]
http_port = $VHOST_HTTP
https_port = 0
tcpmux_port = $TCPMUX_PORT
subdomain_host = "load.test"
[ports]
auto_range = "22000-23000"
[log]
level = "warn"
EOF

"$SERVER" --config "$WORK/server.toml" admin create admin Load-pass-1 >/dev/null 2>&1
"$SERVER" --config "$WORK/server.toml" >"$WORK/server.log" 2>&1 &
PIDS+=($!)
sleep 2

TOKEN=$(curl -s -X POST "http://127.0.0.1:$API_PORT/api/auth/login" \
  -H 'Content-Type: application/json' \
  -d '{"username":"admin","password":"Load-pass-1"}' \
  | python3 -c 'import json,sys; print(json.load(sys.stdin)["data"]["accessToken"])')
CR=$(curl -s -X POST "http://127.0.0.1:$API_PORT/api/clients" \
  -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json' -d '{"name":"load"}')
CID=$(echo "$CR" | python3 -c 'import json,sys; print(json.load(sys.stdin)["data"]["id"])')
CSECRET=$(echo "$CR" | python3 -c 'import json,sys; print(json.load(sys.stdin)["data"]["secret"])')

cat > "$WORK/client.toml" <<EOF
server_addr = "127.0.0.1:$CTL_PORT"
protocol = "tcp"
multiplex = true
client_id = "$CID"
secret = "$CSECRET"
log_level = "warn"
EOF
"$CLIENT" --config "$WORK/client.toml" >"$WORK/client.log" 2>&1 &
PIDS+=($!)
sleep 2

curl -s -X POST "http://127.0.0.1:$API_PORT/api/services" \
  -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json' \
  -d "{\"client_id\":\"$CID\",\"name\":\"tcp\",\"kind\":\"tcp\",\"bind_port\":$P_TCP,\"target_host\":\"127.0.0.1\",\"target_port\":$ECHO_TCP,\"pool_size\":16,\"enabled\":true}" >/dev/null
sleep 2

# 负载主体:100 并发,每连接反复 echo 校验(round 间隔休眠),持续 DURATION 秒
(while true; do echo "$(date +%s) $(netstat -an 2>/dev/null | grep -c TIME_WAIT)"; sleep 5; done > "$WORK/tw.log" 2>/dev/null) &
TWPID=$!
PIDS+=($TWPID)
python3 - "$P_TCP" "$DURATION" <<'EOF' &
import socket, sys, threading, time, random

port, duration = int(sys.argv[1]), int(sys.argv[2])
errors = []
lock = threading.Lock()
total_rounds = [0]

def worker(i):
    end = time.time() + duration
    payload = bytes([i % 256]) * 4096
    while time.time() < end:
        try:
            s = socket.create_connection(("127.0.0.1", port), timeout=15)
            s.settimeout(30)
            for _ in range(5):  # 每连接 5 轮请求-响应
                s.sendall(payload)
                got = b""
                while len(got) < len(payload):
                    d = s.recv(65536)
                    if not d:
                        raise EOFError("early eof")
                    got += d
                if got != payload:
                    raise ValueError("payload mismatch")
            s.close()
            with lock:
                total_rounds[0] += 5
            time.sleep(random.uniform(0.2, 1.0))
        except Exception as e:
            import traceback
            tb = traceback.format_exc()
            phase = "connect" if "create_connection" in tb else "round"
            with lock:
                errors.append(f"{time.time():.1f} {phase} w{i}: {type(e).__name__}")
            time.sleep(1)

threads = [threading.Thread(target=worker, args=(i,), daemon=True) for i in range(100)]
t0 = time.time()
for t in threads: t.start()
for t in threads: t.join()
dt = time.time() - t0
print(f"RESULT rounds={total_rounds[0]} errors={len(errors)} duration={dt:.0f}s")
for e in errors[:10]:
    print("ERR", e)
sys.exit(1 if errors else 0)
EOF
LOADPID=$!
wait $LOADPID
RC=$?
echo "== 负载结束 rc=$RC;抽查一次转发 =="
python3 - "$P_TCP" <<'EOF'
import socket, sys
s = socket.create_connection(("127.0.0.1", int(sys.argv[1])), timeout=10)
p = b"final-check"
s.sendall(p); s.settimeout(10)
g = b""
while len(g) < len(p):
    d = s.recv(4096)
    if not d: break
    g += d
sys.exit(0 if g == p else 1)
EOF
FINAL=$?
echo "== 负载后转发校验 rc=$FINAL =="
if [ "$RC" = "0" ] && [ "$FINAL" = "0" ]; then
  echo "== 负载测试: PASS =="
else
  echo "== 负载测试: FAIL =="
fi
exit $(( RC | FINAL ))
