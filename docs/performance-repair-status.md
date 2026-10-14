# 性能修复状态(2026-10-14,kcp-boundary 七轮收官 + TCP 监听生命周期后)

**结论:原始性能目标未完成,不宣称绝对无缺陷。**本轮完成主代理 [kcp-main-review](../benchmark/runs/goal-repair/kcp-main-review/README.md) 六项确定性失败的全部修复,及其后六批追加复核(shutdown 交付、listener 释放、未交付会话、demux 自锁、TCP 未交付 mux、punch 父取消)与 TCP 监听生命周期并行修复;最终源码全工作区测试/clippy exit=0。但四套 E2E 与三机性能矩阵**未在当前源码重跑**,G1/G2/G6 以旧构建数据一律未达标;本轮定向通过 ≠ 全矩阵验收。上一版 56 行已存档 [performance-repair-history-3.md](performance-repair-history-3.md)(SHA256 `aebfa707…`,本轮 cp 后 shasum 核验);1059 行全文在 history.md、44 行简报在 history-2.md。

## 0. 当前源码与证据对应(先读)

- 本会话复核:`crates/rh-transport/src/kcp.rs` SHA256 `16b721555dc53afdaea177cfef7c2ca4cc2ee1430f92099972bd97a0a7b380ec`(4221 行)= kcp-boundary 第七轮最终稿;`crates/rh-transport/src/tcp.rs` `575d9a1f…` = TCP 监听生命周期最终稿。
- **上轮 kcp-improvement 的 75 文件 manifest(kcp.rs `bd3a8b9c…`)成为历史**:其后 kcp.rs 经七轮、tcp.rs 经两轮补修,其三机验收/identity 证据不再代表当前源码;final-linux(-r2) 的 74 文件清单更旧。当前源码尚无对应的远端构建与验收 manifest。
- 旧 136(本地)与 148(Linux)全绿**不覆盖**主代理六项——六项正是在 148 绿的 `bd3a8b9c` 上确定性复现(red.log 0 passed/6 failed,exit=101)。不得再以旧全绿抵扣六项覆盖。
- 无 git 提交:HEAD 仍 `c3ca4bd`(本会话 `git log` 核实)。

## 1. 主代理六项确定性失败逐项(基线 red.log exit=101;修复全程见 [kcp-boundary/README.md](../benchmark/runs/goal-repair/kcp-boundary/README.md))

| # | 失败(原判定) | 状态 | 修复与默认回归 | 当前源验证 |
|---|---|---|---|---|
| 1 | 超大写永远 Pending | 已修 | 第一轮 partial write 前缀接受;`main_review_tests::oversized_write_must_make_partial_progress` | 最终源全量含此测试通过(§3) |
| 2 | 空读缓冲阻塞 | 已修 | 第一轮 `remaining()==0` 即 Ready;`…empty_readbuf_on_idle_stream_must_complete` | 同上 |
| 3 | 回插超字节容量 | 已修 | 第一轮在途记账+第三轮 S1 总预算含在途;`…failed_send_reinsertion_must_respect_byte_cap`+`complete_backlog_budget_counts_inflight_packet` | 同上 |
| 4 | 在途包被新包超前 | 已修 | 第一轮串行 drain 所有权+第三轮 S1;`…draining_datagram_must_not_be_overtaken` | 同上 |
| 5 | idle 发送任务持 socket 不退 | 已修 | 第一轮 Drop notify+退避兜底;`…idle_backlog_worker_must_release_after_producer_drop` | 同上 |
| 6 | 未 ACK 绕过背压 | 已修 | 第一轮 `wait_snd≤512` 站立限额+第三轮 E1 精确上界 864,256B;`…no_ack_peer_must_backpressure_total_pending_writes`+`e1_send_path_total_bound_is_exact` | 同上 |

- 红→绿:首轮独立复现 13 failed/18 passed(exit=101)→ 修复后绿;主代理六项**原样探针**独立复跑绿最近一次在第二轮最终稿 `5d448955`(boundary-original-six-green.log:6 passed/0 failed;该日志未附显式 exit 行)。**未验部分如实:主代理未在最终 `16b72155` 上重录六项原样探针**;六项以工作区默认回归身份在最终源全量测试通过(本会话解析 stdout 含全部六名+pa6/pa7)。

## 2. 追加复核项(主代理逐批点名,全部落为默认回归;细节见 kcp-boundary README 各轮)

- **shutdown 丢弃已接受数据**(SHUTDOWN-ADDENDUM):第二轮真契约(排空+`wait_snd()==0` 才 Ready);红 exit=101 → b09 真实对端 256KiB 逐字节绿。
- **KCP listener drop 后仍占端口**:第二轮 `DemuxCtl` closing/会话表收敛;b10 红 AddrInUse → 绿。
- **未 accept 会话滞留**(KCP-UNACCEPTED-REVIEW):第四轮 accept 时才 wrap mux、ready 释放即整体取消;红 exit=101(unaccepted-red.log)→ 绿;主代理独立复绿 kcp-unaccepted-independent-green.log。
- **demux 满路径 SessionMap 自锁**(DEMUX-FULL-DEADLOCK r3):第五轮锁内只查表/注册、锁外 spawn/投递;红 exit=71(WATCHDOG)→ 绿;主代理独立复绿 demux-full-independent-green(-r2).log。
- **TCP listener drop 泄漏 + 未交付 mux 滞留**(TCP-QUEUED-REVIEW,并行任务,tcp.rs 唯一改动源):句柄 Drop 关停 accept 泵、accept 交付才构造 mux;红 0/3、4/2(exit=101)→ 6/6 ×3(exit=0);终态 `575d9a1f…`,主代理探针 queued_tcp_ready 修复后 exit=0([tcp-listener-lifecycle/RUNS.md](../benchmark/runs/goal-repair/tcp-listener-lifecycle/RUNS.md))。端口观察任务曾记"该项修复仍待办",以实际源码与终态日志为准:已修。
- **punch 父取消轮询任务永生**(PUNCH-PARENT-CANCEL,最后阻断):第七轮 `PunchCancel` RAII 守卫(kcp.rs:1433,Drop 对双 abort 通道 notify_one;成功路径显式解除武装)。红 parent-cancel-red.log(快照 `0d43257e`+pa6)0/1 exit=101 `socket_refs=2`;主代理探针同快照 check-r2.log `socket_refs=2 released=false EXIT=1` → 当前源 final-green.log `socket_refs=1 released=true EXIT=0`(11:08,前后同探针对照)。pa6/pa7 入默认回归;复核方定向复现成功路径 `strong_count==3` EXIT=0。
- **保留未修(审计自身定性)**:F5(poll_shutdown 成功终止读侧,生产不可达)/F6(idle 被未消费数据推迟)/F9(PunchOutput WouldBlock 假成功)——理由与位置见 README 第六轮;F2/F3/F4/F7/F8 修复未重动(复核抽查行号一致)。

## 3. 本轮验证(命令与真实退出码;「最终源」= kcp.rs 11:00 落盘之后)

- 全工作区:`cargo test --workspace --locked --offline` **exit=0,177 passed/0 failed/2 ignored**(checks/kcp-boundary-workspace-test-77c8lvoo,11:03>11:00;本会话逐行累加 7+15+11+12+82+4+10+1+6+1+28,含六项+pa6/pa7+b10)。
- clippy:`cargo clippy --workspace --all-targets --locked --offline` **exit=0**(checks/kcp-boundary-clippy-uo6mpehh,11:03)。
- 定向:kcp 模块 **52/52**、rh-transport lib **82/82**、集成 goal_repair_mux+ws_mux_repro **10+1** 全 exit=0(kcp-boundary/green-*.log);红 parent-cancel-red.log exit=101。
- loopback 诊断 bench(32MiB×3):单流中位 **355.4**、4 流 **354.7** Mbps(loopback-bench-after.log;诊断性,不替代三机验收)。
- 红线在位:quinn 65536(`quic.rs:24`)、64KiB 转发缓冲、未设 carrier socket 缓冲、ws.rs 状态机未动(`c3b67a1e…`)、yamux open_rx Ready(None) 语义未动(`4a9a219a…`)。

## 4. E2E 与 Linux 现状(如实,不沿用旧通过)

- 四套件 E2E(e2e.sh 73/0、resilience 19/0、transports 12/0、relay-modes 21/0,exit 全 0)最近一次全绿在 final-linux-r2,**属改动前旧构建**(server `26c4704d`/client `087bcb8c`)。**当前源码(kcp.rs/tcp.rs 改动后)未重跑四套件**——正确性现状以 §3 全量测试为准,E2E 级验收待跑。
- Linux 三机:性能候选未过门槛,**本轮未执行新的远端测试**;若历史验收器重跑结果与旧输出不一致,先核对 source/版本差异,不得混构建归因。
- 「早期 WS 缺锁」「未跑 Linux」「G6=24」均为已被后续证据取代的旧状态(WS 不变量锁已落、final-linux-r2 已跑、round-2 实为 25/36 档 FAIL),不再列为现状。

## 5. 性能目标 G1–G7(验收门槛 [performance-repair-acceptance.md](performance-repair-acceptance.md) §1)

- **G1 TCP nomux 单流双向 ≥3000:FAIL**(CPU 口径 <50% 达标)。round-2 旧构建 1943/2113;final-lowat 生产诊断三档+默认中位 1845–2302,全部 <3000,CPU 最高 36.9%;LOWAT 档位作为 G1 手段已证伪。**当前源码未重测**。
- **G2 TLS mux 单流双向 ≥2500:FAIL**。round-2 1172/1752;当前源码未重测。
- **G3/G4/G5:曾在 kcp-improve 构建 PASS**(六协议正吞吐;1200B UDP 0.0%×3;4096 隧道 alive 5 轮/setup 9.76s),但该构建旧于当前源七轮补修,**不构成当前源的同一码验收**,需随全矩阵重跑。
- **G6 配对保留率 ≥80%:FAIL**。round-2(旧构建)25/36 档 <80%;KCP kcp-improve 实测 12.1–14.8%(OLD 4.2–6.2%);tcp/tls/ws/wss/quic 在最新源未重测。
- **G7(≥3 次配对中位):不能报全过**。kcp-improvement 对照 OLD 两档不足 3 有效配对(4 个 exit=1 漏判,RAW-ERRATA 已勘误)。
- **当前源码完整六协议矩阵(逐协议×方向×流数)未跑**;本轮全部通过证据均为定向/单元级,不得泛化为全矩阵。

## 6. KCP 遗留与观察(下一轮候选,均未关闭)

- **ACK 合批:未实施**。方案(batching-plan.md,基线=当前 `16b72155`)审查 3 项阻断:①tokio 1.53.2 `recv_many` 为 append 语义,方案必须写明每轮 `batch.clear()` 否则 Vec 无界增长;②通道已关闭且空时 `recv_many` 立即返回 0,必须映射 `if n==0 break` 否则热转到 90s 空闲超时;③b12 单包 1 flush 现行为已成立、不可能红,须改列冻结守护或删红绿措辞。另 2 项注记(b11 喂纯 ACK 包、门槛(e)低负载盲区)。按审查修订方案后方可实施。
- **NEW rh-server 隧道窗口 ~156–169% 单核口径进程 CPU**(OLD 31–53%):多线程进程占用,**未证明单线程饱和或整机饱和**;疑似每包一 ACK 数据报放大,归因待合批实验的计数器证据。
- ghost-20004 已排除:是压测装置自身把 vhost `http_port` 写为 20004(peer.sh:187),非 trial_bind/DB 不同步;auto_range 分配已实测跳过自占端口(ghost-20004-verify/ repro+skip-proof)。遗留低严重性:2007 报错不区分"被自身 vhost/web/metrics 占用"与"被外部进程占用",创建路径无端口预检(api.rs:842),非阻断。
- 服务端 demux try_send 满丢包(靠 kcp 重传)、RECORD.md 延迟表值与引用日志不一致(方向性结论不变)——见 history-3 §3,未重动。

## 7. 需用户决定(未授权不执行)

1. sysctl 解锁实验(rmem_max/wmem_max 等)——环境改动与恢复范围。
2. 网卡/拓扑变更(如 vmxnet3 USO)——平台级决策;软件侧仍有可证伪实验(合批、falsifiable 归因)未做完,不以"无实验剩余"为由跳过。
3. **当前源码完整验收重跑**:四套件 E2E + 六协议性能矩阵(G1/G2/G6 及 G3–G5/G7 同源落地)——现有授权未覆盖。
4. KCP 合批实验是否按修订方案开轮(含 b12 分类修正)。
5. 红线内可继续:tls 单流差额归因(perf 级采样需授权)、KCP 80% 保留率路径(合批+CPU 归因)。

## 证据索引

- 六项+追加复核:`benchmark/runs/goal-repair/{kcp-main-review,kcp-boundary}/`(红/绿日志、快照、README 七轮)。
- TCP:`benchmark/runs/goal-repair/tcp-listener-lifecycle/`(RUNS.md 三组红绿+终态 575d9a1f)。
- 最终源全量/clippy:`benchmark/runs/goal-repair/checks/kcp-boundary-{workspace-test-77c8lvoo,clippy-uo6mpehh}/`。
- 端口观察:`benchmark/runs/goal-repair/ghost-20004-verify/`;性能旧数据:round-2/final-lowat/kcp-improvement(含 RAW-ERRATA)。
- 历史:history.md(1059 行)、history-2.md(44 行)、history-3.md(56 行,`aebfa707…`)。
