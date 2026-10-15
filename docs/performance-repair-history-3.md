# 性能修复状态(2026-10-14，主代理复核补充中)

> **最新主代理复核撤回下文“正确性全部通过”的结论。** 全套136项本地测试虽绿，但独立故障注入新复现KCP六项边界失败、成功shutdown后丢弃已接受数据，以及TCP/KCP监听句柄释放后仍占端口。上一轮KCP性能对照另有4个exit=1样本被汇总漏判，OLD两档不足3个有效配对，G7不能报全过。证据见 [KCP主代理复核](../benchmark/runs/goal-repair/kcp-main-review/README.md)、[关闭与监听补充](../benchmark/runs/goal-repair/kcp-main-review/SHUTDOWN-ADDENDUM.md)、[原始样本勘误](../benchmark/runs/goal-repair/kcp-main-review/RAW-ERRATA.md)。GLM-5.3工作流正在补修；下文是上一轮交付状态，不代表本次复核已放行。G1/G2/G6目标仍未达成。

**结论:原始性能目标未完成——最新正确性证据(Linux workspace 133 测试/E2E 四套件/主代理六项复现/KCP 定向验收 G4·G5)全部通过,但 G1/G2/G6 性能门槛在全部已测构建上均未达标,不得宣称无缺陷或达标。**本文 ≤180 行;此前 1059 行长篇存于 [performance-repair-history.md](performance-repair-history.md)(`96ec9574…`),上一版 44 行简报存于 [performance-repair-history-2.md](performance-repair-history-2.md)(`84bad920…`,本轮 cmp 逐字节核验)。

## 0. 版本覆盖(先读:哪份证据对应哪份源码)

- 本地生产源码已含 KCP 改动:`crates/rh-transport/src/kcp.rs`(`bd3a8b9c…`)、`crates/rh-transport/Cargo.toml`(`ea5be287…`)与 `benchmark/runs/goal-repair/kcp-improvement/remote/build/sources.sha256`(75 文件)逐条一致,另新增 `examples/kcp_loopback_bench.rs`(测试工具,非生产)。
- 因此 **final-linux 的 74 文件清单不再等于最新源码**:final-linux / final-linux-r2 / 主代理六项复现 / final-lowat 的证据属于改动前构建(rh-server `26c4704d…` / rh-client `087bcb8c…`);KCP 三机验收属于 kcp-improve 构建(server `d5930cfc…` / client `bcf113eb…`,workspace 测试 148/0 exit=0)。
- 旧构建全绿不得当作新版本(含 KCP 改动)的全协议验收;两份构建的正确性证据都在,但覆盖面不同(见 §2)。
- 无 git 提交:HEAD 仍 `c3ca4bd`(本会话 `git log` 核实)。

## 1. 已修复(最终源码;无 git 提交)

- **vendor yamux 首次关闭丢 SYN(close-before-first-write)**:close_stream 携带 syn 位(SYN|FIN 标准合帧,syn=false 时字节与旧版一致);poll_close/poll_ready 容量确认后才消费 SYN;SendClosed 幂等。主代理修前复现 exit=101,修后 3 次 exit=0。
- **ws.rs R5 不变量锁**:`crates/rh-transport/src/ws.rs` `#[cfg(test)]` 新增 4 测试(send_queued 背压不重不丢、单帧判读、from_partially_read 同批首帧保留含跨批拆分);生产状态机零改动,突变取证 `benchmark/runs/goal-repair/final-gaps/logs/ws-mutation-red.log`。
- **copy/splice 错误路径有界收敛**:`crates/rh-server/src/data.rs`(CANCEL_CLOSE_GRACE=1s 有界;先完成侧 panic/错误立即终止对向;clean EOF 半关闭保持无界红线)、`crates/rh-client/src/forward.rs`(ERROR_CLOSE_GRACE=1s)、`crates/rh-transport/src/splice.rs`(join_relay_pumps:单泵 panic 立即 abort 对向;splice 模块为 Linux-only,其可移植核心已由 darwin 测试覆盖)。
- **KCP 生产改动三件**(本轮新增,审查已批准):A 事件驱动 flush——poll_write 入队后 notify_one,user_wrote/got_packet/wait_snd()>0 时立即 kcp.flush()(kcp 0.6.0 interval 钳 ≥10ms,原 2ms tick 注释前提不成立);B UdpOutput WouldBlock 不再假成功丢包——有界 UdpBacklog(128 条/64KiB)+drain 任务,满/真实错误计数,短写报 Err,未 setsockopt;C poll_read as_slices 两段直拷+写队列 256KiB 上限+scratch 复用。修前红取证 `benchmark/runs/goal-repair/kcp-improvement/before-fail-verification.txt`(t01–t04 FAILED)。
- **叙述勘误(ERRATA)**:`benchmark/runs/goal-repair/final-gaps/ERRATA.md`——round-2 G6 实为 25/36 档 FAIL(ROUND2-REPORT 误写 24);两轮对比三处口径混用;refrelay splice 管道实际 64KiB。历史 raw 未改动。
- 更早轮次的 mux 父取消/送达扣费/64KiB cap/quota 前缀等缺陷与逐处代码位置见存档;主代理逐处独立复核在 `/tmp/rh-main-independent-review/completion-audit.md`。

## 2. 已验证(全部真实退出码;「本会话」=本轮亲自复核,其余引自留档日志)

- **final-linux**(改动前构建):`cargo test --workspace` exit=0,**133 passed/0 failed**(本会话逐行累加 logs/test-workspace.log 14 条 test result:7+15+11+12+44+4+10+1+1+28+0×4);`cargo build --release` BUILD_EXIT=0。
- **final-linux-r2**(同一构建,源码 hash 与 final-linux 0 差异):四套件 Linux 全绿——e2e.sh **73/0**、resilience **19/0**、transports **12/0**、relay-modes **21/0**,exit 全 0(REPORT.md:19-22,本会话核对)。此前「未跑 Linux」的自述已被此日志覆盖,不再列为未做。
- **主代理六项边界回归各 3 次**:`/tmp/rh-main-independent-review/final-six-regressions.json` 18 条(double-charge/abort-parent/cap/cancel-close/cancel-after-eof/close-before-write),本会话解析 **exits 全 0**。
- **kcp-improve 构建**:workspace 测试 **148/0 exit=0**、release exit=0、75 文件 manifest 哈希一致、四份 identity.jsonl proc_exe 逐条匹配(summary.json)。
- **KCP 定向三机验收**(`benchmark/runs/goal-repair/kcp-improvement/REPORT.md`,OLD=final-linux vs NEW=kcp-improve,iperf 10s×3 配对中位):吞吐单流 176.3→**366.6**(上行 ×2.08)/127.3→**397.4** Mbps(下行 ×3.12),16 流 346.2/374.8;**G4 PASS**(1200B UDP 三样本 0.0% loss,首跑装置错误标 INVALID 保留);**G5 PASS**(established=initial_verified=hold_alive=4096、failed=0、alive 5 轮、setup 9.76s;首跑规则指错后端标 INVALID 保留,64 连接 sanity 后重跑)。
- **final-lowat**(改动前生产二进制,TCP nomux splice,n=3 中位):每档 metrics `relay_path_total{path="splice"}=13` 真实命中;未设任何 carrier SO_RCVBUF/SO_SNDBUF、未改 sysctl/源码;CPU 单核口径覆盖率 0.98。
- **红线**:redline_scan.py code 桶 0 命中(2 条为解释性注释;字面量并入 comment 桶的局限已知);quinn 并发流上限 65536(`quic.rs:24`)、64KiB 转发缓冲(单元常量锁)、ws.rs 状态机未动、yamux open_rx Ready(None) 不终止已有流——均在位。三机收尾 CLEAN(进程 PID+/proc/exe 身份核验、端口释放、证据脱敏 0 残留)。

## 3. 仍未达(性能;如实,不将局部诊断泛化到未测协议/方向)

- **G1 TCP nomux ≥3000 Mbps:FAIL**。round-2(旧构建)单流 1943/2113(`round-2/verify-goal-report.txt:13-17`,CPU 全过);最终生产 LOWAT 诊断(`final-lowat/REPORT.md`)三档 {65536,262144,1048576}+默认轮 tunnel 中位 1845–2302,全部 <3000,CPU 全档 <50%(最高 36.9%);**LOWAT 档位作为 G1 手段证伪**(绝对中位均在两跳 ~2G 波动带内,1048576 轮 81.2% 保留率伴随同轮 direct 偏低属比值效应;65536 档在生产 rh 上为最低轮之一,不改默认凑结论)。
- **G2 TLS mux ≥2500 Mbps:FAIL**。round-2 单流 1172/1752(`verify-goal-report.txt:20-22`);最终构建未重测。
- **G6 配对保留率 ≥80%:FAIL**。round-2(旧构建)25/36 档 <80%;KCP 新构建实测 12.1–14.8%(OLD 4.2–6.2%,改善 +7–9pp,仍差 65–68pp);tcp/tls/ws/wss/quic 在最终构建未重测。
- **最终源码(含 KCP 改动)的完整六协议矩阵未跑**:G1/G2/G6 结论分别沿用 round-2(旧构建)与 final-lowat/kcp-improve(各自局部),不是同一份构建的全量结论;G3/G4/G5 的通过证据分属两个构建,KCP 部分已在最新构建复测、其余协议未在。
- **KCP 遗留**(审查批准意见如实保留):立即 flush 使 ACK 数据报约 1:1 放大(每包一 ACK 数据报,反向 pps/系统调用翻倍,4096 隧道 CPU/丢包风险,loopback 测不出);NEW rh-server 隧道窗口 CPU 升至约 156–169% 单核(OLD 31–53%,16 流下行实测 server 155.8–161.1%,`REPORT.md:41`)——以 CPU 换吞吐,单核饱和是逼近 80% 保留率的下一瓶颈;RECORD.md 延迟表值(42/84µs)与其引用日志(71/115µs)不一致,方向性结论不变但表值无日志支撑;服务端 demux try_send 满丢包(靠重传)、punch 路径 WouldBlock 假成功与 try_send-break、2ms 固定 tick 未改;积压满丢包含窗口探测/ACK 的取舍需在三机指标观察 dropped_full。
- **观察待查**:rh-server 运行时 20004 幽灵监听(DB 无行、新建规则报 2007 绑定失败),疑 trial_bind_tcp(`api.rs:838`)与 DB 状态不同步,未定位(`kcp-improvement/REPORT.md` §9)。
- **实测局限(只陈述测到的事实)**:同拓扑参照 relay 与生产 rh 两跳均落 ~2.0–2.2Gbps 带,是当前实测对照,不构成软件上限证明;NIC 硬件 USO off 不等于内核软件 GSO 不可用;>100% 单核口径进程 CPU 是多线程占用,不等于整机饱和;tls 上/下行差额、quic 高 CPU 低吞吐、kcp ACK 风暴均无 perf 级热点定位。

## 4. 需用户决定(未授权不执行)

1. **sysctl 解锁实验**(rmem_max/wmem_max 等):wmem_max=212992 使更大 iperf3 窗口被 clamp,而显式扩 socket 缓冲被红线禁止;解锁需授权环境改动与恢复范围。
2. **网卡/拓扑变更**(如 vmxnet3 USO fixed 换网卡):平台级决策,无软件侧可证伪实验剩余。
3. **最终源码(含 KCP)完整六协议性能矩阵是否重跑**:把 G1/G2/G6 及 G3/G5 证据落到同一份最新构建;当前授权仅覆盖正确性 e2e、TCP LOWAT 诊断与 KCP 定向验收。
4. **KCP 后续投入取舍**:ACK 合批与及时 flush 的吞吐/CPU 权衡、每会话有界 demux 队列、kcp.check() 动态调度、demux/punch 遗留缺陷——是否另开一轮。
5. 红线内仍可探但尚无达标证据的方向:tls 单流差额归因(需授权 perf 级采样)、LOWAT 对 mux/copy 路径对照(仅 TCP nomux 已证伪)。

## 证据索引

- 正确性:`benchmark/runs/goal-repair/{final-gaps,final-linux,final-linux-r2}/`;KCP:`kcp-improvement/`(RECORD/REPORT/summary/remote 257 文件)。
- 性能:`benchmark/runs/goal-repair/{round-1,round-2,diagnosis-after-round-2,targeted-before-round-3,final-lowat,kcp-improvement}/`。
- 主代理独立证据:`/tmp/rh-main-independent-review/`(completion-audit.md 结论同为「目标未达成、不宣称绝对无缺陷」;final-six-regressions.json 18/18 exit=0)。
- 历史:1059 行全文 [performance-repair-history.md](performance-repair-history.md);上一版简报 [performance-repair-history-2.md](performance-repair-history-2.md)。
