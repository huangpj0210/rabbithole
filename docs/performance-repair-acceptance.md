# 性能修复验收方案(goal-repair)

状态:**规划稿 r5(2026-10-13,按主代理裁定与独立审查阻断修订为可执行版),本轮未改任何实现代码。**
★基线口径:本方案文件自身**不在** `benchmark/runs/goal-repair/baseline/` 快照内
(快照之后创建);后续完整性核对时,未跟踪集相对 `baseline/git-status.txt` 的
**预期且唯一**差异是 `?? docs/performance-repair-acceptance.md`(41 vs 40 行),
其余任何差异都是意外改动,见 `baseline/supplement-plan-exclusion.md`。
依据:本轮逐行复核的源码、`/tmp/rh-readonly-review.eFSs9d/`(REVIEW.md、repro-results.txt、
evidence.json、src/main.rs)与 `/tmp/rh-splice-audit-ZrHoaE/harness.rs`(可移植控制流复现,
**非** Linux splice(2) 证据)。审计 6 个源文件 SHA-256 已与当前工作区核对一致
(`benchmark/runs/goal-repair/baseline/untracked-and-sources.sha256`)。
本文所有 `path:line` 均为本轮实际读取位置;r2–r5 修订点已逐条回应三轮审查 issue 与
主代理 6 项裁定,门槛与未确认项保留,未删项换批准。

---

## 1. 目标门槛(不降低)

| # | 门槛 | 口径细化 |
|---|---|---|
| G1 | TCP nomux(multiplex=false,relay_mode=splice)单流**上传与下载各** ≥ 3000 Mbps | CPU 口径见 §1.1 |
| G2 | TLS mux 单流**上传与下载各** ≥ 2500 Mbps | |
| G3 | tcp / tls / ws / wss / quic / kcp 最终版本正吞吐 | 逐协议,不允许用任一协议代替 |
| G4 | 1200B UDP 损失 < 1% | 排空修正后的损失口径(见 A8) |
| G5 | 4096 条**真实应用层可用**隧道 | 拓扑见 §1.2 |
| G6 | 与**同轮配对直连基线**保留率 ≥ 80% | 逐协议 × 方向 × 流数报告;单流门槛不得以多流峰值替代 |
| G7 | 每项数字 = ≥ 3 次独立配对测量的**中位数** | 全部样本与失败逐条保存 |

前置:§6a 缺陷回归与 §6b 不变量锁全部通过,才允许进入性能验收;
任何门槛未达时如实报告并继续改进,不得改口径、删测试或复用旧样本。

参考现状(读自 REVIEW.md「Benchmark evidence review」,非本轮实测):
TCP 单流历史最佳 2738 Mbps;默认 splice 单流 2046/2259 Mbps(保留率 68.54%/84.58%);
TLS 单流 1246/1638 Mbps(旧 yamux0 轮);无最新版本 TLS/WS/WSS/KCP 轮。

### 1.1 G1 CPU 口径(实现级定义)

- 采样:A/B 侧各起一个仅依赖 /proc 的采样器(peer.sh server/client 角色,P3 文件集),
  ≥10Hz 记录 `(epoch_µs, utime+stime ticks)`(进程级,**跨线程求和后折算单核百分比**:
  `Δticks/CLK_TCK/窗口秒 × 100%`,100% = 一个满核;"各 < 50% 单核"即此值 < 50)。
- 窗口对齐:drive 在每条吞吐记录写入 iperf 起止 epoch_µs;汇总按时间戳交集对齐,
  **覆盖率 < 95% 的样本判 invalid 并如实列出,不得静默通过**。
- CPU 样本与吞吐样本同轮成对保存,同样取 ≥3 轮中位数。

### 1.2 G5 拓扑(实现级定义)

- 形态:**单条 TLS mux 物理连接上并发 4096 条数据通道**,每通道完成一次应用层
  PING/PONG 交互并保持至测量期末(`concurrent --connections 4096` 的 **verified** 计数,
  见 A12 修复);另跑一轮裸 tcp 多物理连接形态作对照(不替代前者)。
- 前置:三机 `ulimit -n ≥ 65535`——★r5 更正:当前**仅 server 与 client 角色调用
  `raise_nofile 65535`(peer.sh:62/100),drive 角色的 `peer_drive()`(peer.sh:130 起)未调用**,
  本节 r4 的"各角色已 raise_nofile"为事实错误;G5 的 4096 条 socket 由 drive 机发起,
  默认软限(常见 1024)下 EMFILE 会计入 `failed`(loadgen.py:318-323 把连接失败计为
  failed 而非中止),established/verified 塌缩会把环境问题误报为产品缺陷。
  **前置修复 = P3 的 F-Drive-Nofile(§5 P3.0)**:drive 增加 raise_nofile、记录实际软限、
  不足即以环境错误中止本轮;三角色同检。EMFILE 不得计入产品吞吐。
  服务端默认配额允许:benchmark 配置 `[limits]` 并发走默认 `0=不限`
  (benchmark/lib/common.sh:184-185),`data_conn_limit()` 默认 = mux_streams−1024
  (config.rs:166-172;max_mux_streams=0 → MUX_STREAMS_UNLIMITED=1<<24,mux.rs:480)→ 4096 合法。
- 与 G6 关系:G6 流数档(1/4/16)是吞吐维度;G5 是并发隧道计数维度,互不顶替。
- 内存预算:C4 修复后空闲中继初始驻留 = 4096 × 12KiB ≈ 48MiB(本地初始缓冲容量
  < MIN_RECYCLE=16KiB,取 12KiB,§5 P2.10;修复前 4096 × ≥64KiB ≈ ≥256MiB);验收记录两端 RSS。

## 2. 红线(修复全程不可违反;每条在 §6b 有对应锁)

1. quinn 并发流上限 65536:`QUIC_STREAMS_CAP = 1<<16`(quic.rs:24,钳制点 :32-38)。→ 锁 R1
2. 转发读取缓冲最大 64KiB:`CHUNK_CAP`(mux.rs:230)、copy 路径 `MAX_BUF`
   (data.rs:333-334 / forward.rs:244-245)。A4 修复即是恢复该红线。→ 锁 R2/R3
3. 不对 carrier 设置 SO_RCVBUF/SO_SNDBUF(splice 现仅设 TCP_NOTSENT_LOWAT,splice.rs:163-175,
   保持)。→ 锁 R4(静态检查)
4. 不改 `ws.rs` 的 `send_queued`/`from_partially_read` 已修复状态机(ws.rs:92,156,183-265)。→ 锁 R5
5. yamux driver 的 **`open_rx.poll_recv` 的 `Ready(None)`** 不可终止已有流——全部开流请求
   句柄释放时,driver 必须继续服务存量流直至载体/IO 结束(mux.rs:573-595 现语义:
   `Poll::Ready(None) => break` 仅跳出批量收集内循环,注释明示"连接仍需继续服务存量流")。
   ★r5 逐字恢复原红线口径;`poll_next_inbound` 的 `Ready(None)` 是**载体结束**,允许收敛,
   两者不得混淆。→ 锁 R6(rh-transport 驱动集成测试,P2)+ R6b(vendor/API 残留缓冲契约,P1)
6. 不修改 AGENTS.md;不 git commit/push;不覆盖 `benchmark/runs/` 既有历史证据。
7. 全工作区测试由 world.run 统一执行;修复者只跑本模块定向回归并留退出码日志。
8. **wire 兼容红线(新增)**:P1 的任何修复不得引入 upstream yamux 规约之外的
   帧头 flag 组合或新帧类型(混版本部署时对端为旧 vendor;见 C2)。→ 锁 R7

## 3. 三部分文件所有权(两两不相交,可并行)

★r5 按主代理裁定澄清:**排他权仅限制实现源码**;证据目录按下表分配给各部分独占写入,
不得覆盖彼此日志与 `baseline/`(基线旧证据永久保留,不刷新既有 manifest)。
`docs/` 由方案/集成人单写,三个修复部分都不改。

| 部分 | 实现源码独占集 | 证据目录(独占写入) | 实现源码严禁触碰 |
|---|---|---|---|
| **P1 vendor/yamux** | `vendor/yamux/src/**`(含内联 tests;`vendor/yamux/Cargo.toml` 如需) | `benchmark/runs/goal-repair/vendor/` | crates/**、tests/**、根 Cargo.toml、其余 benchmark 源文件 |
| **P2 应用/transport** | `crates/**`(含 rh-core/src/ratelimit.rs、quic.rs 测试)、`tests/e2e-relay-modes.sh`(仅此新增文件);★禁改 `ws.rs` P0 行为与无关既有配置;必要时 `deploy/*.toml` | `benchmark/runs/goal-repair/relay/` | vendor/**、既有 tests/*.sh、其余 benchmark 源文件 |
| **P3 benchmark** | `benchmark/**` 源文件(peer.sh、lib/common.sh、py/*.py、README.md) | `benchmark/runs/goal-repair/harness/` 及 `<label>/` 结果目录 | crates/**、vendor/**、tests/**、docs/** |

冲突检查(实现源码):P1∩P2=∅,P1∩P3=∅,P2∩P3=∅;`tests/` 既有脚本三部分都不改;
三个证据子目录互不相交且都不含 baseline。
P2 与 P1 的耦合关系(r2 澄清):**A4 的修复**(P2 内切片 spare)不依赖 P1;
**A4 的回归取证**依赖"池内可存在超限缓冲"这一前置,受 B3 合入时序影响,顺序规则见 §8
(r5:取证改在 /tmp 未修复源快照上执行或引用既有 9 次失败证据,不依赖共享树时序)。
其余 P2 修复不依赖 P1 API。

## 4. 缺陷清单(先分类:实证 vs 条件性)

分类口径:
- **A 实证(运行时复现)**:已有可执行复现,输出稳定可重复(repro-results.txt 三次一致 / harness 可重跑)。
- **B 源码判定(确定性缺陷,未做独立运行时复现)**:代码路径逻辑必然错误,逐行可证。
- **C 条件性风险(API/环境依赖)**:是否成害取决于使用模式或环境,修复以合同化+回归锁死为目标。

### A 类:已复现的实证缺陷

| # | 缺陷 | 位置 | 证据 | 部分 |
|---|---|---|---|---|
| A1 | 中继父任务取消后子任务 detach:**四个同构位点**——mux 路径 `mux_relay`(spawn+join!)、splice 路径 `splice_relay`(守卫被中和)、**服务端 copy 路径**(t1/t2 spawn 后 `join!`,句柄 move 进 join future,父取消即 detach)、**客户端 copy 路径**(`splice()` 函数 t1.await/t2.await 同形态) | mux:`mux.rs:251-299`;splice:`splice.rs:363-364`;服务端 copy:`data.rs:304-320`;客户端 copy:`forward.rs:219-231` | mux 路径运行时复现 ×3:`io_dropped=false`,abort 后写入的第 9 字节仍被转发(repro-results.txt;复现含 Drop 探针+定点字节,src/main.rs);两处 copy 位点为逐行同构的源码判定(B 类证据,修同一缺陷时一并覆盖) | P2 |
| A2 | splice 守卫被中和:`AbortOnDrop::take_handle` 在 join 前把句柄取走,父取消时 Drop 不再 abort 子泵 | `splice.rs:363-364`(守卫 318-333) | 便携 harness `parent_drop_detaches`:取消后存活子任务 = 2 | P2(Linux 生效,测试可移植) |
| A3 | pump_down 重复扣费:聚合块先按 `c.len()` 扣令牌,push_front 回余量后外层循环对同一余量再扣 | `mux.rs:339-341` 与 `363-365`;回退 `368-370` | double-charge ×3:实际 98304B,扣费 131072B,多扣 32768B | P2 |
| A4 | `read_chunk_from` 上限失效:读入 `pool::get(cap)` 返回缓冲的**全部** spare,池中有 256KiB 缓冲时一次可读 262144B,违反 64KiB 红线 | `mux.rs:203-206` | cap ×3:cap=65536,actual=262144;**定性为合同失效**:生产路径 vendor 发送侧 `pool::get(k)` 的 k ≤ split_send_size(rabbithole 配置 64KiB,mux.rs:503;钳制 stream.rs:677-683),归还侧同为 64KiB 类(frame/io.rs:303-306、mux.rs:321-328),池内 256KiB 条目仅外部播种可造(本轮 grep 全部 `pool::get/put` 调用点证实;与 REVIEW.md:10 口径一致) | P2(修复)+P1(池侧锁,见 B3) |
| A5 | splice 令牌等待不可取消:两方向同时阻塞在 `acquire(n).await` 时,规则取消信号无法到达 | `splice.rs:268-271` | 便携 harness(真实 TokenBucket):取消后双泵仍 pending,无 shutdown | P2(Linux) |
| A6 | splice 排空中取消:在送达回调前直接 `teardown; return`,部分送达字节不计量 | `splice.rs:282-288` 先于回调 `303-310` | 便携 harness:accepted=3,callback=0 | P2(Linux) |
| A7 | PIPE_CAP_MIN 竞争:load+条件 store 非原子最小,并发观测 8KiB/256KiB 可留下 256KiB 当"最小值" | `splice.rs:115-118` | harness `pipe_min_lost_update`:观测 [8192,262144],记录 min=262144 | P2(Linux) |
| A8 | UDP 测量终止污染:到时不排空在途回包,window=32 时所有样本恰"丢"31 | `benchmark/py/loadgen.py:464-501` | REVIEW:32 个近期直连/隧道 UDP 样本全部恰好 31 losses | P3 |
| A9 | 结果元数据来自驱动环境变量而非端点实况 | `benchmark/peer.sh:187-192` | REVIEW 对 364 行/16 文件的解析(QUIC 标签轮 proto=tcp、copy 轮 relay=splice) | P3 |
| A10 | 多实验混合汇总互相覆盖:同 proto 键位后写覆盖先写,connect/udp/http 取首条,标签不进键 | `benchmark/py/summarize.py:310-314,353` | peer-f2 目录 8 个 jsonl 同为 tcp,仅末文件值存活 | P3 |
| A11 | iperf3 失败无痕迹:`2>/dev/null` + `jq … // 0` | `benchmark/peer.sh:200-209` | REVIEW:8 个直连吞吐为 0 且无法归因 | P3 |
| A12 | 并发只计 TCP 握手,不证明端到端应用层可用 | `benchmark/py/loadgen.py:297-375` | REVIEW:4096"established"≠4096 真实隧道 | P3 |

### B 类:源码判定的确定性缺陷

| # | 缺陷 | 位置 | 判定 | 部分 |
|---|---|---|---|---|
| B1 | QuotaIo 使服务端 chunk 快路径不可达:`route_data_stream` 无条件包 `QuotaIo`,`data.rs` 只判外层具体类型;实际链 `QuotaIo<PrependedIo<RhMuxStream>>`,downcast 恒 false | 包装 `control.rs:528`;判定 `data.rs:262`;PrependedIo `control.rs:323-329` | 逐行可证;客户端侧无此包装(session.rs:535→568 直传,forward.rs:115 判定),单侧缺失 | P2 |
| B2 | pool 转子奇偶失配:get 与 put 各自 `ROTOR.fetch_add`,串行 get→use→put 落不同片,池有存货也持续新分配 | `pool.rs:36-40`(shard),43-55(get),59-68(put) | 调度推演直接可得 | P1 |
| B3 | pool 容量上界按条目不按字节:put 只设 `MIN_RECYCLE` 下限,无上限 | `pool.rs:20-22,59-69` | 与 A4 联动 | P1 |
| B4 | yamux 写错误被当就绪吞掉:`is_ready()` 对 `Ready(Err)` 同样为真,随后 `start_send` 覆盖 Poisoned → 半写帧后可继新帧 | `connection.rs:403`;`frame/io.rs:290-299`(置)与 `321-329`(覆盖) | `Poll::is_ready` 不区分 Result | P1 |
| B5 | splice 写错误分支不 shutdown 写侧,与 copy 路径口径分叉 | `splice.rs:290-301,311-314` vs `data.rs:304-319` | 便携 harness 已演示:WRITE_ERROR 计 3B 且无 shutdown | P2(Linux) |
| B6 | 上行计量口径分叉:CountedUserIo 在用户侧 read 时计(同时驱动 metrics/规则 bytes_in/客户端配额,`data.rs:539-565`),splice 在目的端排空后计 | `data.rs:568-594` vs `splice.rs:303-310` | 两处口径并存 | P2(Linux) |
| B7 | chunk 路径不受 relay_mode 开关控制:`RELAY=copy` 只关 splice(benchmark/README.md:93 宣称的对照失效) | `data.rs:262`、`forward.rs:115` vs `data.rs:275`、`forward.rs:137` | 配置语义分叉,逐行可证 | P2 |

### C 类:条件性风险(API/环境依赖,修复=合同化+回归)

| # | 风险 | 位置 | 条件 | 部分 |
|---|---|---|---|---|
| C1 | 小帧切片保留放大:帧体切片交付后,不完整帧按 `missing.max(READ_RESERVE)` 再预留 64KiB | `frame/io.rs:374-384` | 取决于对端发送模式与本端消费速率;REVIEW 明确"not a measured resident-memory result" | P1 |
| C2 | 拆半前 ACK 丢失:`into_halves` 把 flag 只给写半;读半窗口更新不携带 ACK;`MAX_ACK_BACKLOG=256`(vendor lib.rs:55)打满时对端 open_stream 挂起 | `stream.rs:341-369` vs `405-421` | RHTP 数据通道 hello/START_FORWARD 交换(data.rs:232-237)一般先于拆半;突发开流才成害 | P1 |
| C3 | graceful close 唤醒缺失:`Active::close()` 不做流状态清理/唤醒(cleanup 路径有,不得混同) | `connection.rs:382-390` vs `898-912` | rabbithole driver 不调 `poll_close`,影响 vendor API 合同性 | P1 |
| C4 | 空闲 chunk 中继驻留池缓冲:`read_chunk_from` 先取池缓冲再等输入 | `mux.rs:203` | 每条空闲中继钉 ≥64KiB;G5 的 4096 条 ≈ ≥256MiB 驻留(§1.2) | P2 |
| C5 | CountedUserIo 不透传向量写 | `data.rs:596-619` | 仅当未来对用户侧 IO 引入 writev 优化 | P2(补透传,低优先) |

## 5. 修复设计(按部分;r2 修订处标注 ★)

### P1 — vendor/yamux(只改 vendor/yamux/src/**)

1. **B2/B3 pool(★重写设计)**:`pool::get/put` 签名不变。改为**容量类 + 类内轮转 + miss 扫描兜底**:
   - put:按 `capacity()` 归入容量类(如 `≤CLASS` / 超大类单独类,后者带字节计账与上限,
     超过类字节预算即丢弃——落实 B3 的字节口径上界);类内用现有 ROTOR 选片(保留分散)。
   - get(min):选**最小满足 `capacity ≥ min` 的类**,先试 ROTOR 片,**miss 时扫描该类其余片**,
     全部 miss 才 `BytesMut::with_capacity(min.max(CLASS))` 新分配。
   - 闭环论证:put 落哪个片不再影响后续 get 命中(get 有扫描兜底),B2 的串行失配消除;
     get/put 键不同的问题不存在——匹配由"类 + 容量 ≥ min 逐条检查"(现 get 内
     `b.capacity() >= min` 判定,pool.rs:50)完成,而非键哈希。
   - ★性能中性验证(硬性):vendor 内微基准回归——N 线程 × M 次 get/put 循环,
     断言(a)稳态分配次数显著少于循环次数(命中率锁),(b)miss-扫描仅发生在冷启动
     (可用计数器断言);G1/G2 Linux 复测为最终裁定。防止"按容量归类把 64KiB 主力类
     挤回单分片、逆转 Y2 降争用初衷"(pool.rs:9-11)。
2. **B4 写错误/Poisoned**:`connection.rs:403` 区分 `Ready(Ok)`/`Ready(Err)`,Err 立即走 cleanup
   传播;`frame/io.rs` 保证 Poisoned 一旦置位,后续 `poll_ready` 持续 Err、`start_send` 拒绝。
3. **C1 有界保留**:小帧(阈值 ≤ READ_RESERVE)走独立小分配或 copy-out,大帧保留零拷贝切片;
   驻留字节上界回归锁死(1B 帧 × M 个未消费,断言分配量有界)。
4. **C2 拆半前 ACK(★保守择优 + ★r3 对端回归)**:采用"拆半时先发 ACK"且**不发明新
   flag 组合**:`into_halves` 时若写半仍持 `Flag::Ack`,立即经既有命令通道排队一个
   **0-credit 窗口更新帧并附 ACK**(ACK-on-WindowUpdate 是 upstream yamux 既有合法组合,
   入站流本就以该机制回 ACK,stream.rs:144/243;0-credit 经对端
   `increase_send_window_by(0)=checked_add(0)` 恒 Ok,flow_control.rs:180-186,
   不会触发 connection.rs:749-759 的 Terminate 分支)。
   ★否决另一选项"读半窗口更新携带 ACK"(需给 StreamReadHalf 加 flag 状态,扩散面大)。
   ★r3 注意:0-credit WU 是 vendor 自身**从不发出**的帧形态(`next_window_update` 在
   credit < max_receive_window/2 时返回 None,flow_control.rs:52-54),对端接收路径
   (on_frame 对 WU+ACK 的处理 connection.rs:574-586:update_state(Open{acknowledged:true})
   + 唤醒 new_outbound_stream_waker)现无任何测试锁定——故必须双向回归:
   发送侧(R7 header 往返)+ **对端侧新增 `peer_accepts_zero_credit_wu_ack`**
   (构造该帧注入连接,断言流被 acknowledged、阻塞中的 open_stream 被唤醒、连接不
   Terminate/不发 GoAway)。
5. **C3 Closing 唤醒**:进入 Closing 前对驻留流做唤醒;★禁止照搬 `drop_all_streams` 的
   `State::Closed` 推进(connection.rs:898-912)——那会使读半在缓冲有数据时判 EOF
   (stream.rs:397-399 can_read 检查),破坏 `read_after_close` 契约(stream.rs:385)。
   正确语义:仅唤醒 reader/waker,不提前改状态;已缓冲数据必须仍可读。回归见 §6a/C3 双断言。
6. 上述全部配 vendor 内联回归;不改公共 API 形状。

### P2 — 应用/transport(crates/** + tests/e2e-relay-modes.sh)

1. **A1/A2 父取消终止(★r3 扩面:四个位点全修)**:保持**双向各自运行到结束的 join! 语义**,
   禁止把两个方向放进"先完成者胜出"的 `select!`(那会在单向 EOF 时提前 drop 另一方向,
   破坏半关闭传递;现有回归 `mux_relay_echo_and_half_close`,mux.rs:707-747,必须全程保持绿)。
   正确形态:两方向照旧 `tokio::spawn`,但父 future 在 join 期间**不把 JoinHandle 移出守卫**——
   为 `AbortOnDrop` 实现 Future 转发(或提供 `join_both(g1, g2)` 辅助,内部以 `&mut` 等待句柄),
   取消父任务 → join future 被 drop → 守卫 Drop → `h.abort()`(splice.rs:327-333 既有语义)。
   ★修复面(与 §4 A1 四位点一一对应,缺一不可):
   (a) `mux_relay`(mux.rs,引入同款守卫);
   (b) `splice_relay`(splice.rs,恢复守卫语义);
   (c) **服务端 copy 路径** `assign_forward` 的 t1/t2(data.rs:304-320,同 spawn+join! 形态);
   (d) **客户端 copy 路径** `forward.rs::splice()` 的 t1/t2(forward.rs:219-231,t1.await/t2.await
   在父取消时同样丢句柄 detach)。
   ★同步化回归要求:测试用 Drop 探针(对中继 IO 包一层 Drop 计数,同审计 src/main.rs 手法):
   abort 父 → **先等探针翻转**(带 deadline 的轮询)→ 才写探测字节 → 断言窗口期内对端零接收。
   未修复树上的失败模式 = 探针不翻转 + 字节被转发(与 repro-results.txt 一致),非 flaky。
   四位点各配一条回归(a/b 见 §6a 既有行;c/d 新增于 §6a)。
2. **A3 双重扣费**:push_front 回退的余量不重复扣。★r5 断言口径按**计费合同**分方向定义
   (见 3):up 方向(读侧计费合同)总扣费==已 fill 字节;down 方向(送达计费合同)
   总扣费==已送达字节,且拆解时未消费预留已退还;无取消全量运行时两式同归为"==载荷"。
3. **★A5 + 取消安全令牌(新增设计;★r5 按计费合同精确化)**:`TokenBucket::acquire` 是
   take+sleep 循环(rh-core/src/ratelimit.rs:37-60),**已扣令牌在 future 被 drop 后不退还**
   ——直接把裸 `acquire` 放进 select! 会在取消路径丢令牌。P2 在 rh-core 增加**预留式
   取消安全 API**(如 `acquire_reserved(n) -> impl Future<Output=Reservation>`,
   `Reservation` 显式区分**预留/消费/退款**三态),供 splice/mux chunk 的 select 集成路径
   专用;**既有 `acquire` 与既有调用点语义不动**(copy 路径照旧)。
   计费合同(★r5:按实际合同定义,不把读侧计量强改成送达计量):
   - **up 方向 = 读侧计费**(与 copy 路径 CountedUserIo 读侧一致,§5 P2.7):
     fill 完成即预留并**即时消费**,拆解不退款——取消时扣费==已 fill 字节;
   - **down 方向 = 送达计费**:fill 时预留,送达时消费,拆解时**退还尚未消费的预留**
     ——取消时扣费==已送达字节。
   Reservation 单元测试覆盖三态(预留可见、消费不可退、drop 未消费即退)。
4. **A4 读上限**:read into `spare_capacity_mut()[..cap]` 切片(P2 本地,不依赖 P1)。
   ★r5 取证与锁规范(取代 r2/r4 时序表述,详见 §6a/§8.3):复现测试 = 独立二进制串行,
   播种 `for _ in 0..256 { pool::put(BytesMut::with_capacity(256*1024)) }`(镜像审计
   src/main.rs `cap_overrun`;单次 put 因 8 片轮转大概率 miss,且并行测试共享全局池
   会互相污染——pool.rs:26-36),载荷 ≥128KiB;before-fix 证据优先引用既有 9 次失败
   记录(repro-results.txt),新取证在 /tmp 未修复源快照上执行。B3 合入后播种不可复现,
   长期锁 = `read_chunk_from_cap_lock`(mux.rs 内联,私有 helper/注入 oversized 测试缝,
   避免 pool 拒收后恒真)+ P1 侧 `put_oversized_rejected`;原复现与新长期锁都保留。
5. **A6/A7**:排空中途取消先按已送达字节回调再退出;PIPE_CAP_MIN 用 `compare_exchange`
   循环实现真原子最小。
6. **B5**:splice 写错误分支与 copy 路径一致地收尾 shutdown 写侧。
7. **B6(★方向修正 + ★r3 计数点位锁定)**:**不**改 CountedUserIo(其读侧计数同时驱动 metrics、
   规则 bytes_in、`ClientUsage.add_bytes` 客户端配额/flow_cut,data.rs:539-565;改成"送达后计"
   会推迟超限判定与计费时机,属语义变更,本方案否决)。改为把 **splice 上行计量移到搬入管道
   完成时**,且★点位必须是 **fill 完成后、acquire 之前**——与 copy 路径的相对顺序一致
   (CountedUserIo.poll_read 先计数,data.rs:568-594;copy_limited 随后才 acquire,data.rs:345-350;
   splice 现序为 fill→acquire→drain,splice.rs:251-271,故计数插在 fill 与 acquire 之间)。
   若落在 acquire 之后,限速阻塞期间 ClientUsage.add_bytes/flow_cut 的判定窗口将与 copy 不同,
   终值断言测不出。下行维持送达计(对应写侧口径)。
   ★parity 测试的确定性定义——按**字节事件**而非调度注入,三组:
   (i) 全量完成无注入:两路径 bytes_in/bytes_out 相等;
   (ii) 上行写失败注入(对端早期断开):copy 计读侧全量,splice(fill 计)同为全量 → 相等;
   (iii) 下行部分送达后断连:copy 计已写用户字节,splice 计 delivered → 相等;
   ★(iv) 时序窗口断言:极小令牌桶(如 1KiB/s)阻塞传输期间,copy 与 splice 两路径的
   ClientUsage.add_bytes 均已计入已 fill/已读字节(阻塞中立即可见),不等即点位放错。
8. **B1(★守卫保全 + ★r3 恢复 PrependedIo 剥壳细节)**:chunk 剥壳必须按所有权进行,且
   **限额占用不得提前释放**:`QuotaIo::into_parts(self) -> (BoxedIo, QuotaGuard)`,其中
   `QuotaGuard` 复刻 Drop 的 `data_conn_count.fetch_sub`(control.rs:587-593);data.rs 剥壳后
   把 guard 移入 `mux_forward_chunked` 存活至中继结束。对照:现有 splice 剥壳用 `inner_ref`
   借用(data.rs:282-287)外壳存活故无此问题;chunk 剥壳无法借用,故必须显式 guard。
   ★r3 两层拆解(mux 常态路径实际链为 `QuotaIo<PrependedIo<RhMuxStream>>`,PrependedIo 来自
   control.rs:323-329 的同批残留帧前传,漏了它 B1 就修了个空):`into_parts` 拆开 QuotaIo 后,
   若内层是 `PrependedIo`,经其 `into_parts`(P2 在 lib.rs 补)取**剩余前缀 Vec + 内层流**;
   前缀字节是通道→用户方向的先行数据,转为 `Bytes` 塞进 `RhMuxStreamRead.pending`
   (mux.rs:129-162 既有机制,读半最先交付),保证与 PrependedIo 读语义逐字节等价。
   ★覆盖面:route_data_stream 的 4 个 purpose 分支(control.rs:530-560
   TcpPool/UdpChannel/VisitorRelay/RelayProxy)中,**只对 TcpPool→assign_forward 路径**剥壳
   走 chunk;其余三分支保持现状包装与语义不变(其转发仍为 copy 路径,等价性不受影响),
   并在测试中逐分支断言包装仍存在(guard 计数不提前递减)。
   ★回归须含"带残留前缀"的场景:HELLO 与数据同批到达(control.rs:324-326 会包 PrependedIo)
   时 chunk 路径仍被选中且前缀字节先于后续数据送达。
   配套可观测性(先行合入,见 §8 顺序):路径选择 Prometheus 计数器(chunk/splice/copy)。
9. **B7**:`relay_mode == "copy"` 时跳过 chunk 快路径(data.rs:262、forward.rs:115 两处),
   恢复 benchmark README:93 承诺的同二进制 A/B 语义。
10. **C4(★r4 修正:P2 本地初始缓冲,容量必须 < MIN_RECYCLE)**:不改 `read_chunk_from`
    的泛型签名(mux.rs:199,面向任意 AsyncRead+Unpin——"等就绪再取缓冲"缺少独立
    readiness API,不可实现,予以否决)。★r2 的"16KiB 池类"在现有池语义下不成立,否决:
    `pool::get` miss 时新分配 `with_capacity(min.max(CLASS))`,CLASS=64KiB(pool.rs:55),
    传 16KiB 仍得 ≥64KiB 容量;池内也不存在 16KiB 条目(本轮 grep 全部调用点:vendor 发送侧
    stream.rs:683 的 k≤64KiB;mux.rs:351/366 为取出侧,mux.rs:321-328 与 frame/io.rs:303-306
    为归还侧,全为 64KiB 类)。★r3 的"本地 16KiB + 写穿后收回本地复用"同样不可行,否决:
    up 方向冻结 Bytes 经 `RhMuxStreamWrite::write_chunk` 进入 vendor 帧体,写穿后 vendor 在
    frame/io.rs:300-308 **无条件 `try_into_mut` + `pool::put`**,所有权已进 vendor,P2 无法
    拦截收回;且 16KiB=16384 恰 ≥ MIN_RECYCLE=16384(pool.rs:22,60-61),本地初始缓冲经
    vendor 自动归池,正是要避免的污染(r3 的污染分析只考虑了"P2 主动 put",漏了
    "vendor 写穿后自动 put"这条主路径)。
    r4 设计:**初始相本地缓冲容量 < MIN_RECYCLE(取 12KiB=12288)**——读满一次即升级为
    池 64KiB 类,之后照旧走池;初始相冻结 Bytes 写穿后,vendor 的 `pool::put` 因
    `capacity < MIN_RECYCLE` 被拒收(pool.rs:60-61),BytesMut 自然交还分配器,池零扰动。
    本地缓冲不做"收回复用"(初始相每读一次分配一个 12KiB:滴流连接分配速率低,
    批量流首个满读后即入池相,代价可接受)。
    空闲中继驻留 = 本地 12KiB → G5 下 4096×12KiB ≈ 48MiB(§1.2),不依赖 P1 任何改动。
    ★可观测性:read_chunk_from 的池借用经 P2 内部小封装计数(test cfg 原子计数器),
    回归断言"慢启动对端(滴流 1B)下,空闲中继的未决池借用量 == 0
    + 初始相本地缓冲容量 < MIN_RECYCLE(12KiB)"。
11. **C5**:CountedUserIo 补 `poll_write_vectored`/`is_write_vectored` 透传与一次计量。

### P3 — benchmark(只改 benchmark/** 实现源码;证据落 harness/)

0. **★r5 F-Drive-Nofile(前置修复,先于任何验收)**:`peer_drive()` 补 `raise_nofile 65535`
   (现仅 server/client 调用,peer.sh:62/100);三角色启动时把**实际软限**写入各自
   `$PEER_DIR/nofile.json`;drive 侧实际软限 < 所需(并发档 × 每连接 1 fd + 余量,
   4096 档即 ≥ 4608,推荐统一 65535)时**以环境错误中止本轮**(meta 记
   `env_error`,不产出吞吐/并发数字)——EMFILE 属环境问题,不得计入产品吞吐
   (loadgen.py:318-323 现把连接失败计 failed,中止责任在脚本门控)。
1. **A9 元数据可信**:meta 行记录 drive 实际使用的 PROTO/RELAY/MUX(显式必填,无默认值暗中
   生效),新增 `config_evidence` 字段由运行者从 A/B 侧 `identity.jsonl`/配置回填;
   `client_version` 为空即中止本轮。README 写明换协议必须重启两端并重跑。
2. **A11 留痕**:iperf3 原始 JSON 落盘 `runs/goal-repair/<label>/raw/`,stderr 与退出码同行保存;
   失败行如实记 `"error"`,不再 `// 0` 掩盖。
3. **G7 配对中位数 + G1 CPU**:每档(协议×方向×流数)交替 direct/tunnel 成对执行,≥3 次独立
   重复;汇总输出中位数并保留全部样本。A/B 侧 /proc 采样器落 `cpu.jsonl`(§1.1 规格:
   进程 utime+stime、≥10Hz、对齐覆盖率 <95% 判 invalid),drive 记录窗口起止 epoch_µs。
4. **A12 真并发**:concurrent 保持期内每连接一次 PING/PONG 校验,报告 `established` 与
   `verified`;G5 用 verified。
5. **A8 UDP 排空**:deadline 后进入有界排空期(固定 5s)继续收包,`late_recv` 单列;
   损失口径基于排空后。
6. **A10 汇总**:键含 label;同键多样本给中位数/最小/最大;失败样本显式呈现;
   单流门槛从 (path, streams=1) 专属计算。
7. 汇总与门槛判定脚本化:`summarize.py --gates` 输出 §1 表格逐项 pass/fail
   (数据不足=fail,不是 skip)。

## 6a. 缺陷回归矩阵(★r5 两类口径)

- **缺陷回归(红→绿)**:先在未修复状态取证失败(退出码非 0 的断言失败;§8 计数器先行
  步骤保证 B1/B7 回归可编译且行为失败),修复后同命令通过。
- **保护/诊断缝测试(允许修前通过或先加缝)**:锁已正确行为或为新增可观测性开的测试缝
  (计数器、常量锁、cfg(test) 计数器等)不要求修前失败——★r5 明示,不要求所有新增测试
  修前失败,两类不得混淆申报。
- ★r4 表内"文件"列一律显式路径、禁用"同上"承接(防插行错位——r3 曾因插行使
  pump_down 行错承 rh-client 测试;`pump_down` 是 rh-transport mux.rs 模块私有函数,
  rh-client 测试不可及,只能经 `mux_relay` 间接驱动)。
- ★r5 测试放置规则:依赖 `cfg(test)` 私有计数器/私有 helper 的测试放**源文件内联单测**
  (集成测试二进制链接的 library 不带 cfg(test),库内计数器对其不可见);涉及 vendor
  **全局池播种**的旧缺陷复现必须**单独测试二进制/进程串行运行**(默认并行测试共享
  static POOL,pool.rs:26-36,并发归还条目可压住播种条目造成假阴性)。
- 日志按部分落各自证据目录(P1→`runs/goal-repair/vendor/`、P2→`relay/`、P3→`harness/`),
  不覆盖历史。平台:macOS=本机可跑;Linux=必须 Linux。

| 测试 | 文件(P1/P2 内) | 覆盖 | 平台 | ★确定性要求 |
|---|---|---|---|---|
| `pool_roundtrip_reuse` | vendor pool.rs tests | B2 | macOS | 串行 get→use→put×K:断言新分配计数为 0(仅首次) |
| `pool_byte_cap` / `put_oversized_rejected` | vendor pool.rs tests | B3(+A4 池侧锁) | macOS | 超大类字节预算断言;put(256KiB) 后 get 全类 miss |
| `pool_hit_rate_microbench` | vendor pool.rs tests | B2 性能中性 | macOS | N 线程×M 次 get/put:稳态分配次数 << 循环次数 |
| `write_error_poisons_sink` | vendor frame/io.rs tests | B4 帧侧 | macOS | 写错误后 poll_ready 持续 Err、start_send 拒绝 |
| `connection_write_error_propagates` | vendor connection.rs tests | B4 连接侧 | macOS | Err 即 cleanup,不再续帧 |
| `staging_small_frame_bounded` | vendor frame/io.rs tests | C1 | macOS | 1B 帧×M 未消费分配量有界 |
| `split_before_ack_emits_standard_ack` | vendor stream.rs tests | C2 发送侧 | macOS | 拆半触发 0-credit WU+ACK;header::encode/decode 往返合法(R7) |
| `peer_accepts_zero_credit_wu_ack` | vendor connection.rs tests | C2 ★r3 对端侧 | macOS | 注入 0-credit WU+ACK:流 acknowledged、open_stream 唤醒、不 Terminate(§5 P1.4) |
| `closing_wakes_parked_streams` | vendor closing.rs tests | C3 | macOS | ★双断言:唤醒发生 **且** 已缓冲数据仍可读(不提前 Closed) |
| `mux_relay_parent_cancel_kills_children` | crates/rh-transport/tests/goal_repair_mux.rs | A1(a) | macOS | Drop 探针先翻转再写探测字节(§5 P2.1) |
| `server_copy_parent_cancel_kills_children` | crates/rh-server data.rs tests | A1(c) | macOS | 同上探针协议,作用于 assign_forward copy 路径 |
| `client_copy_parent_cancel_kills_children` | crates/rh-client forward.rs tests | A1(d) | macOS | 同上探针协议,作用于 forward.rs::splice() |
| `pump_down_no_double_charge` | crates/rh-transport/tests/goal_repair_mux.rs | A3 | macOS | 无取消:总扣费==载荷;★r5 取消场景按计费合同:up==已 fill、down==已送达+未消费预留已退(§5 P2.2/3) |
| `token_reservation_cancel_safe` | crates/rh-core ratelimit.rs tests | A5 前置 | macOS | Reservation 三态:预留可见、消费不可退、drop 未消费即退(§5 P2.3) |
| `read_chunk_from_cap_repro` | ★r5 独立测试二进制(串行运行,`--test-threads=1` 或专用 bin) | A4 复现(红→绿) | macOS | 播种 0..256×256KiB(§5 P2.4);载荷 ≥128KiB(跨过 C4 初始相);在 /tmp 未修复源快照上取证或引用既有 9 次失败证据(repro-results.txt),不依赖共享树并行改动时序(§8.3) |
| `read_chunk_from_cap_lock` | crates/rh-transport/src/mux.rs 内联单测 | A4 长期锁(诊断缝) | macOS | ★r5 经私有 helper/注入 oversized 缓冲的测试缝触发(B3 修复后 pool 拒收 oversized,直接播种会恒真);断言每块 ≤ CHUNK_CAP |
| `idle_chunk_relay_releases_buffer` | crates/rh-transport/src/mux.rs 内联单测 | C4 | macOS | ★r5 移入内联(cfg(test) 池借用计数器对集成测试不可见):慢启动对端下未决池借用 == 0(初始相本地缓冲 12KiB < MIN_RECYCLE,不经池;§5 P2.10) |
| `counted_user_io_forwards_vectored` | crates/rh-server data.rs tests | C5 | macOS | 向量写透传 + 一次计量 |
| `splice_parent_cancel_*` / `splice_token_cancel` / `splice_partial_meter` / `pipe_cap_min_atomic` | crates/rh-transport/src/splice.rs tests | A2/A5/A6/A7 | Linux | 同 §5 P2.1 Drop 探针同步;token_cancel 断言退款 |
| `splice_write_error_shutdown_parity` | crates/rh-transport/src/splice.rs tests | B5 | Linux | 写错误后写侧 shutdown 与 copy 收尾一致 |
| `upload_metering_parity` | crates/rh-transport/src/splice.rs tests | B6 | Linux | ★四组:三组字节事件终值相等(§5 P2.7)+ (iv) 小令牌桶阻塞期间 add_bytes 立即可见 |
| `server_chunk_path_engages` | crates/rh-server data.rs tests | B1 | macOS(计数器) | ★依赖计数器先行(§8);断言 TcpPool 分支计数=chunk;★另断言 guard 存活期 data_conn_count 不提前递减,Udp/Visitor/RelayProxy 分支包装不变;★含 PrependedIo 残留场景(§5 P2.8) |
| `relay_mode_copy_disables_chunk` | crates/rh-server data.rs tests | B7 | macOS(计数器) | ★r3 分格取证:(mux,默认)→chunk 在树 A 失败;(mux,copy)→copy 在树 B(B1 已修、B7 未修)失败——两格取证树不同,见 §8 |
| loadgen/summarize 自检 | benchmark/py/(fixtures 驱动) | A8/A10/A12 | macOS(纯逻辑)+Linux(现场) | drain 后 loss、verified 并发、label 键、失败留痕 |
| `tests/e2e-relay-modes.sh`(新增,P2) | tests/(新文件) | B1/B7 端到端 | Linux | tcp+tls × {默认,copy}:转发功能 + 路径计数器断言;补 §7.2 命令集不含 copy 模式的缺口 |

既有回归 `mux_relay_echo_and_half_close`(mux.rs:707-747)与
`crates/rh-transport/tests/ws_mux_repro.rs` 全程保持绿(半关闭/ws 载体行为锁)。

## 6b. 不变量锁(修复前后都必须通过;它们锁红线,不锁缺陷)

| 锁 | 测试/检查 | 覆盖红线 | 平台 |
|---|---|---|---|
| R1 | quic.rs 单测:`QUIC_STREAMS_CAP == 65536` 且 transport_config 钳制路径(0→CAP、n→min(n,CAP)) | 红线 1 | macOS |
| R2 | mux.rs 单测:`CHUNK_CAP == 64*1024`(常量断言);A4 回归的行为面 | 红线 2 | macOS |
| R3 | data.rs/forward.rs 单测:copy 路径 MAX_BUF 常量 == 64*1024(重构为命名 pub(crate) 常量+断言,不改行为) | 红线 2 | macOS |
| R4 | ★静态检查命令(§7.5):`grep -rn "SO_RCVBUF\|SO_SNDBUF" crates/ vendor/` 必须无命中,输出与退出码留档(`grep` 无命中退出码 1 = 通过) | 红线 3 | 任意 |
| R5 | ws.rs 行为锁:★r3 必须新增针对性测试——既有 mod tests(ws.rs:287-333,本轮实读)仅覆盖握手/流传输/404 路径,**不覆盖** `send_queued` 状态机与 `from_partially_read` 升级路径,无捷径可走。新增:(1) 背压/排队下 send_queued 状态转移与 flush 语义;(2) 升级时带部分读前缀(from_partially_read,服务端 ws.rs:92 / 客户端 :156)前缀字节不丢不重。改动 ws.rs 前后同跑 | 红线 4 | macOS |
| R6 | ★r5 重定义(rh-transport 驱动集成测试,P2 文件集):经 `YamuxMultiplexer` 开流取得活跃流 → 释放 Multiplexer 全部句柄(open_tx 尽,`open_rx.poll_recv` 转 `Ready(None)`)→ 断言活跃流**仍可双向传数据**(红线 5 的规范面在 mux.rs:573-595 driver 循环,vendor 测试约束不到它) | 红线 5 | macOS |
| R6b | ★r5 新增(vendor/API 契约,P1):载体关闭后残留缓冲仍可读(read_after_close,lib.rs:94 默认 true;stream.rs:385/394 先弹缓冲后查状态)——与 R6 分开测试,不得混淆 | 红线 5 关联契约 | macOS |
| R7 | vendor header 往返测试:本方案新增的全部外发帧 flag 组合 ⊆ upstream 解码接受集 | 红线 8 | macOS |

说明:6b 类测试**不要求**"修复前失败"(它们锁的是不可回退的不变量);任何一部分的改动
导致 6b 变红即视为该部分修复不合规。

## 7. Linux 检查清单(验收机,三台同网段 Linux)

按 README.md:145-157 与 benchmark/README.md:56-81 的原命令,不得替换为等价物:

1. `cargo fmt --all && cargo clippy --workspace --all-targets && cargo test --workspace`
   (含 Linux-only splice 测试;退出码留档)
2. `bash tests/e2e.sh target/release`、`bash tests/e2e-resilience.sh target/release`、
   `bash tests/e2e-transports.sh target/release`(12 项传输变体)
3. ★`bash tests/e2e-relay-modes.sh target/release`(P2 新增:tcp/tls × 默认/copy,
   功能+路径计数器断言)
4. `bash tests/load.sh target/release 600`(可选冒烟)
5. ★静态红线检查(真实退出码,禁止管道掩盖):
   `grep -rn "SO_RCVBUF\|SO_SNDBUF" crates/ vendor/; echo "exit=$?"`
   —— 期望 `exit=1`(无命中);`exit=0` 即违反红线 3。
6. 三机基准:每协议 `PROTO=<p> peer.sh server/client/drive` 全套
   (tcp/tls/ws/wss/quic/kcp × 门槛所需 MUX 开关),外加 `RELAY=copy` 对照轮;
   G5 专项(concurrent 4096 verified + RSS 记录);`RH_NOTSENT_LOWAT` 矩阵如需归因
7. `python3 py/summarize.py runs/goal-repair --gates` 产出逐项判定
8. 全部原始样本/stderr/退出码/cpu.jsonl 留在 `benchmark/runs/goal-repair/<label>/`

## 8. 执行顺序与汇报(★含取证时序)

1. P1、P2、P3 并行开工(实现源码集不相交;证据各落 vendor/、relay/、harness/ 子目录)。
2. ★计数器先行(P2 内第一步,无行为变更):路径选择计数器 + R1-R3 常量锁 + R6 驱动锁
   (**均属 P2 文件集**——R6 是 rh-transport 驱动集成测试;R6b/R7 归 P1 自行安排,不阻塞 P2)
   先合入并全绿;随后 B1/B7 回归在未修复树上即以**断言失败**形式取证(而非编译失败)。
   ★r3 分格取证树(B7 两格证据不可来自同一棵树):
   - **树 A = 仅计数器合入**(B1/B7 均未修):`relay_mode_copy_disables_chunk` 的
     (mux,默认)→chunk 格失败(服务端实际走 copy,因 QuotaIo 包装 chunk 不可达,§4 B1)
     ——此树上 (mux,copy) 格**恰好通过**(走的本就是 copy),不产生失败记录,不得作为该格证据;
   - **树 B = B1 已修、B7 未修**:(mux,copy)→copy 格失败(chunk 路径不受 relay_mode 控制,
     实际走 chunk)——该格的失败取证只能来自树 B;
   两格各自留下"失败→修复→通过"的完整记录后 B7 方可判定完成。
3. ★r5 A4 取证规程(取代 r4 时序表述):全局池播种类旧缺陷(A1/A3/A4)的 before-fix 失败
   证据**优先引用既有独立 main 的 9 次失败记录**(repro-results.txt,3 场景 × 3 次);
   需新取证时,在 **/tmp 保存的未修复源快照**上以独立二进制串行执行(§6a 放置规则),
   不以生产共享树的并行改动时序为依赖。B3(put 拒收 oversized)合入后,池侧播种不可复现,
   A4 的长期锁改由 `read_chunk_from_cap_lock`(私有 helper/注入 oversized 测试缝,
   §6a)承担,`put_oversized_rejected` 承接池侧合同——原复现测试与新长期锁**都保留**。
   ★r4 的 A4×C4 载荷规则(≥128KiB 跨过初始相)继续适用于任何仍在池相取证的场合。
4. 其余缺陷:先在未修复状态(共享树或 /tmp 快照;全局池播种类见 §8.3)记录 §6a 对应
   回归的失败输出,再修复使其通过(同命令)。
5. 正确性回归全绿(world.run 全工作区 + §7.1-7.5)后,才跑 §7.6 性能验收。
6. 结果逐协议×方向×流数报告;未达门槛的项如实列出差距与下一步,禁止以多流、单侧方向、
   旧轮数据或非中位数顶替。
7. 本方案是计划;实施轮不得宣称"绝对无缺陷",最终以主代理独立复核为准。
8. ★r5 设计约束条款:本方案是**设计约束而非不可更改伪代码**——实现者若发现某具体设计
   (如 C2 的拆半同步排队/共享握手状态、C3 的 Closing 唤醒与 buffered-read 语义等)不能
   正确实现,应以**更小且经测试证明**的实现替代,并在本部分证据目录记录偏差与验证,
   不为遵照错误设计牺牲正确性。
9. ★r5 基线条款:baseline 旧证据与既有 manifest 永久保留,任何轮次不得刷新或覆盖;
   后续轮次的新快照另立目录。

## 9. 未确认项(如实保留,不以删项换批准)

- splice(2) 全部 A 类证据来自可移植控制流 harness,真实 Linux syscall 行为(如写错误
  种类对对向泵的影响,B5 的"是否挂死")待 Linux 回归与现场确认。
- 生产路径池内 256KiB 条目非现状常态(A4 定性为合同失效),但 B3 修复仍为红线强制所必需。
- `RH_NOTSENT_LOWAT` 矩阵与 QUIC GSO/拥塞控制的归因(REVIEW「Benchmark evidence review」
  末两条)仍未闭合,列入 §7.6 可选归因轮,不作为门槛豁免理由。
