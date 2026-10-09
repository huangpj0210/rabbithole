#!/usr/bin/env bash
# rabbithole vhost 进阶 E2E:
#   - HTTPS SNI 路由(A4):自签证书 + curl --resolve,按 SNI 域名分流到两条规则,
#     未知 SNI 拒绝(服务端 peek ClientHello 不终结 TLS,由客户端插件终结)
#   - HTTP 头改写(C11):host_rewrite + request_headers(set/del)+ response_headers
#   - HTTP Basic 访问鉴权(C12):无凭据 401 / 错误凭据 401 / 正确凭据 200
#   - 自定义 404 页(C27):未匹配域名返回服务端配置的 404 文件
# 用法: tests/e2e-vhost.sh [二进制目录(默认 target/release)]
set -u
BIN_DIR="${1:-$(dirname "$0")/../target/release}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SERVER="$ROOT/$BIN_DIR/rh-server"
CLIENT="$ROOT/$BIN_DIR/rh-client"
WORK=$(mktemp -d /tmp/rh-vhost.XXXXXX)
API=30780; CTL=30700; VH_HTTP=30701; VH_HTTPS=30702; HDR1=30703; HDR2=30704; ORIGIN=30705
PASS=0; FAIL=0; PIDS=()
cleanup(){
  for p in "${PIDS[@]:-}"; do kill "$p" 2>/dev/null; done
  rm -rf /tmp/rh-vhost-last; cp -r "$WORK" /tmp/rh-vhost-last 2>/dev/null
  rm -rf "$WORK"
}
trap cleanup EXIT
ok()   { PASS=$((PASS+1)); echo "PASS: $1"; }
bad()  { FAIL=$((FAIL+1)); echo "FAIL: $1"; }
check(){ [ "$1" = "0" ] && ok "$2" || bad "$2"; }

echo "== rabbithole vhost 进阶 E2E($WORK)=="
for pid in $(ps -eo pid,args | grep -E "rh-(client|server) --config /tmp/rh-vhost" | grep -v grep | awk '{print $1}'); do kill "$pid" 2>/dev/null; done

# 自签证书(SAN 含两个 SNI 域名;客户端插件用其终结 TLS)
openssl req -x509 -newkey rsa:2048 -nodes -keyout "$WORK/tls.key" -out "$WORK/tls.crt" \
  -days 2 -subj "/CN=one.vh.test" -addext "subjectAltName=DNS:one.vh.test,DNS:two.vh.test,DNS:vh.test,IP:127.0.0.1" >/dev/null 2>&1

# 上游记录服务(端口,日志路径;记录完整请求头,回 200 OK/2 字节)
start_hdr(){
python3 - "$1" "$2" <<'EOF' &
import socket, threading, sys
PORT, LOG = int(sys.argv[1]), sys.argv[2]
s = socket.socket(); s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
s.bind(("127.0.0.1", PORT)); s.listen(64)
def serve(c):
    try:
        buf = b""
        while b"\r\n\r\n" not in buf:
            d = c.recv(65536)
            if not d: return
            buf += d
        with open(LOG, "ab") as f:
            f.write(b"---REQ---\n" + buf.split(b"\r\n\r\n", 1)[0] + b"\n")
        c.sendall(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nOK")
    except OSError: pass
    finally: c.close()
while True:
    c, _ = s.accept(); threading.Thread(target=serve, args=(c,), daemon=True).start()
EOF
}
start_hdr "$HDR1" "$WORK/hdr1.log"
PIDS+=($!)
start_hdr "$HDR2" "$WORK/hdr2.log"
PIDS+=($!)

# 简单 200 源站(Basic 鉴权目标)
python3 - "$ORIGIN" <<'EOF' &
import socket, threading, sys
s = socket.socket(); s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
s.bind(("127.0.0.1", int(sys.argv[1]))); s.listen(64)
def serve(c):
    try:
        buf = b""
        while b"\r\n\r\n" not in buf:
            d = c.recv(65536)
            if not d: return
            buf += d
        c.sendall(b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\nauth")
    except OSError: pass
    finally: c.close()
while True:
    c, _ = s.accept(); threading.Thread(target=serve, args=(c,), daemon=True).start()
EOF
PIDS+=($!)

echo "custom-404-page-content" > "$WORK/404.html"

cat > "$WORK/server.toml" <<EOF
bind_addr = "127.0.0.1"
bind_protocol = "tcp"
bind_port = $CTL
multiplex = true
[web]
bind_addr = "127.0.0.1"
port = $API
[vhost]
http_port = $VH_HTTP
https_port = $VH_HTTPS
tcpmux_port = 0
subdomain_host = "vh.test"
custom_404_page = "$WORK/404.html"
[ports]
auto_range = "30800-30900"
[log]
level = "info"
EOF
"$SERVER" --config "$WORK/server.toml" admin create admin Vh-123-456 >/dev/null 2>&1
"$SERVER" --config "$WORK/server.toml" >"$WORK/server.log" 2>&1 &
PIDS+=($!)
sleep 2
TOKEN=$(curl -s -X POST "http://127.0.0.1:$API/api/auth/login" -H 'Content-Type: application/json' -d '{"username":"admin","password":"Vh-123-456"}' | python3 -c 'import json,sys;print(json.load(sys.stdin)["data"]["accessToken"])')
api(){ curl -s -m 15 -H "Authorization: Bearer $TOKEN" "$@"; }
CR=$(api -X POST "http://127.0.0.1:$API/api/clients" -H 'Content-Type: application/json' -d '{"name":"main"}')
CID=$(echo "$CR"|python3 -c 'import json,sys;print(json.load(sys.stdin)["data"]["id"])')
SEC=$(echo "$CR"|python3 -c 'import json,sys;print(json.load(sys.stdin)["data"]["secret"])')
cat > "$WORK/client.toml" <<EOF
server_addr = "127.0.0.1:$CTL"
protocol = "tcp"
multiplex = true
client_id = "$CID"
secret = "$SEC"
log_level = "info"
EOF
"$CLIENT" --config "$WORK/client.toml" >"$WORK/client.log" 2>&1 &
PIDS+=($!)
for i in $(seq 1 20); do
  on=$(api "http://127.0.0.1:$API/api/clients" | python3 -c 'import json,sys;print(json.load(sys.stdin)["data"][0]["online"])' 2>/dev/null)
  [ "$on" = "True" ] && break; sleep 0.5
done
[ "$on" = "True" ]; check $? "客户端上线"

mk_svc(){
  r=$(api -X POST "http://127.0.0.1:$API/api/services" -H 'Content-Type: application/json' -d "$1")
  code=$(echo "$r"|python3 -c 'import json,sys;print(json.load(sys.stdin).get("code"))' 2>/dev/null)
  [ "$code" = "0" ] || echo "MK_SVC_FAIL: $r" >&2
}
wait_rule(){
  for i in $(seq 1 12); do
    st=$(api "http://127.0.0.1:$API/api/services?client_id=$CID" | python3 -c "import json,sys;print([s['status'] for s in json.load(sys.stdin)['data'] if s['name']=='$1'][0])" 2>/dev/null)
    [ "$st" = "running" ] && return 0; sleep 0.5
  done
  return 1
}
HDRASSERT='import sys
log = open(sys.argv[1], "rb").read().decode("latin1")
last = log.rsplit("---REQ---\n", 1)[1] if "---REQ---" in log else ""
checks = sys.argv[2:]
ok_all = True
for i in range(0, len(checks), 2):
    key, want = checks[i], checks[i+1]
    low = last.lower()
    if want.startswith("!"):
        found = (key.lower() + ":") in low
        if found: print("FAIL: %s should be absent" % key); ok_all = False
    else:
        if (key.lower() + ": " + want).lower() not in low:
            print("FAIL: %s: %s not found; head=%r" % (key, want, last)); ok_all = False
sys.exit(0 if ok_all else 1)'

# ---------- C11:HTTP 头改写(Host 重写 + 请求 set/del + 响应 set) ----------
mk_svc "{\"client_id\":\"$CID\",\"name\":\"rw\",\"kind\":\"http\",\"target_host\":\"127.0.0.1\",\"target_port\":$HDR1,\"enabled\":true,\"http\":{\"custom_domains\":[\"rw.vh.test\"],\"host_rewrite\":\"up.rewritten.test\",\"request_headers\":{\"set\":[{\"name\":\"X-Rew\",\"value\":\"on\"}],\"del\":[\"X-Del\"]},\"response_headers\":{\"set\":[{\"name\":\"X-Resp-Vh\",\"value\":\"yes\"}]}}}"
wait_rule rw
h404=$(curl -s -o /dev/null -w "%{http_code}" -m 8 -H "Host: rw.vh.test" "http://127.0.0.1:$VH_HTTP/x")
body=$(curl -s -m 8 -H "Host: rw.vh.test" -H "X-Del: yes" "http://127.0.0.1:$VH_HTTP/x")
[ "$body" = "OK" ] && [ "$h404" = "200" ]; check $? "头改写:响应回传($body/$h404)"
resp=$(curl -s -D - -o /dev/null -m 8 -H "Host: rw.vh.test" "http://127.0.0.1:$VH_HTTP/x")
echo "$resp" | grep -qi "X-Resp-Vh: yes"; check $? "头改写:响应头注入生效"
python3 -c "$HDRASSERT" "$WORK/hdr1.log" Host "up.rewritten.test" X-Rew "on" X-Del "!"; check $? "头改写:上游收到 Host 重写+set+del"

# ---------- C12:Basic 访问鉴权 ----------
mk_svc "{\"client_id\":\"$CID\",\"name\":\"auth\",\"kind\":\"http\",\"target_host\":\"127.0.0.1\",\"target_port\":$ORIGIN,\"enabled\":true,\"http\":{\"custom_domains\":[\"auth.vh.test\"],\"http_user\":\"au\",\"http_password\":\"ap\"}}"
wait_rule auth
c1=$(curl -s -o /dev/null -w "%{http_code}" -m 8 -H "Host: auth.vh.test" "http://127.0.0.1:$VH_HTTP/")
c2=$(curl -s -o /dev/null -w "%{http_code}" -m 8 -u au:WRONG -H "Host: auth.vh.test" "http://127.0.0.1:$VH_HTTP/")
c3body=$(curl -s -m 8 -u au:ap -H "Host: auth.vh.test" "http://127.0.0.1:$VH_HTTP/")
[ "$c1" = "401" ] && [ "$c2" = "401" ] && [ "$c3body" = "auth" ]; check $? "Basic 鉴权:无/错凭据 401,正确凭据放行($c1/$c2/$c3body)"

# ---------- C27:自定义 404 页 ----------
mk_svc "{\"client_id\":\"$CID\",\"name\":\"web\",\"kind\":\"http\",\"target_host\":\"127.0.0.1\",\"target_port\":$HDR1,\"enabled\":true,\"http\":{\"custom_domains\":[\"app.vh.test\"]}}"
wait_rule web
n404=$(curl -s -m 8 -H "Host: unknown.vh.test" "http://127.0.0.1:$VH_HTTP/")
s404=$(curl -s -o /dev/null -w "%{http_code}" -m 8 -H "Host: unknown.vh.test" "http://127.0.0.1:$VH_HTTP/")
[ "$n404" = "custom-404-page-content" ] && [ "$s404" = "404" ]; check $? "自定义 404 页生效($s404/$n404)"
known=$(curl -s -m 8 -H "Host: app.vh.test" "http://127.0.0.1:$VH_HTTP/")
[ "$known" = "OK" ]; check $? "已知域名不受 404 配置影响"

# ---------- A4:HTTPS SNI 路由 ----------
mk_svc "{\"client_id\":\"$CID\",\"name\":\"tone\",\"kind\":\"https\",\"target_host\":\"127.0.0.1\",\"target_port\":1,\"enabled\":true,\"https\":{\"custom_domains\":[\"one.vh.test\"]},\"plugin\":{\"kind\":\"https2_http\",\"local_addr\":\"127.0.0.1:$HDR1\",\"crt_path\":\"$WORK/tls.crt\",\"key_path\":\"$WORK/tls.key\",\"host_rewrite\":\"one.up.test\"}}"
wait_rule tone || echo "tone status: $(api "http://127.0.0.1:$API/api/services?client_id=$CID" | python3 -c "import json,sys;print([(s[\"name\"],s[\"status\"],s.get(\"status_reason\")) for s in json.load(sys.stdin)[\"data\"]])" 2>/dev/null)" >&2
mk_svc "{\"client_id\":\"$CID\",\"name\":\"ttwo\",\"kind\":\"https\",\"target_host\":\"127.0.0.1\",\"target_port\":1,\"enabled\":true,\"https\":{\"custom_domains\":[\"two.vh.test\"]},\"plugin\":{\"kind\":\"https2_http\",\"local_addr\":\"127.0.0.1:$HDR2\",\"crt_path\":\"$WORK/tls.crt\",\"key_path\":\"$WORK/tls.key\",\"host_rewrite\":\"two.up.test\"}}"
wait_rule ttwo || echo "ttwo status: $(api "http://127.0.0.1:$API/api/services?client_id=$CID" | python3 -c "import json,sys;print([(s[\"name\"],s[\"status\"],s.get(\"status_reason\")) for s in json.load(sys.stdin)[\"data\"]])" 2>/dev/null)" >&2
curl -sk -m 8 --resolve "one.vh.test:$VH_HTTPS:127.0.0.1" "https://one.vh.test:$VH_HTTPS/a" > "$WORK/b1.out" 2>"$WORK/b1.err"; echo "curl1 rc=$?" >&2
curl -sk -m 8 --resolve "two.vh.test:$VH_HTTPS:127.0.0.1" "https://two.vh.test:$VH_HTTPS/b" > "$WORK/b2.out" 2>"$WORK/b2.err"; echo "curl2 rc=$?" >&2
b1=$(cat "$WORK/b1.out"); b2=$(cat "$WORK/b2.out")
grep -o "error[^<]*" "$WORK/b1.err" "$WORK/b2.err" >&2 || true
[ "$b1" = "OK" ] && [ "$b2" = "OK" ]; check $? "HTTPS SNI:双域名分流响应回传($b1/$b2)"
python3 -c "$HDRASSERT" "$WORK/hdr1.log" Host "one.up.test"
r1=$?
python3 -c "$HDRASSERT" "$WORK/hdr2.log" Host "two.up.test"
r2=$?
[ $r1 = 0 ] && [ $r2 = 0 ]; check $? "HTTPS SNI:各 SNI 到达各自上游(one→hdr1,two→hdr2)"
neg=$(curl -sk -o /dev/null -w "%{http_code}" -m 8 --resolve "nope.vh.test:$VH_HTTPS:127.0.0.1" "https://nope.vh.test:$VH_HTTPS/" 2>/dev/null)
# 未知 SNI:服务端 peek 后无路由直接关闭,curl 得不到 200(通常 000 连接层失败)
[ "$neg" != "200" ]; check $? "HTTPS SNI:未知域名被拒(无 200,code=$neg)"

echo
echo "== 结果: PASS=$PASS FAIL=$FAIL =="
[ "$FAIL" = "0" ]
