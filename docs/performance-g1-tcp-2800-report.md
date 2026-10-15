# G1-2800 TCP 非复用单流：主代理诊断、指定编码与真实验收报告

## 结论：FAIL，性能目标未达成

最终默认配置 TCP / `multiplex=false` / `relay_mode=splice` 的真实三机单流，上传中位 **1963.57 Mbps**、下载中位 **1990.58 Mbps**，均低于用户最新要求 **各 ≥2800 Mbps**。原始证据有效、CPU达标、Linux及四套E2E通过；失败仅在真实吞吐门槛，不是样本/构建缺件。

不把234条诊断、代码交付、26项Linux回归或完整workflow完成当成G1达成。当前没有足以支撑“在现有代码边界内已找到必达2800改法”的证据，也不宣称环境存在硬上限。

## 1. 分工与交付

- **主代理**亲自分析、执行三机诊断、独立复核原件、制定精确代码规格、独立审查GLM差分、复现失败与复验Linux、运行最终强验收和独立核验。
- **GLM-5.3**仅按 `benchmark/runs/g1-tcp-2800-main/coding-spec.md` 编写两个授权文件的实现和回归，不承担诊断、优化方向选择、代码评审或性能判断。
- 本轮只运行完整编码workflow，没有运行EvalWorkflowSnippet或留待补的脚本空位。既有run通过AmendWorkflow修订，不重复重写已完成代码。
- 最后完整编码run：`dwfrun-1c886e3c-3c29-42bb-9d9e-aa765102c4f9`，模型 `account:bigmodel-individual-coding-plan/GLM-5.3$max`。产物卡片「G1-2800 指定代码交付」不是性能验收报告。

方案：[performance-g1-tcp-2800-plan.md](performance-g1-tcp-2800-plan.md)。本轮证据根：`benchmark/runs/g1-tcp-2800-main/`，所有新实验独立目录，旧raw与失败只增不覆盖。

## 2. 最终单流全部样本（真实默认配置，精确raw位速率）

命令：`python3 benchmark/runs/g1-tcp-2800-main/tools/main_accept.py candidate-final`，内部真实执行 `python3 benchmark/g1_tcp.py measure --out-dir .../candidate-final --final`。测量命令exit0、1915.09秒；性能核验exit1，二者分别留痕，不能把runner0当目标通过。

两个独立server/client生命周期，每方向每窗口3次 direct/tunnel紧邻配对，10秒、P=1。G1只取24条单流记录；另有48条P=4/16诊断附带，不进本表。全部72条原始吞吐valid=true、exit0；0条invalid、0条静默过滤。

| 方向/路径 | 窗口1样本 Mbps | 窗口2样本 Mbps | 合并6样本中位 Mbps |
|---|---|---|---:|
| 上传direct | 2811.600, 2578.534, 2604.156 | 3071.862, 2740.891, 2919.441 | 2776.245 |
| **上传tunnel** | 2004.794, 2112.512, 2001.740 | 1925.393, 1915.187, 1905.379 | **1963.566 — FAIL** |
| 下载direct | 3234.174, 2984.183, 2735.424 | 2923.949, 3061.440, 2862.473 | 2954.066 |
| **下载tunnel** | 2016.992, 2106.345, 2134.669 | 1933.465, 1964.174, 1945.922 | **1990.583 — FAIL** |

以上从每条iperf raw `end.sum_received.bits_per_second/1e6`重算，不用记录整数越线。对应 `candidate-final/evidence/g1-tcp-nomux-w{1,2}/results-*.jsonl`、`raw/`。

主代理独立判定：

- `candidate-final-main-gates/verdict.json`：`evidence_valid=true,correctness_pass=true,cpu_pass=true,throughput_pass=false,pass=false`。
- `candidate-final-main-audit.json`：72条raw、24条单流、invalid0、stats-enabled日志0、issues=[]、`goal_satisfied=false`。
- 旧3000核验器及历史结果未改。新2800阈值仅来自用户明确调整；`tools/verify_2800.py`和`tools/audit_final.py`由主代理独立编写，17条边界/缺件/CPU/源码/binary/raw对抗用例通过，不能据自检代替产品验收。

同轮direct上传中位也略低于2800，说明窗口容量波动需要保留；此前主代理分段诊断三物理段单向均超过3200，故不能拿本轮direct2776推“2800在环境绝对不可能”。tunnel与direct差距依然真实。

## 3. CPU与覆盖（单核、按方向和端点分别计算）

| 方向 | server CPU中位 | client CPU中位 | 两端全部样本最小覆盖 |
|---|---:|---:|---:|
| 上传 | 29.34% | 34.395% | 97.989% |
| 下载 | 28.04% | 13.58% | 98.053% |

均满足既有CPU<50%单核与采样覆盖≥95%。原件是每个窗口自身的`endpoints/{server,client}/cpu.jsonl`，未跨目录拼配。CPU不是G1失败原因，但低CPU也不能单独证明syscall/唤醒是根因。

## 4. 最终同构建强回归（均由主代理执行和复核）

| 检查 | 实际结果 | 原件 |
|---|---|---|
| Linux `cargo test --workspace --locked` | exit0，232 passed，15套汇总 | `candidate-final/logs/test-workspace.log/.exit` |
| Linux `cargo build --release --locked` | exit0 | `candidate-final/logs/build-release.log/.exit` |
| `tests/e2e.sh` | 73 PASS /0 FAIL，exit0 | `candidate-final/logs/e2e-e2e.*` |
| `tests/e2e-resilience.sh` | 19/0，exit0 | `candidate-final/logs/e2e-resilience.*` |
| `tests/e2e-transports.sh` | 12/0，exit0 | `candidate-final/logs/e2e-transports.*` |
| `tests/e2e-relay-modes.sh` | 21/0，exit0 | `candidate-final/logs/e2e-relay-modes.*` |
| 独立Linux splice定向复验 | 26 passed /0 failed | `linux-splice-fixed/tests.log:46-94`、run.json |
| 最终macOS fmt/clippy/workspace | 全exit0，workspace206 passed | `main-gates-fixed/` |
| 既有Python验收装置 | 141 tests OK，exit0 | `main-gates/python.log/.json` |
| 新目标严格核验器 | 17 tests OK，exit0 | `verify-tests-main.log/.json` |

macOS测试不编译Linux-only splice，因此单列Linux实际运行；没有用交叉编译/scratch检查代替Linux测试。

第一版新增取消测试被主代理真实复现失败：`linux-cancel-repro/tests.log`，cargo exit101、0pass/1fail。根因为select借用被pin的future，断言时拥有的wrapper尚未析构。主代理提出明确修复契约，GLM只改代码；词法scope析构后以FakeClock精确500ns检查，修补后26项Linux全绿。红证保留，没有放宽/删除断言。

完整编码workflow也曾因硬编码Cargo路径ENOENT中断；主代理核实实际Cargo在`/opt/homebrew/opt/rustup/bin/cargo`，修订完整run复用已写代码。这个工具路径错误不是产品测试红，已与真实Linux回归红分开留痕。

## 5. 实际产品改动与源码绑定

相对本轮编码前完整快照`before-code-source/`，构建输入仅两个文件变化：

1. `crates/rh-transport/src/splice.rs`：统计关闭时不分配计数Arc、不读统计时钟、不执行统计原子累计；开启时记录实际readiness `Poll::Pending`次数、挂起周期与累计等待时长（包括取消/drop），保留旧EAGAIN等待尝试字段语义；v2输出socket四元组、块大小min/max、epoch锚及duration。原业务计量、限速reservation、取消、半关闭、错误/panic收敛及默认batched策略不改。
2. `crates/rh-transport/examples/g1_splice_bench.rs`：必要统计说明对齐。

关键代码：`splice.rs:217`开关解析、`:289`等待wrapper、`:403`统计开关装配、`:461`结束记录、`:1150`生产默认入口、`:2286`字段回归、`:2518`修补的取消回归、`:2557`默认无统计传输/半关闭回归。原19项Linux测试全部保留，新增7项。

最终源码tree_hash：`85daf0be16285407d42bf5df858d658e41a9af2c23ce858007776493078aac3a`。

- splice.rs SHA256：`612015b0d1f7a982b84fd93d19f61909020f6a080e1a69f93a52b8db78a83acb`
- rh-server SHA256：`2971014f414ef410a3caf965d41c233ea75326ad5e34cdd6f6b870983b760f5a`
- rh-client SHA256：`2a98b3c4b69a7da7f84fb04e0f7e1f4bf9a665efa49e1247f8a238447460a269`
- 旧verify_goal.py SHA256仍为：`a580c0b6449c100e1d827331b8177fa74c5f05fabc38da1cdf7d91eccf3a89e5`

最终当前工作区与快照/build-inputs逐项一致；二进制副本、manifest、实际端点exe身份核对一致。构建/版本、nofile65535及源包传输SHA见`candidate-final/run-index.json`、`build/`。全部既有未提交修改保留；未commit/push，未改AGENTS、NIC、irq、sysctl或SO_RCVBUF/SO_SNDBUF。

**性能采纳结论**：本次仅确认统计准确性和默认去仪表开销；未证实吞吐提升。旧基线精确1952.92/2036.57，本次1963.57/1990.58，窗口不同，不声称单因果百分比收益。

## 6. 主代理诊断与已淘汰假设

234条有效P=1参考诊断及全部原件在`probe-*/`，统计设计、逐条独立核对见`diagnosis-final.md`、`independent-diagnostic-audit-{1,2}.json`及各probe独立审计。另保留一次blocking并行pipe原型的控制连接失败，不计有效吞吐、不覆盖旧raw。

- 三物理段单向中位都≥3200，reference两跳cubic约2004/2075；BBR约1635/1710反而下降。
- 排空64/16KiB、MORE/CORK/copy、pacing两范围、并行pipe、QUICKACK、窗口钳制、线程定位和不同pipe容量均没有双向达2800的支持证据。永久MORE/CORK还引入控制小包约2秒等待，不进入产品。
- 两条互不关联的P=1物理单流在转发节点同时收发，低速段也约2G；P=4补充对照显示部分段可更高。因此支持“同机收发耦合”调查，**不支持**“总带宽固定硬限2G”或“所有软件手段不可能”的结论。

## 7. 与计分隔离的真实生产统计（不是验收成绩）

默认验收完全结束并清理后，主代理使用同一源码/binary身份另跑`candidate-diagnostic/`，仅诊断轮`RH_SPLICE_STATS=1`，高频采样原件`server/client-ss-proc-fast.jsonl`。主代理按所有6个单流的drive四元组、carrier四元组及epoch窗口唯一匹配；`candidate-diagnostic-analysis.json` issues=[]。

| 活动方向 | server读等待占比中位 | server写等待占比中位 | client读等待占比中位 | client写等待占比中位 |
|---|---:|---:|---:|---:|
| 上传 | 4.60% | 77.88% | 70.17% | 0% |
| 下载 | 3.89% | 78.39% | 0.0365% | 88.33% |

真实平均fill量：server约227KiB/块，client上行约127KiB、下行约262KiB；不是持续极小fill。按窗口全部约42–43个高频ss样本分析，server入口Recv-Q中位约3.5–3.8MiB、出口notsent约0.77–0.83MiB；client上行carrier接收队列大多0，下行本地目标接收队列约5.7MiB、carrier出口持续积压。原件与重算：`candidate-tcp-queue-analysis.json`。

**能确认的是**出口背压/就绪等待占主导且发送队列并非总被应用饿空；**不能确认的是**背压的底层根因。等待包含内核及调度时间，future Pending计数不是内核wakeup；不从这些相关性宣称“已找到唯一根因”。

## 8. 清理、缺口与阻断

主代理最后独立复扫三机`ss -ltnup`与本轮路径进程列表：所有测试监听/本轮进程均无残留，仅SSH/DNS/DHCP等原有服务。原件`test{1,2,3}-final-process-port-audit.json`。仅终止本轮记录且exe/starttime身份匹配的进程，未知身份跳过；密钥未进入报告。

### 仍未完成的核心要求

- **双向单流≥2800未达**，上传差836.43Mbps、下载差809.42Mbps；此项不能由正确性、仪表或工作流完成替代。
- 未获得可承诺达标的下一项软件机制。没有跳过任何已承诺强验收：Linux、四E2E、三机、CPU与源绑定全部实际执行，但性能结果仍FAIL。
- 当前边界禁止修改主机/NIC/irq/sysctl及虚拟化网络参数。若继续做虚拟网卡收发/卸载/中断合并或宿主机调度的**可逆A/B**，需要用户单独明确扩大授权；现有结果仅支持进一步定位，不证明这一定能解决。未获授权，不擅改主机，也不拿新环境或多流峰值替代原单流目标。

本报告为主代理已完成工作的真实交付，不标记会话目标完成。
