# 性能修复状态(简明最终版,2026-10-14)

**结论:原始目标未完成——最新正确性证据(Linux 全套测试/E2E/主代理六项复现)全部通过,但性能 G1/G2/G6 未达标。**本文件取代此前 1059 行累积报告;旧文已逐字存档于 [performance-repair-history.md](performance-repair-history.md)(sha256 `96ec9574…`,cp 后 cmp 逐字节一致,本会话核验)。历史细节一律查存档,本文只保留现状与证据指针。

## 一、已修复(最终源码;无 git 提交)

生产源码 74 文件与 `benchmark/runs/goal-repair/final-linux/sources.sha256` 逐条 SHA256 0 差异(最终构建即此源码);git HEAD 仍为 `c3ca4bd`(本会话核实,无新提交)。

- **vendor yamux 首次关闭丢 SYN(close-before-first-write)**:`vendor/yamux/src/frame.rs` close_stream 增加 syn 位(SYN|FIN 标准合帧,syn=false 时字节与旧版一致);poll_close/poll_close_half 在 poll_ready 确认容量后才消费 SYN;SendClosed 幂等。修前主代理复现 exit=101,修后 6/6 exit=0。
- **ws.rs R5 不变量锁**:`crates/rh-transport/src/ws.rs:284` `#[cfg(test)]` 新增 4 测试(send_queued 背压不重不丢、单帧判读、from_partially_read 同批首帧保留含跨批拆分);生产状态机零改动,突变验证见 `benchmark/runs/goal-repair/final-gaps/logs/ws-mutation-red.log`(破坏守卫→2 红)。
- **copy 通道错误路径有界收敛**:`crates/rh-server/src/data.rs`(CANCEL_CLOSE_GRACE 有界;先完成侧 panic/错误立即终止对向;clean EOF 半关闭保持无界红线)、`crates/rh-client/src/forward.rs`(ERROR_CLOSE_GRACE=1s)、`crates/rh-transport/src/splice.rs`(join_relay_pumps:单泵 panic 立即 abort 对向)。
- **叙述勘误(ERRATA)**:`benchmark/runs/goal-repair/final-gaps/ERRATA.md`——round-2 G6 实为 25/36 档 FAIL(ROUND2-REPORT 误写 24);两轮对比三处口径混用;refrelay splice 管道实际容量 64KiB(未设 F_SETPIPE_SZ),非 256KiB。历史原始日志未改动。
- 此前轮次已修复的 mux 父取消/送达扣费/64KiB cap/取消时序/非 mux prefix 等缺陷清单与逐处代码位置见存档历史;主代理逐处独立复核记载于 `/tmp/rh-main-independent-review/completion-audit.md`。

## 二、已验证(全部真实退出码;标注「本会话」者为本轮亲自复核,其余引自留档日志)

- **final-linux**(`benchmark/runs/goal-repair/final-linux/`):`cargo test --workspace` exit=0,**133 passed / 0 failed**(本会话逐行累加 logs/test-workspace.log:7+15+11+12+44+4+10+1+1+28+0×4;另有 2 个声明中的 `#[ignore]` 诊断);`cargo build --release` exit=0(logs/build-release.exit=BUILD_EXIT=0)。
- **final-linux-r2**(`benchmark/runs/goal-repair/final-linux-r2/REPORT.md`):四套件 Linux 全绿——e2e.sh **73/0**、e2e-resilience.sh **19/0**、e2e-transports.sh **12/0**、e2e-relay-modes.sh **21/0**,exit 全 0;源码 hash 与 final-linux 一致,复用同一构建(rh-server `26c4704d…`/rh-client `087bcb8c…`)。
- **主代理六项边界回归各 3 次复现**:`/tmp/rh-main-independent-review/final-six-regressions.json` 共 18 条结果(double-charge/abort-parent/cap/cancel-close/cancel-after-eof/close-before-write),本会话解析确认 **exits 全 0**。
- **final-gaps 定向回归**(`benchmark/runs/goal-repair/final-gaps/`):vendor yamux 28/28 绿;rh-transport final_gaps 4/4;ws 4 锁与 join_relay_pumps 测试 darwin 复跑绿。
- **final-lowat**(`benchmark/runs/goal-repair/final-lowat/`):最终生产二进制 TCP nomux splice 真实命中(每档 metrics `relay_path_total{path="splice"}=13`);未设任何 carrier SO_RCVBUF/SO_SNDBUF、未改 sysctl/源码。
- **红线静态**:redline_scan.py code 桶命中 0(2 条命中为解释性注释);本会话复读 `crates/rh-transport/src/quic.rs:24` `QUIC_STREAMS_CAP: u32 = 1 << 16`(65536)钳制在位;64KiB 转发缓冲另有单元常量锁(copy_buf_cap_is_64kib)。
- 流程红线:主代理审计确认不提交、不覆盖无关改动(`preexisting-protected.json`);三测试机收尾 CLEAN。

## 三、仍未达(性能,如实;不将局部诊断泛化到未测协议/方向)

- **round-2(旧构建,`benchmark/runs/goal-repair/round-2/verify-goal-report.txt`)**:总体 FAIL——**G1** TCP nomux 单流 1943/2113 Mbps < 3000(CPU 全过);**G2** TLS mux 单流 1172/1752 Mbps < 2500;**G6** 配对保留率 **25/36 档 <80%**;G3/G4/G5/G7 PASS。
- **final-lowat(最终构建,TCP nomux splice,n=3 中位)**:G1 仍未达——三档 RH_NOTSENT_LOWAT{65536,262144,1048576}+默认轮 tunnel 中位 1845–2302 Mbps,全部 <3000;CPU <50% 单核全档通过(最高 36.9%);**LOWAT 档位无可辨别系统收益**(绝对中位均在两跳 ~2G 波动带内;1048576 轮 81.2% 保留率伴随同轮 direct 偏低,属比值效应非档位功劳);TLS/KCP/QUIC 未覆盖。
- **最终源码的完整六协议性能矩阵未重跑**:round-2 不是最终源码清单;G3/G4/G5 的通过证据来自历史构建,最终构建上未复测。
- **实测局限(仅陈述测到的事实,不作绝对化结论)**:同拓扑参照 relay 与生产 rh 两跳均落在 ~2.0–2.2Gbps 带,这是当前实测对照,不构成软件上限的证明;NIC 硬件 USO off 不等于内核软件 GSO 不可用;>100% 单核口径进程 CPU 是多线程进程占用,不等于整机饱和;tls 上行 1341–1450 vs 下行 ~1950 差额、quic 高 CPU 低吞吐、kcp ACK 风暴均未完成热点定位(无 perf 级采样)。

## 四、需用户决定(未授权不执行)

1. **sysctl 解锁实验**(rmem_max/wmem_max 等):wmem_max=212992 使更大 iperf3 窗口被 clamp,而显式扩 socket 缓冲被红线禁止——解锁该空间需用户授权环境改动与恢复范围。
2. **网卡/拓扑变更**(如 vmxnet3 USO fixed 换网卡):平台级决策。
3. **最终构建完整六协议性能矩阵是否重跑**(约一轮 3×3 配对全矩阵):用于把 G3/G4/G5/G6 证据落到最终源码;当前授权仅覆盖正确性 e2e 与 TCP LOWAT 诊断。
4. 红线内仍可探但尚无达标证据的方向是否继续投入:KCP 窗口 A/B、tls 单流差额归因(需授权 perf 级采样)、LOWAT 对 mux/copy 路径的对照(仅 TCP nomux 已证伪)。

## 证据索引

- 正确性:`benchmark/runs/goal-repair/{final-gaps,final-linux,final-linux-r2}/`
- 性能:`benchmark/runs/goal-repair/{round-1,round-2,diagnosis-after-round-2,targeted-before-round-3,final-lowat}/`
- 主代理独立证据与完成度审计:`/tmp/rh-main-independent-review/`(completion-audit.md 结论同为「目标未达成、不宣称绝对无缺陷」)。
- 历史 1059 行全文:[performance-repair-history.md](performance-repair-history.md)。
