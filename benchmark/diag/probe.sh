#!/usr/bin/env bash
# probe.sh —— diagnosis-after-round-2 同窗采样探针(在被测进程所在机运行)。
# 用法: probe.sh <pid> <out_prefix> <duration_s> [ss_filter]
# 每秒采:
#   * ss -tinmp(按 ss_filter 过滤;不给则采全部 established)
#   * /proc/<pid>/task/*/stat 的 pid/comm/utime/stime(第 1/2/14/15 字段)
#   * /proc/softnet_stat、/proc/stat 的 cpu/steal/softirq 行(全局,差分用)
# pidstat/perf 在三机缺失(已核实),线程级 CPU 以 /proc 自采替代;
# 采样循环有 deadline(duration+5s 硬限)。
set -u
PID="$1"; OUT="$2"; DUR="$3"; FILTER="${4:-}"
mkdir -p "$(dirname "$OUT")"
: > "$OUT.ss"; : > "$OUT.threads"; : > "$OUT.proc"
end=$(( $(date +%s) + DUR + 5 ))
while [ "$(date +%s)" -lt "$end" ]; do
  ts=$(date +%s.%N)
  if [ -n "$FILTER" ]; then
    ss -tinmp "$FILTER" >> "$OUT.ss" 2>/dev/null
  else
    ss -tinmp state established >> "$OUT.ss" 2>/dev/null
  fi
  echo "@$ts" >> "$OUT.threads"
  for t in /proc/$PID/task/*/stat; do
    [ -r "$t" ] && awk '{print $1,$2,$14,$15}' "$t" >> "$OUT.threads"
  done
  { echo "@$ts"; grep -E "^cpu |^softirq|^steal" /proc/stat | head -3; cat /proc/softnet_stat 2>/dev/null | head -2; } >> "$OUT.proc"
  sleep 1
done
echo "probe done: $OUT.*"
