#!/usr/bin/env bash
# pairtest16.sh —— KCP 交替配对探针(支持流数参数,t3 运行)。
# 用法: pairtest16.sh <direct_ip> <direct_port> <tunnel_ip> <tunnel_port> <outdir> <label> [streams]
set -u
IPERF=/tmp/rh-bin/iperf3
DIP="$1"; DPORT="$2"; TIP="$3"; TPORT="$4"; OUT="$5"; LABEL="$6"; P="${7:-1}"
mkdir -p "$OUT"
for i in 1 2 3; do
  for spec in "up:" "down:-R"; do
    tag="${spec%%:*}"; rev="${spec#*:}"
    for pair in "direct:$DIP:$DPORT" "tunnel:$TIP:$TPORT"; do
      path="${pair%%:*}"; rest="${pair#*:}"; host="${rest%%:*}"; port="${rest#*:}"
      t0=$(date +%s%N)
      timeout 90 "$IPERF" -c "$host" -p "$port" -t 10 -P "$P" $rev -J \
        > "$OUT/$LABEL-$path-$tag-s$i.json" 2> "$OUT/$LABEL-$path-$tag-s$i.stderr"
      echo "$?" > "$OUT/$LABEL-$path-$tag-s$i.exit"
      t1=$(date +%s%N); echo "$t0 $t1" > "$OUT/$LABEL-$path-$tag-s$i.window"
      m=$(jq -r '.end.sum_received.bits_per_second/1e6|floor' "$OUT/$LABEL-$path-$tag-s$i.json" 2>/dev/null)
      echo "$LABEL ${P}流 $path $tag s$i: ${m:-INVALID} Mbps"
    done
  done
done
