# G1 第二轮方案：先分离 TCP 段限制，再决定修改

## 当前审计结论

用户目标不变：TCP 非复用 `multiplex=false`、`relay_mode=splice` 单流上传与下载各 ≥3000 Mbps；各方向至少 3 次独立配对中位，CPU 两端每方向 <50% 单核且覆盖 ≥95%。主代理分析方案，GLM-5.3 写实现，真实三机验收决定成败。

2026-10-15 主代理重跑 `python3 benchmark/g1_tcp.py verify --run-dir benchmark/runs/g1-tcp-20261014/final --repo . --json --require-e2e`：`evidence_valid=true,g1_pass=false,regressions_pass=true,upload_mbps=1952.5,download_mbps=2036.0`。最终 Linux workspace/release、四套 E2E 原件齐全。上轮子代理复核在最终验收之前形成的“四套 E2E 未跑”现在已过时，不作为当前缺陷。仍有两项轻微旧记录偏差：summary-smallpacket 无文件 SHA；final-change 中“ss 缺失”过时。旧历史记录不重写，本文件补充纠正。

上轮同窗口 baseline-retry 1931/2039 vs 默认 batched 1952.5/2036 未证实吞吐收益。公平性/取消改动及回归保留，不再重复控制流候选。最终 direct 中位 3077/3002.5，不能把较早 direct<3000 当成不再调查的硬停止条件，亦不能把测到 direct>3000 当成稳定硬件保证。

报告中的“RTT 62–78ms”其实来自 `run-index.env.*.rtt_s`，为本地编排机到远端命令通道的往返时间，不是三机 TCP 数据路径 RTT。恢复的 ss 样本 minrtt≈0.16–0.27ms、活动 rtt≈1–3ms，二者不可混用。

## 已由主代理筛选的真实单流证据

`round-4/diagnostics/g1-tcp-nomux-diag/server-diag-ss-proc-recovered.log`：

- :468–473：入口 `172.16.1.6:20002 ← 172.16.1.18:45750` Recv-Q=4616712；同 tick 大流量 carrier `172.16.1.6:17100 → 172.16.1.17:46878` Send-Q=1227584、notsent=901784、cwnd=1461、rtt=1.179ms、非 app_limited。
- :994–1009：入口 `:20002 ← .18:42154` Recv-Q=3728672；carrier `:17100 → .17:50356` Send-Q=2194960、notsent=1103624、cwnd=1619、rtt=3.138ms、非 app_limited。
- :1045–1069：下行 carrier 接收积压 3835472，server→drive 出口 Send-Q=1735840、notsent=697376。client 相同 carrier 的 :1565–1574 表明 rwnd_limited=35.2%，本地 target→client Recv-Q=5512144。

四元组验证：主代理只读远端 `/tmp/rh-g1-tcp-measure-20261015-030512/rounds/g1-tcp-nomux-diag/drive/results-*.jsonl` 与对应 raw，上述 drive 端口分别为 **sample2/upload、sample3/upload、sample3/download 的单流 tunnel**，并非4/16流。吞吐分别1875、1895、2491 Mbps。旧诊断每15秒经远程串行采样，tick 时间写成字面量 `$(date -Is)`，两端不同步；这是稀疏观测，仅足以反驳“所有出口总是空”的笼统说法，不能算时间占比或根因证明。

## 本轮最小可证伪路线

1. 保存新基线（=上轮最终当前树），不重新执行上轮全部工作。新目录 `benchmark/runs/g1-tcp-20261015-r2/`，旧证据只读。
2. 由 GLM-5.3 编写小型专项观测器 `benchmark/g1_network.py` 与定向 Python tests；复用现有 snapshot、Termark、PID身份治理和 `G1RoundRunner`，不改 `verify_goal.py`、`source_inventory.py`、`peer.sh`、旧 raw。
3. **先测物理段，不先改生产代码**：test3↔test2（原direct）、test3↔test1（入口段）、test1↔test2（carrier段），固定 iperf3 3.17.1、单流、10秒、≥3重复双向，有限串行交替。仅绑定独立空闲端口、只清理自有进程。每次同时保留 JSON、stderr、exit 与真实起止时间。三段之间“最低速段”不是绝对上限证明，必须结合 paired tunnel。
4. 在**同一份当前构建**上跑独立诊断轮，仅 single-flow，每方向 ≥3 direct/tunnel 配对（复用CPU/raw/端点证据）；100–200ms 远端本地循环记录时间、`ss -tinm`、两端真实 PID/TID CPU与上下文切换，收集 active tuple、TCP_INFO busy/rwnd_limited/sndbuf_limited/notsent、pipe实际容量；泵统计开启仅在诊断轮，JSON日志必须回收，不能用0占位。采样不要每tick另开SSH。采样扰动样本不计最终门槛。
5. 选定的下一项软件假设：**socket 发送节律/拥塞控制与两跳链路耦合**。主代理已只读确认两个Linux节点 `tcp_available_congestion_control=tcp_allowed_congestion_control=reno cubic bbr`，不用安装模块或写sysctl。允许在自有新诊断socket上设置 `TCP_CONGESTION` 作 cubic/bbr 单变量对照（不改系统默认、不改buffer、不改测试生成端）。先用 reference relay / 诊断适配演示这个变量是否移动两跳吞吐，记录 getsockopt 实际值；不因 bbr“听起来快”直接改产品默认。
6. 若 carrier 阶段或 socket对照有稳定支持，GLM-5.3 再最小修改裸TCP数据socket策略，限定splice路径，保留混部署wire/认证/计量/限速/取消/半关闭/回落。候选采用要有同源AB与失败全保留，最终默认无需实验env才算G1。若无支持，不添加无用CC旋钮；依据真实队列/计数决定下一机制，不盲扫LOWAT。
7. `SPLICE_F_MORE` 是次级候选，必须有实际小块/flush密度支持才试。Linux5.15 `pipe_to_sendpage` 内部已对非最后页使用 MSG_SENDPAGE_NOTLAST，所以“每页都立即推送”不成立。只在明确后续待发送时 MORE，不能永久cork/引入无限等待；低负载p50/p99、背压/EOF必须覆盖。
8. 对新发现的 lost-wakeup、零进展、公平性等**独立正确性问题**先复现再修，不能把“改guard时序”自动归因成 50%吞吐收益；与CC实验分开，禁止多机制混改。

## 红线与完成证据

全部既有未提交修改保留；不git提交/推送，不改AGENTS，不调系统/NIC/irq/sysctl，不设置SO_RCVBUF/SO_SNDBUF，不改wire或认证，不安装工具。只修改新的专项观测器、splice相关实现和必要定向测试。前轮fmt波及其它文件不作新增性能改动，不继续全仓格式化。使用明确 `/Users/yii/.cargo/bin/cargo` 或在命令子进程注入PATH，避免前轮“cargo不存在”的空转。

工作流先完成有边界的诊断与独立核对，在真实结果处交回主代理决定后续拓扑，不预写无证据的四轮修改。生产代码由GLM-5.3执行。最终必须真实执行fmt/clippy/workspace/Python、Linux splice与四套E2E、同构建三机默认单流10秒×至少3配对，原G1核验+当前树hash独立复核；诊断成功、局部5Gbps或工作流结束都不算目标完成。缺项或吞吐<3000一律FAIL继续，不改门槛。
