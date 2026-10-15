# performance-closure-c84 收口轮报告(2026-10-14)

一句话结论:五项风险修复(A–E)全部红→绿落地,验收装置与三机 full-accept 真实执行(机械结果:runner=0、artifact verify=1、G1–G7=1);**固定门槛未达标:G3/G4/G7 PASS,G1/G2/G6 及 G5 的 kcp 单缺口 FAIL,candidatePassed=false**。不以报告完成、单项协议改善或红绿数量替代门槛结论。

## 0. 身份与机械结果

- 基线 `c84f63174cd2ebafd994c4fafacd5beba4a0240b`(分支 `fix/performance-closure-c84f631`,快照时 clean,baseline-local/commit.txt);HEAD 仍为该检查点,**本轮无新提交**(会话内 `git rev-parse HEAD`/`git log` 实读);全部改动为工作树未提交状态。
- 三机:test1 `cizwDkKnGqY47eBL`/172.16.1.6(server)、test2 `Du9x3tHGGDhcZJNx`/172.16.1.17(target)、test3 `tN76t9aSo3zaCwJJ`/172.16.1.18(client),均 Linux 5.15.0-47;rustc/cargo 1.98.1(toolchain pin),nofile soft=hard=65535,时钟偏差 ≤0.052s(run-index env)。
- 机械结果(命令与原始输出在 `benchmark/runs/performance-closure-c84/checks/`):
  - runner `python3 benchmark/closure_c84.py accept --out-dir …/candidate-r0` **exit=0**(linux-full-accept-atoctbgz,5955s,events.jsonl 全程留痕);
  - artifact `closure_c84.py verify --evidence …/candidate-r0` **exit=1**,18/19 项过(唯一 FAIL=G 门槛,linux-artifact-verify-m3l3wbp3);
  - 门槛 `py/verify_goal.py --evidence …/candidate-r0/evidence` **exit=1**(fixed-performance-goal-xxdxm43w;原件=candidate-r0/verify-goal-report.txt)。
- 时序:candidate-r0 快照=16:25–18:04;其后 20:04–20:41 的 test-only 修(listener 探针)与 H 计数器(state.rs/data.rs)**不在该构建内**,已做定向回归+本机 mac 全量(workspace 15 套 206 passed/0 failed/2 ignored、clippy exit=0,optimized-*),未重跑 Linux 强验收。

## 1. 逐项 A–E(+追加 F/G/H)

**A KCP 半关闭只关本地写侧(修 F5)**:`Guarded.write_closed` 区分"本地写关完成"与"会话终止",poll_read 不再被写关闭杀掉,对端不 ACK 由 dead_link 浮出 Err;不加短超时、不伪造远端 EOF。红:基线 kcp.rs `16b72155…` 真实双端长度协议,写 7B→shutdown→对端 25ms 后回 8B→本端 UnexpectedEof,`main-independent/halfclose-exact-baseline-red.log`(0/1,exit101);绿:`halfclose-candidate-independent-green.log`(1/1,EXIT=0)。

**B unread 回收(修 F6)+ idle 假成功修正**:进展=消费(`consume_seq`)或无待交付时入包;read_buf 有货时纯窗口探测/重传不再刷新 idle,90s 无进展有界回收;`run_session` 增 `idle_timeout` 测试参数(生产默认不变);MAX_READ_BUF 精确界不动。红:基线 `main-independent/c84-risk-red.log`(0/2,read_buf 预置 1B 零消费 250ms 不退);绿:`idle-candidate-independent-green.log`(1/1)。候选期主代理复核再挖一红:未 ACK 写在 idle 终止被伪报成功关闭(候选快照 `8ee7f84b…`,`shutdown-idle-result-red.log` 0/1 exit101)→ 修复后 `shutdown-idle-final-independent-green.log`(1/1,EXIT=0);该修正含在最终 `610dd1e9…` 内。

**C PunchOutput WouldBlock 有界队列(修 F9)**:punch 复用 connect 的 `UdpBacklog`/drain(目的址泛化 `Connected/Peer/Punch`),pop→send 一次、WouldBlock 回插队头每报至多送达一次;PunchCancel RAII 不动;错误桥接 dial。红:基线 `kcp/red/repro-baseline-red.log`(5 passed/5 failed,含 `c_punch_wouldblock_datagram_not_lost`,kcp.rs=16b72155);绿:新守护 `c1_punch_wouldblock_enqueues_not_fake_ok`/`c3_punch_fatal_bridges_to_io` 稳定绿;**`c2_punch_rotating_learned_delivery_exactly_once` 首两轮 FAILED(66/1,`阶段1 drain 应 Empty: Blocked enqueued=2 sent=0`,kcp/candidate-tests/lib-kcp{,-rerun}.log)第三轮绿(67/0,lib-kcp-rerun2.log)**——偶发未闭环,如实保留三份日志。

**D ACK 合批**:run_session 入包分支 `batch.clear()` 后 `recv_many(&mut batch, 64)`,n==0 即 break;批内逐包 input、整批 1×update+1×flush;512 段/256KiB/背压界不动。按 BATCH-AB-REVIEW 交付标准做**单因果隔离对照**(`kcp/counter-ab-isolated/`,batch 侧=当前工作树 `610dd1e9…`,single 侧仅 recv_many 上限 1,production.diff 一行):t1 中位 306.9→664.9、t4 317.3→644.6 Mbps(组间无重叠),CPU ms/MiB 31.5→15.5/30.8→15.7,flush/MiB 1665→178/1644→162,低载 p50 87→88µs 无回归(p99 292→239);旧 avg_batch≥8 私有门槛未满足(实测 5.4–5.9 包/唤醒)如实列出,采纳依据=实测整体收益+correctness(analysis.json honesty/verdict)。保留,不触发撤 hunk。

**E 端口诊断(api.rs/state.rs)**:`classify_port_occupation` 按协议族+端口+IP 重叠匹配**活**自身监听(控制面/web/vhost/ssh_gateway/metrics 实际监听地址含热改),2007/3005 数字码与 JSON 形状不变仅消息细化,OS 试绑兜底保留。新守护 8 例(metrics 真实监听/web/vhost、外部文案逐字节不变、UDP/TCP 不误冲突、3005 同分类、组共享不误报、自动分配不动、enable+回滚、无幽灵预留):`ports/server-diag-e.log` api:: 8/8、全 bin 20/20;`server-diag-e-stability.log` 连续 5 次全绿;`ports/commands.txt` 记 `--lib`→`--bin` 偏差(rh-server 纯 bin crate)与测试清单。修前无测试,属允许的"修前即绿"。

**F 验收装置(lab)**:`closure_c84.py`(运行时 SHA256 `fc82833e…`,当前工作树已再演化为 `e6b5b9f3…`、未参与 candidate-r0)——自检 14/14、stdlib 单测、LAB-PRECHECK 九项缺口修复;linux-baseline attempt1 cargo test EMFILE(fd 软限 1024)原样保留,装置侧改 fd preflight+ulimit 65535;launch 通道 -1/超时不判死、以远端 pid 文件恢复(accept 内 11 次恢复成功)。`linux-baseline-r2` targeted-baseline(4 配置单流双向)drive/test/build exit 全 0,基线二进制 rh-server `1c33664a…`/rh-client `cac8421e…`。见 `lab/README.md`。

**G TCP listener 测试混杂(仅测试文件)**:Linux 默认并行 3/10 轮挂起(`listener-probe/linux-default-parallel-10rounds.log`,ROUND_EXIT=101×3,原样保留未标 INVALID)。修:test-only——探针改固定互异源端口(排除自连接)+先写后读(强制内核裁决四元组)+新机制锁定测试;2s/3s 预算、断言、并行性不变,生产零改动。绿:mac 10×"7 passed"(`tcp-cancel-race/mac-postfix-parallel-10rounds.log`)、**Linux `listener-probe/linux-fix-r2-10rounds.log` 10×ROUND_EXIT=0(exit 文件=0)**。机理(内核拆除竞态孤儿/跨进程端口重用)未定论,绿不构成产品正确性证明。

**H 会话失败计数器(state.rs/data.rs)**:`rabbithole_session_failures_total{reason}`(health_fast_fail/channel_unavailable/quota_rejected/start_forward_*/relay_*_error/panic),规则取消与 clean EOF 不计;relay 数据面零改动。定向:`cargo test -p rh-server --bin rh-server` 23/0(新增 3 例),clippy -p rh-server 两文件零告警。动机=G5 kcp 1/12288 建立层不可归因;**下次运行携带本计数器才能证伪**(`session-failure-counter/README.md`)。

## 2. 公开回归红绿汇总(路径均在 `benchmark/runs/performance-closure-c84/`)

| 项 | 红 | 绿 |
|---|---|---|
| A | main-independent/halfclose-exact-baseline-red.log 0/1 exit101 | halfclose-candidate-independent-green.log 1/1 EXIT=0 |
| A/B/C repro | kcp/red/repro-baseline-red.log 5 failed | kcp/candidate-tests/lib-kcp-rerun2.log 67/0;kcp/shutdown-guard/green-lib-kcp-full.log 69/0 |
| B | main-independent/c84-risk-red.log 0/2 | idle-candidate-independent-green.log 1/1 EXIT=0 |
| B-idle 假成功 | shutdown-idle-result-red.log 0/1 exit101(候选 8ee7f84b) | shutdown-idle-final-independent-green.log 1/1 EXIT=0 |
| B 守护(idle/通道关闭) | kcp/shutdown-guard/red-{idle,channelclosed}.log 各 0/1 FAILED | green-{idle,channelclosed}.log 对应绿(green-lib-kcp-full 69/0) |
| C c2 | —(新守护) | **两轮 66/1 FAILED→第三轮 67/0(偶发,未闭环)** |
| D | 行为改动无红;冻结守护 b12 允许修前绿 | counter-ab-isolated 组间无重叠(§1.D);latency p50 无回归 |
| E | 修前无测试(允许修前即绿) | ports/server-diag-e.log 8/8+20/20;stability 5×全绿 |
| G | linux-default-parallel-10rounds.log 3/10 ROUND_EXIT=101 | mac 10×7/0;linux-fix-r2-10rounds.log 10×ROUND_EXIT=0 |
| H | 可观测缺口(非缺陷红) | rh-server bin 23/0 |
| 全量 | — | candidate-r0 Linux `cargo test --workspace` exit=0(15 套 214/0);accept 后 mac 全量 15 套 206/0/2、clippy exit=0 |

## 3. 最新 Linux/source/run hashes

| 对象 | 值 | 出处 |
|---|---|---|
| source tree_hash(80 构建输入,含 3 migrations+rust-toolchain.toml) | `6e8f720b22e0e457b2a1411234b287c5a4790638ae2a5378eb6cc46b554c442e` | run-index.json;verify C0/C1/C3 与当前工作区/跨目录一致 |
| sources.sha256(76 Rust/Cargo 文件) | 清单 sha256 `24f655e989423324c73386fcf61f43f44b2c30cae57faf7b850816b490ef3682` | verify [build] 行 |
| rh-server / rh-client(Linux release) | `2912ab8cd16099d878b41a88662617842bd292f3933d2142ee0448526174fb26` / `dd1ecb186d7c22776b41417728659cb5e71d83a3bfb8d53d359325f12e6cb295` | binaries.json;7 证据目录 endpoints identity 逐一致 |
| kcp.rs 基线→当前 | `16b721555dc53afdaea177cfef7c2ca4cc2ee1430f92099972bd97a0a7b380ec` → `610dd1e93d79b6840b1a66575f462ed90ad46c8dad86a9f8d5e298d1613c6c13` | baseline-local / 本会话 shasum;候选内快照曾为 8ee7f84b… |
| api.rs/state.rs/data.rs 当前 | `07e63545…` / `f945dd6d…` / `5931bff0…`(基线快照 196ee063/96a37758,data.rs 未入快照) | 本会话 shasum;state/data 含 accept 后 H 改动 |
| run_id(7 目录) | c84-accept-{tcp-mux0 1791967035, tcp-mux1 1791967790, tls-mux1 1791968523, ws-mux1 1791969276, wss-mux1 1791970028, quic-mux1 1791970779, kcp-mux1 1791971535} | verify-goal-report.txt(互异,窗口无重叠) |
| 编排器(运行时) | closure_c84.py `fc82833e…`;git_status_sha256 `29f43dc7…` | run-index.json |

## 4. 四套 E2E 真实退出(verify B 逐套读原件,非 index 摘要)

| 套件 | exit 文件 | 原件 PASS/FAIL |
|---|---|---|
| tests/e2e.sh | logs/e2e-e2e.exit=0 | 73/0 |
| tests/e2e-resilience.sh | logs/e2e-resilience.exit=0 | 19/0 |
| tests/e2e-transports.sh | logs/e2e-transports.exit=0 | 12/0 |
| tests/e2e-relay-modes.sh | logs/e2e-relay-modes.exit=0 | 21/0 |

A1/A2 同轮:Linux `cargo test --workspace` exit=0(15 套 214 passed)、`cargo build --release` exit=0。

## 5. 六协议 G1–G7 与有效样本/paired ratio(candidate-r0/evidence,7 目录)

- **G1 FAIL**:tcp mux=0 splice 单流中位 upload 2077 / download 2156 Mbps(<3000);同窗口 direct 中位 3182/2957。CPU 上/下行 server 30.22/28.09%、client 34.6/14.61% 单核口径(全部 <50% 达标),cpu.jsonl 按隧道窗实算覆盖 98–99%(≥95%)——G1 FAIL 纯吞吐。单流 paired retention:upload 63.4%、download 71.8%。
- **G2 FAIL**:tls mux=1 单流中位 1229/1801 Mbps(<2500);单流 retention 43.1%/54.7%。
- **G3 PASS**:tcp/tls/ws/wss/quic/kcp(mux=1,splice)单流正吞吐。
- **G4 PASS**:1200B UDP 排空(5s)去重后六协议损失中位均 0%(各 n=3,dup=0,sent==recv_unique,60k–104k/样本)。
- **G5 FAIL 仅 kcp**:verified 逐样本 tcp/tls/ws/wss/quic-mux 与 tcp-nomux 对照均 4096×3(alive_by_round 5 轮逐轮核对);kcp=[4095,4096,4096]——s3 established=4096 而 initial_verified=4095,该 1 条保持期 5 轮始终未过应用层往返。窗口 RSS 峰值 server ≤312 MiB(quic)、client ≤186 MiB。
- **G6 FAIL**:verify-goal-report 原件逐条 **22/36 条件中位 <80%**(独立复核概述 21,以原件为准):tcp u1 37.2/d1 65.8、tls u1 43.1/d1 54.7、ws u1 34.1/d1 66.7、wss u1 36.9/u4 52.6/d1 50.6/d4 72.3、quic 全档 19.9–29.9、kcp 全档 13.6–22.9(%;达标条件如 tcp u4 90.8/d4 104.9/u16 95.1/d16 123.0 见原件)。
- **G7 PASS**:7 目录 × {up,down}×{1,4,16} 每条件 3/3 有效配对、invalid=0、missing_raw=0、252 个 raw/*.exit 全 0、同 key 窗口重叠 0(verify G7 + 独立抽查计算)。

## 6. 独立复核结论(approved=false,候选不通过)

五项定性(独立抽查非重跑,命令与计算在其报告内):①G1 真实性能 FAIL,CPU 充裕→瓶颈在单流转发路径本身(与 G6 tcp/1 流同源);②G2 真实 FAIL,TLS 上行显著低于 tcp-nomux 上行需单独立项;③G5 唯一正确性缺口=kcp 建连首包竞态(3 次中 1 次),需查 KCP 建连;④G6 真实 FAIL,建议 targeted A/B(RH_QUIC_CC 等用户态旋钮)分别归因 quic/kcp;⑤观察:harness 新增 launch 恢复路径未深审(下游产物自洽佐证)、tcp-nomux 16s upload client CPU 66–69% 近饱和影响 G6 解读。另有主代理 DIAGNOSTIC-CAUSALITY-REVIEW:G5 1/12288 不得写"超窗瞬态";refrelay 对照只能否定"该干预有稳定改善",不得推"环境为上限"。

## 7. 未解决/未验证(不做即称未做)

1. **G1/G2/G6 未修复**:本轮吞吐路径零改动(refsplice/copy 对照与 rh 同噪声带 2.0–2.2G、affinity 无收益、无线程级热点证据);修复方案未立项,不预判百分比。
2. **G5 kcp 1/12288 原因未定**:重跑未复现≠瞬态;需下次运行携带 H 计数器(EOF 型→查计数器;timeout 型且计数 0→支持排队/ARQ 退避假设)。
3. **listener 挂起机理未定论**:Linux 5.15 拆除竞态孤儿/跨进程重用未排除;10/10 绿非数学证明;旧 3/10 失败日志并存保留。
4. **c2_punch 守卫偶发未根治**(2/3 轮红)。
5. **termark exec 60s/-1 根因不可查**(工具侧);恢复路径(11 次)未被深审。
6. **candidate-r0 后增量未重新强验收**:G/H 改动只有定向+mac 全量;Linux 级 world.run 复验未执行。
7. **D 的 b11/b12 无独立红证据**(冻结守护,允许修前绿;shutdown-guard/red-new-tests.log 是一次 cargo 参数错误留痕,非测试红;真红仅 idle/channelclosed 两守护);D 不降低 G1–G7,三机达标以 world.run 为准。
