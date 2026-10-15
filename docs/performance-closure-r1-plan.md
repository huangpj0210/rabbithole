# performance-closure r1 计划(candidate-r1,一页)

输入(本会话实读):`docs/performance-closure-c84-report.md`、`candidate-r0/verify-goal-report.txt`(G1 2077/2156、G2 1229/1801、G6 22/36 条件 <80%、G5 仅 kcp s3=4095/4096,原件 :13-107)、`candidate-r0/diagnostics/README.md`(P2 未复现+无产品计数器;P3 对照带)、`main-independent/DIAGNOSTIC-CAUSALITY-REVIEW.md`(5 条事实约束)、`main-independent/BATCH-AB-REVIEW.md`(单因果交付标准)。基线=HEAD c84f631+工作树未提交 A–H(含 H 计数器 `rabbithole_session_failures_total`、loadgen failures[] db91a244);`candidate-r1` 目录实测不存在(ls 报 No such file)。门槛 G1–G7 不降;无百分比承诺。

## T1|G5-kcp 复现与归因(优先,判定树来自 causality-review#1)

- 假设 H1(可证伪):s3 的 1/12288 属四类之一——EOF(建立层丢弃)/timeout(载体窗口·ARQ 尾部退避)/badline/senderr(协议层);旧证据无法区分,新双计数器可以。
- 最小实现:closure_c84.py targeted(新构建,r1 源含 H 计数器;不复用 r0 二进制——H 不在 r0 构建内)kcp tunnel 4096,repeat≥4,最多 6 轮,每轮回收 endpoint 指标族+failures[]。原失败 evidence/accept-kcp-mux1 s3 保留不动。
- 判定:reason=EOF→查 `session_failures_total{reason}`(channel_unavailable/start_forward_* 等映射见 session-failure-counter/README.md:24-33)→归池/建立层,开 T-data 小修(data.rs/state.rs 排他授权仅此触发);reason=timeout 且计数=0→支持载体假设→T2;badline/senderr→KCP 修复任务定点;6 轮不复现→如实报告,T2 无据不做,G5 以 accept 复测结果为准(可能仍 FAIL,不掩饰)。

## T2|KCP 会话级总预算动态发送窗口(仅 T1 归因支持时;模板=mux.rs:725-731 预授信池先例)

- 假设 H2:512 段站立上限(kcp.rs:68,搬运门 kcp.rs:916-936)在 4096 突发下使单会话首往返被尾部退避拖过 10s 窗;空闲容量借出可缩短尾部。
- 最小实现:MAX_KCP_PENDING_SEGS=512 保留为每会话保底;新增进程级 `POOL_TOTAL_SEGS`(AtomicUsize,静态常量+编译期断言):会话 wait_snd 顶格且对端持续 ACK 时借额外配额,ACK 降回即还,池空退保底不等待。总内存静态可算并写入断言:`N_max×(MAX_WRITE_QUEUE_BYTES+512×MSS)+POOL_TOTAL_SEGS×MSS`(4096 代入,等价替换 kcp.rs:63-68 的 858KiB/会话注释,全量说明)。不动 MSS/MTU、 wndsize 协商语义、半关闭/idle/unread、backlog 界;不加私有帧。
- 验收:kcp 全测试绿+新守护(池界精确/借还不泄漏/池空退保底,允许修前绿不弱化);kcp_loopback_bench 固定工作量 KcpCounters 单因果 A/B(run_bounded 外层);loopback 仅归因,达标只认三机。

## T3|c1/c2 punch 偶发闭环(根因已定位,缺当前树 5 连跑)

- 根因(证据):两红均为旧快照断言环境假设——c2 假设"prime 后首轮 drain 必不 Blocked"(lib-kcp.log:111,enqueued=2 sent=0)、c1 假设"灌满 loopback 必现 WouldBlock"(lib-kcp-rerun.log 前置失败);当前树已确定性化:c2 `drain_until_empty`(kcp.rs:5022-5042,Blocked 按生产语义等 writable,5s 有界)、c1 合同注入(kcp.rs:4962-4966)。kcp.rs mtime 14:22 晚于 rerun2 14:10——第三轮绿是旧代码侥幸,新写法零轮验证。
- 验收:run_bounded `cargo test -p rh-transport --lib kcp::` 连跑 5 轮 ROUND_EXIT=0(证据入 benchmark/runs/performance-closure-c84/ 新子目录);任一轮红→继续定位(候选:read_all 2s 预算/100ms 静默窗),禁放宽断言、禁串行凑绿。

## T4|G2-tls 观测先行,≤2 微改

- 假设 H3:tls 上行 1229 与 client CPU≈108% 近饱和(ask 材料)是客户端每 TLS 记录加密/写放大所致;yamux 64KiB split(mux.rs:734)到 rustls 16KiB 记录的写未聚合。
- 最小观测(先装):yamux→TLS 边界计数 adapter(bench/env-gated,不进默认热路径):poll_write vs poll_write_vectored 次数、bytes/call、is_write_vectored 是否实际到达(透传位在 data.rs:895-920/lib.rs:222-233,需证 yamux Stream 真走 writev)。
- 微改候选:(a) 聚合写/打通 writev;(b) 记录路径一次 feed 多记录。loopback TLS 固定工作量 counter A/B 定夺;无收益→no-change 如实报告。

## T5|QUIC CC 定向 A/B

- 假设 H4:quic 全档 20-30% 保留率部分由 cubic 窗限制;bbr(开关已在 quic.rs:50-51)可移动。spec-h2-quic-cc.json 已存在未被运行(diagnostics 目录仅 spec 无结果)。
- 实现:targeted cubic vs bbr,10s×3 配对。有收益→保留默认 cubic 不动,记录为默认外实测建议+复测;无收益→no-change,quinn 会话串行假设如实列为需更深改动的未立项项。

## T6|G1/G6-tcp splice 方向(env-gated A/B)

- 假设 H5:splice 单流 1.98-2.2G、CPU 25-36% → 唤醒/syscall 节律限制(非计算);每 socket SO_BUSY_POLL(红线允许 env-gated、默认关)或 DRAIN_SLICE(splice.rs:29,:353)/唤醒批量可移动吞吐。
- 实现:两臂各 10s×3 配对三机 targeted(计数:每 MiB splice 次数/wakeups);任一无收益即 no-change。不宣称环境硬上限;irq/RSS/GRO 等列需授权项。direct 自身 2815-3062 事实保留——若 T6 全 no-change,G1 缺口如实报告。

## 执行顺序、排他、验收

顺序:T3→T4 观测→T1(一次 r1 构建,多轮复用)→T5/T6(同构建 env 旋钮)→T2(条件触发)→收口。文件排他:kcp.rs+其测试+kcp_loopback_bench=T1/T2/T3;splice.rs/quic.rs/tls·mux 路径=T4/T5/T6;data.rs/state.rs 仅 T-data(T1 EOF 归因触发);api.rs 本轮不动;loadgen/closure_c84 仅观测装置、判门逻辑不动。验收:本地全套门(workspace/clippy/python 由脚本 world.run 一次执行)→targeted 定向对照留痕→closure accept(candidate-r1 新子目录)→verify+verify_goal 全门;失败最多一轮修复重验。全命令外层 deadline+start_new_session+killpg(复用 kcp/tools/run_bounded.py);性能结论必须有固定工作量计数对照+单因果隔离(production.diff 记录)。

## git status 评估(2026-10-14 实跑 `git status --porcelain`)

13 条:8 修改(loadgen.py/api.rs/data.rs/state.rs/kcp_loopback_bench.rs/kcp.rs/tcp_listener_lifecycle.rs/performance-repair-status.md)+5 未跟踪(closure_c84.py/test_closure_c84.py/3 docs)。全部属 c84 已验证基线(A–H+装置+文档),**保留不回退、不提交**;唯一缺口=kcp.rs 14:22 的 c1/c2 确定性改写未经任何轮次验证(T3 补)。r1 新改动按排他矩阵落位,accept 时 closure_c84.py source-snapshot 自动绑定;旧证据零覆盖。
