#!/usr/bin/env bash
# linktest.sh —— diagnosis-after-round-2 链路基线探针(client 侧运行)。
# 用法: linktest.sh <server_ip> <server_port> <outdir> <label> [duration]
# 单链路:upload(client 发送)×3 + download(-R,server 发送)×3,各 10s,
# 同一 /tmp/rh-bin/iperf3(3.17.1),默认 autotune(不动任何 sysctl/ethtool)。
# 串行执行(脚本内部不并发);raw JSON/stderr/退出码 全留痕。
set -u
IPERF=/tmp/rh-bin/iperf3
SRV="$1"; PORT="$2"; OUT="$3"; LABEL="$4"; DUR="${5:-10}"
mkdir -p "$OUT"
for i in 1 2 3; do
  t0=$(date +%s%N)
  timeout 60 "$IPERF" -c "$SRV" -p "$PORT" -t "$DUR" -J \
    > "$OUT/$LABEL-up-s$i.json" 2> "$OUT/$LABEL-up-s$i.stderr"
  echo "$?" > "$OUT/$LABEL-up-s$i.exit"
  t1=$(date +%s%N)
  echo "$t0 $t1" > "$OUT/$LABEL-up-s$i.window"
  m=$(jq -r '.end.sum_received.bits_per_second/1e6|floor' "$OUT/$LABEL-up-s$i.json" 2>/dev/null)
  echo "$LABEL upload s$i: ${m:-INVALID} Mbps (client=$(hostname) 发送)"
  timeout 60 "$IPERF" -c "$SRV" -p "$PORT" -t "$DUR" -R -J \
    > "$OUT/$LABEL-down-s$i.json" 2> "$OUT/$LABEL-down-s$i.stderr"
  echo "$?" > "$OUT/$LABEL-down-s$i.exit"
  t2=$(date +%s%N); t3=$(date +%s%N)
  echo "$t2 $t3" > "$OUT/$LABEL-down-s$i.window"
  m=$(jq -r '.end.sum_received.bits_per_second/1e6|floor' "$OUT/$LABEL-down-s$i.json" 2>/dev/null)
  echo "$LABEL download s$i: ${m:-INVALID} Mbps (-R:server=$SRV 发送)"
done
