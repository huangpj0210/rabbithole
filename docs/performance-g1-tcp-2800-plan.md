# G1 TCP 非复用单流 ≥2800 Mbps：完整工作流修改方案

## 1. 用户要求与完成标准

最新明确要求：**主代理亲自完成诊断、方案、独立复核与真实三机验收；GLM-5.3 仅按已确定方案编写产品代码和回归测试**。主代理在诊断确认修改方向后启动完整编码 workflow，不把诊断、方向选择、代码复核、结果解释或验收判断交给GLM。吞吐目标为TCP非复用单流上传、下载各 **≥2800 Mbps**；禁止单独执行工作流片段。

本文件替代后续执行中的 3000 Mbps 目标，但不修改历史方案、报告、原始数据或原有 `verify_goal.py` 的 3000 门槛。为新目标新增明确标为 **G1-2800** 的核验入口，保留原 G1-3000 判定并列展示。只改变用户明确调整的吞吐数值，其余合同不变：TCP、multiplex=false、relay_mode=splice、单流、每方向10秒×至少3次独立direct/tunnel配对中位；双端进程CPU每方向<50%单核，覆盖≥95%；最终默认配置，不靠临时实验环境开关；源码、构建、端点身份和 raw 绑定；Linux测试、四套E2E实际通过。

## 2. 已核实的当前状态

- 历史有效原始验收为2077/2156 Mbps，未达2800或3000。
- 上轮完整GLM-5.3工作流已实测最终当前树：1952.5/2036.0 Mbps。主代理已重新调用 `g1_tcp.build_verdict(final,'.',require_e2e=True)`，证据有效、正确性通过、吞吐仍FAIL。当前树 `535480dba12a0279…`，四套E2E真实结果73/0、19/0、12/0、21/0。
- 上轮快试/有界批次改变没有证实吞吐收益；只保留其已有公平性、取消行为修补，不再重复该控制流优化。
- 最终同轮direct中位3077/3002.5 Mbps；当前候选到2800尚需约43.4%/37.5%，降目标并不意味着已达成，也不能由某次direct低于3000推断环境硬上限。
- 恢复的单流ss证据已由主代理按drive raw四元组确认：上行sample2入口Recv-Q 4616712、carrier notsent 901784；sample3入口Recv-Q 3728672、carrier notsent 1103624。下行sample3 carrier接收积压3835472、出口notsent697376；client carrier rwnd_limited35.2%、本地目标接收积压5512144。详见 `docs/performance-g1-tcp-r2-plan.md` 的文件行号。这是稀疏观测，尚不能计算时间占比或确定根因，但反驳“发送队列总被用户态饿空”的笼统假设。
- 数据路径TCP RTT为亚毫秒至数毫秒；上轮报告62–78ms其实是编排命令往返，不是三机数据链路RTT。
- 两Linux端只读核验已可用且允许 `reno cubic bbr`，因此可对**本次拥有的单个socket**做 `TCP_CONGESTION` 对照，不用加载模块或修改sysctl。主机/NIC配置保持不动。

## 3. 主代理已完成的诊断与当前修改方案

主代理实测和独立raw复核见 `benchmark/runs/g1-tcp-2800-main/diagnosis-final.md`。234条有效单流诊断包含三物理段、cubic/bbr、排空批量、MORE/CORK/copy、两独立物理流同时收发、pacing两范围、非阻塞有界并行pipe、QUICKACK、接收窗口、线程定位、管道深度；失败原型原件另存未删除。这些不是产品验收。

### 已淘汰的性能策略

三物理段单向均有2800余量，但同机独立收发也降至约2G，不能再把现象简单归因到Tokio await/fill控制流。BBR、pacing、MORE/CORK、减小drain、并行pipe、窗口限制均无双向目标收益或更差；线程定位和大管道也未接近目标。**不把这些策略加入产品，不让GLM自行重选方向，也不把这些反例升级成环境硬上限。**

### 唯一已确定的代码任务

完整规格 `benchmark/runs/g1-tcp-2800-main/coding-spec.md`：

1. 关闭统计时，生产splice路径不创建统计Arc、不执行原子累计/Instant；保留业务计量与限速不动。
2. 开启诊断时，在readiness future实际返回Poll::Pending处记录poll次数、至少挂起一次的等待周期、首次Pending至Ready/取消/drop的累计等待时间。原pending_waits尝试字段语义保留，不冒称内核wakeups。
3. 输出v2 JSON包含mode、方向、socket四元组、时间及真实块大小分布，支持主代理按iperf单流窗口关联。
4. 新增立即Ready、单/多次Pending、取消/drop、默认禁用以及实际payload的确定性回归。GLM仅写指定代码/测试，主代理独立复核。

**不承诺该修改本身能带来37–43%吞吐收益。**它解决的是现有观测口径错误和默认热路径持续仪表开销，之后由主代理在真实rh默认与诊断隔离轮测量；不以参考relay或统计装置完成代替G1。

主代理编写并运行的新目标核验器 `benchmark/runs/g1-tcp-2800-main/tools/verify_2800.py` 已通过10条对抗单测。当前产品精确raw中位1952.92/2036.57，仍未达2800。后续Linux和最终三机验收必须重新执行，结果不足继续明确标FAIL。

## 4. 文件边界与完整执行

- 主代理编写并维护新目标核验与诊断装置（位于本轮证据目录工具区），复用 `g1_tcp.py` / `closure_c84.py` / 原始证据解析与CPU计算，不修改旧3000判定、不用全局monkeypatch。GLM仅实现主代理给出的产品代码与定向回归，不负责装置设计或诊断。
- 新证据根 `benchmark/runs/g1-tcp-2800-main/`，主代理保存当前所有未提交修改和完整可构建快照。旧证据只读、新证据只增。
- 产品主要修改 `crates/rh-transport/src/splice.rs`、相关定向tests/example；必要调用边界最小改动须有主代理确认的证据，不碰无关文件。
- 主代理先独立完成真实三机分段及CC诊断，确定具体修改契约；随后修订并运行单个完整**编码workflow**，GLM只修改授权实现/测试，不担任诊断者、评审者或验收者。不运行EvalWorkflowSnippet，不在运行中留下待补的脚本空位。
- 编码前后源码复核、完整测试命令、最终三机验收、G1-2800判定与报告均由主代理亲自执行和复算。GLM仅可运行所写定向回归。使用绝对cargo路径或显式子进程PATH，避免缺cargo的假失败。所有长命令有deadline及自有进程组清理。

## 5. G1-2800机械判定与交付审计

新核验不相信手填PASS：读取旧解析器的完整EvidenceDir、collect_pairs与cpu_window，按新阈值2800直接计算双向中位数；目录/源/二进制/raw/失败样本、独立配对、CPU覆盖与数值、最终E2E原件全部fail-closed。旧3000的g1_pass为false不是新2800必失败的理由；反过来只满足2800也不报告旧3000通过。

最终必须再次运行：fmt检查、clippy全工作区、workspace tests、Python全套；真实Linux workspace/release、四套E2E、TCP nomux默认配置双向10秒至少3配对且两个独立窗口；新2800核验和独立原件审核。所有失败全留，不能筛样本或拼构建。

交付 `docs/performance-g1-tcp-2800-report.md`，明确PASS/FAIL、全部单流样本和两向中位、各方向CPU/覆盖、默认与实验开关、所采纳机制的证据、源/binary hash、回归与E2E退出码及日志指针。报告完成、workflow完成或局部改善都不代替双向≥2800。

## 6. 不可违反的边界

保留全部已有未提交修改，不git commit/push/reset/checkout/clean，不改AGENTS、不覆写历史raw，不改系统/NIC/irq/sysctl或SO_RCVBUF/SO_SNDBUF，不安装工具，不改变wire/认证/计量/配额/取消/半关闭。凭据仅0600文件且不进入报告，远端仅清理由本轮创建并身份匹配的进程。系统性部署改变若成为唯一出路，报告具体证据与所需授权，不擅自操作。
