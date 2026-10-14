#!/usr/bin/env bash
# pairtest.sh —— final-lowat 交替配对探针(t3 运行)。
# 用法: pairtest.sh <direct_ip> <direct_port> <tunnel_ip> <tunnel_port> <outdir> <label>
# 每 sample 先 direct 后 tunnel(同 spec 紧邻成对,交替不并发),up/down ×3。
set -u
IPERF=/tmp/rh-bin/iperf3
DIP="$1"; DPORT="$2"; TIP="$3"; TPORT="$4"; OUT="$5"; LABEL="$6"
mkdir -p "$OUT"
for i in 1 2 3; do
  for spec in "up:" "down:-R"; do
    tag="${spec%%:*}"; rev="${spec#*:}"
    for pair in "direct:$DIP:$DPORT" "tunnel:$TIP:$TPORT"; do
      path="${pair%%:*}"; rest="${pair#*:}"; host="${rest%%:*}"; port="${rest#*:}"
      t0=$(date +%s%N)
      timeout 60 "$IPERF" -c "$host" -p "$port" -t 10 $rev -J \
        > "$OUT/$LABEL-$path-$tag-s$i.json" 2> "$OUT/$LABEL-$path-$tag-s$i.stderr"
      echo "$?" > "$OUT/$LABEL-$path-$tag-s$i.exit"
      t1=$(date +%s%N); echo "$t0 $t1" > "$OUT/$LABEL-$path-$tag-s$i.window"
      m=$(jq -r '.end.sum_received.bits_per_second/1e6|floor' "$OUT/$LABEL-$path-$tag-s$i.json" 2>/dev/null)
      echo "$LABEL $path $tag s$i: ${m:-INVALID} Mbps"
    done
  done
done
