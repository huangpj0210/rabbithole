# G1 TCP 非复用单流专项方案

目标：真实三机链路中，TCP `multiplex=false`、`relay_mode=splice`、单流上传与下载分别达到 **≥3000 Mbps**。沿用既有 G1/G7 判定：每个方向 ≥3 次独立 direct/tunnel 配对测量的中位数；两端进程 CPU 按方向分别 <50% 单核，采样覆盖 ≥95%。不把工作流结束、单测通过、loopback 峰值或多流峰值当作完成。

## 1. 已核对事实

- 原始 `benchmark/runs/performance-closure-c84/candidate-r0/verify-goal-report.txt:13-19` 确认 G1 FAIL：2077/2156 Mbps，server CPU 30.22/28.09%、client CPU 34.6/14.61%。对应三组原始 iperf JSON 与目录字段一致。相同窗口 direct 中位数为 3182/2956（历史文字写 2957，取整差异不改变结论）。
- `crates/rh-server/src/data.rs:299-325` 与 `crates/rh-client/src/forward.rs:133-160` 均有裸 TCP splice 入口。服务端限额守卫存活、计量/规则取消/限速钩子不可绕开。前缀未消耗时必须保持 copy 回落。
- `crates/rh-transport/src/splice.rs:248-388` 当前每次 fill 都建立 readable/select future，然后排空管道；每次 drain 都建立 writable/select future。这是可实验的控制流，不是已证实的瓶颈。
- Tokio 1.53.2 `AsyncFd::readable/writable` 在未遇 WouldBlock 时保持缓存就绪；因此“每次 await 都必然产生 epoll 唤醒”是错误归因。必须区分 ready 立即返回、真实 Pending、syscall EAGAIN、协作式让步。
- 历史 NOTSENT_LOWAT、affinity、refcopy/refsplice 对照未证明稳定收益；只否定当时具体干预，不证明环境硬上限。历史 raw 单流上行零重传不等于 carrier 没有接收窗口或 app-limited 问题。
- 三机已只读核验可达：test1 172.16.1.6、test2 172.16.1.17、test3 172.16.1.18，Linux 5.15、8 vCPU。既有测试端口未见残留服务；有 strace/ss，无 perf。`/tmp/rh-bin/iperf3` 为既有 3.17.1，勿误用系统 3.9。TCP cubic、自动缓冲默认值仅观察，不修改。
- 工作区已有 A–H 修补、KCP 与 benchmark 未提交改动。它们是输入基线，完整保留，不回退、不提交、不覆盖历史证据。

## 2. 推荐改法：先确定限制，再逐项落地

### A. 加入最小且默认不影响热路径的观测

由 glm-5.3 新建 G1 专项执行/核验工具，复用 `closure_c84.py` 的确定性快照、三机 Termark 通道、进程身份治理、双端 CPU 与既有 raw schema；不修改固定门槛、旧核验器、iperf 时长/缓冲/单流定义。定向轮保留全部失败。需要的观测为两端实际 rh PID/TID 的 `/proc` CPU/上下文切换、带时间的 `ss -tinm`（三段 TCP 的队列、cwnd/RTT、rwnd_limited、sndbuf_limited、notsent、app_limited）、pipe 实际容量、splice 次数/字节、EAGAIN、实际 Pending/等待次数、搬运量分布。

计数器采用每泵本地累计、结束时输出或显式环境开关；禁止每包全局锁/原子成为新热点。strace 仅作独立短诊断，受 ptrace 权限限制时如实记录，绝不把受扰动样本算最终性能验收。固定工作量 Linux 对照使用相同钩子与计数器，每臂 ≥128MiB，交替至少 3 组；它只负责归因。

### B. 第一优先：非阻塞快试 + 有界排空批次

若计数表明 readiness/future/协作调度开销显著，针对 `splice.rs` 实现单变量候选：非阻塞 splice 快试，只有 EAGAIN 后等待对应 fd 的 readiness；保留无丢唤醒的 guard/try_io 约束。一次 poll/批次有字节或操作预算，检查取消与 abort，不允许在两端永久 ready 时霸占 executor。可在连续 fill/drain 间复用就绪 guard，但不得在未观察 EAGAIN 时错误 clear_ready。

严格保持：上行 fill 后立即计量、下行按实际送达计量；限速 reservation 的 consume/refund；读 EOF 半关闭、部分写/错误、父取消、panic 收敛；管道有界且资源创建失败原样回落；任何 drain 返回零必须停止零进展循环而非 busy spin。

### C. 第二优先：TCP 发送聚合节律

若 TCP_INFO/搬运分布显示小块/发送气泡，隔离试验 `SPLICE_F_MORE` 仅在明确还有管道待送字节时使用，最后一次不带；或有界合批排空/单方向调度改进。禁止永久 TCP_CORK、为了好看取消 NODELAY、人为增加未受限缓冲。先写小包低负载/双向背压/EOF 回归，只有真实对照支持且无延迟退化才采纳。

### D. 其余方向的触发条件

pipe/drain 尺寸只在实际容量/系统调用密度支持时做独立 A/B；不再盲扫已证伪的 LOWAT。SO_BUSY_POLL 仅可作为默认关闭的局部实验，必须检查 CPU 门槛，不能以忙等换吞吐而宣布 G1 通过。io_uring、新线程模型、系统/NIC/sysctl 改动不在本方案内；若软件候选无收益，保留 best/default 并列出真实阻断，不擅改拓扑或测试条件。

## 3. 工作流分工与修改范围

- **方案独立复核**：只读源码与证据，指出上述判断/候选会在哪里失效；不写产品代码。
- **验收装置工程师**：独占 `benchmark/g1_tcp.py`、`benchmark/py/tests/test_g1_tcp.py`、新 G1 实验配置及证据目录。不得改 `verify_goal.py` / source_inventory / 原始样本；必要装置扩展限定为新工具中复用，不弱化既有端口、身份、hash、nofile 与超时检查。
- **TCP 实现工程师（glm-5.3）**：独占 `splice.rs`、新增 splice 定向测试/固定工作量 example。通常不需修改 tcp.rs/data.rs/forward.rs；若确有调用边界瓶颈，先在报告列具体证据后最小修补，保留所有既有改动与语义。不改 KCP/TLS/WS/yamux/QUIC/认证/协议。
- **代码独立复核**：读取实际新增 diff，审核取消、背压、计量/配额、错误、唤醒与 fairness；具体疑点由复现或确定性回归确认。
- 产品编辑与基准运行串行，源树测量期间冻结；只读评审可并行。新证据根 `benchmark/runs/g1-tcp-20261014/` 只增不覆盖。

## 4. 机械验收与报告

1. 首先运行既有 Python 装置自检和源码未改的当前树 TCP nomux baseline（不是旧 HEAD archive）。用真实失败与退出码驱动下一步。
2. 每个候选一次只改一个生产机制，记录 baseline/candidate 生产 diff 与 source/build hash；定向回归、固定字节对照与三机每方向 10 秒 ×3 配对均须实际执行。最多 4 轮本次候选迭代，无改进则撤回本次候选而不碰用户原有修改。
3. 全量命令由 workflow `world.run` 统一：`cargo fmt --all --check`、`cargo clippy --workspace --all-targets --locked`、`cargo test --workspace --locked`、Python unittest。Linux splice 模块须在 Linux 真正运行，macOS 绿不构成覆盖。
4. 最终冻结版本运行 Linux workspace/release build、四套既有 E2E（e2e、resilience、transports、relay-modes）与 TCP nomux 真实三机复测。现有 `closure_c84.py targeted` 无四 E2E 日志回收，G1 新装置补齐，不伪造 accept/full 标签。
5. G1-only 核验调用未修改的 `verify_goal.EvidenceDir` / `evaluate_gates` 的 G1 分支，再独立检查样本/所有失败、生产源与最终工作区、二进制/端点 identity、一流 raw 目标与两个方向、CPU/nofile/时间与正常 relay 路径。选择 G1 分支只是本次明确专项范围；不宣称 G2–G7 全部达成。
6. 所有 benchmark 与 subprocess 外层 deadline + 所有者进程组清理；远端仅清理本轮记录且身份匹配的进程。不安装工具/改主机配置，不杀未知进程。凭据只留 0600 文件，报告脱敏。
7. 最终 `docs/performance-g1-tcp-report.md` 要回答：改了什么、为什么采纳、双向中位与全部样本、CPU、回归、源码/二进制 hash、默认/实验开关是否相同、是否真正达到 G1。未達标写 FAIL，不以局部改善冒充完成。
