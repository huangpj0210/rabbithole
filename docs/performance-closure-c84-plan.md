# performance-closure-c84 一页执行设计(2026-10-14)

基线 `c84f63174cd2ebafd994c4fafacd5beba4a0240b`(分支 fix/performance-closure-c84f631,快照时 clean)。补修前快照:`benchmark/runs/performance-closure-c84/baseline-local/`(commit.txt + kcp.rs/api.rs/state.rs 的 SHA256;kcp.rs=`16b72155…` 与 kcp-boundary 终稿一致;只增不覆盖)。门槛 G1–G7 与红线沿用既定口径,不降不减;无新 KCP/yamux wire 帧、不伪造 FIN;不动 ws.rs/mux.rs/quic.rs/splice.rs/tcp.rs 状态机、认证/计量/配额/取消安全。行号=c84f631 实读。

## A KCP poll_shutdown 只关本地写侧(修 F5)
- 现状:closing+队列空+`wait_snd()==0` → `break`(kcp.rs:849-854),收尾置 `closed`(869-878)令 poll_read 立即 EOF(181-182)=成功关写杀读侧。公开调用面:`YamuxCarrier::poll_close`→carrier `poll_shutdown`(mux.rs:668-673)。
- 改:Guarded(kcp.rs:87-106)增 `write_closed`;关闭完成只置它+唤醒 flush/shutdown 等待者,会话循环**继续**(入包→read_buf、窗口 ACK、dead_link/idle/句柄归零照常);`closed` 仅会话真终止置位。poll_write 在 write_closed 报 Err(沿 closing 分支 203-205),poll_flush 按 flushed_seq Ready(232-249),error 可见(257-259)。对端不 ACK 由重传 dead_link(791-793)以 Err 浮出,不加短超时。
- 回归(kcp.rs 内联;红证已有 `main-independent/halfclose-exact-baseline-red.log`,断言 `c84_review_tests.rs::shutdown_write_must_preserve_delayed_reply_read` 原样移植):真实双端长度协议「写 7B→shutdown Ready→对端收全→25ms 后回 8B→本端仍读到」;`a_flush_ready_and_error_visible`;`a_drop_after_shutdown_releases_tasks`(句柄归零→session/inbound/drain 退、socket 释放)。

## B unread read_buf 不刷新 idle(修 F6)
- 现状:has_pending_delivery 只要 read_buf 非空或 kcp 有存货就刷新 last_activity(kcp.rs:855-863),无人消费也永不回收;SESSION_IDLE_TIMEOUT=90s(kcp.rs:46)。
- 改:定义进展=①无待交付时任何入包(现 last_activity 语义不变);②应用消费:Guarded 增 `consume_seq`,poll_read 消费 n>0 时自增(146-188 处),会话轮内 consume_seq 变化才刷新。待交付存在时入包(纯窗口探测/重传)**不再**刷新 → 活跃慢消费者(≤90s 内有读)受保护,无人消费且无消费进展 90s 回收,退出走现有 shutdown/日志可观察终态;MAX_READ_BUF 精确界(795-824)不动。可测性:run_session 增 `idle_timeout` 参数(默认 90s,测试传 ~100-300ms),不睡 90s、不改机器时钟、不改生产默认。
- 回归(kcp.rs 内联;红证 `main-independent/c84-risk-red.log` 用例 2 同行为断言):`b_unconsumed_stall_reclaims`(read_buf 预置数据+零消费+无入包→有界退出+shutdown 可观察);`b_slow_consumer_survives`(周期 1B 读跨 3×timeout 存活);`b_idle_no_traffic_recycles_unchanged`;`b_read_buf_bound_exact_under_stall`。

## C PunchOutput WouldBlock 有界队列(修 F9)
- 现状:`try_send_to` WouldBlock→`Ok(buf.len())` 假成功(kcp.rs:1412-1424),无队列/重试/错误桥,与 connect 的 UdpBacklog(305-489)+drain_once(505-542)+spawn_udp_backlog_drain(555-612)两套机制。
- 改:PunchOutput 持 `Arc<UdpBacklog>` 并 attach_session 错误桥(340-367,Shared 来自 new_io);drain 目的址泛化为枚举 `Connected/Peer(addr)/Punch(Arc<PunchPeer>)`——**同一 backlog/drain 代码**供 connect 与 punch 复用,不复制新状态机。语义:Rotating 阶段每个数据报发往弹出时刻 `peer.current()` 的候选,Learned 后锁定(1360-1404);pop→send 一次、WouldBlock 回插队头(477-488)保证每 pending 报至多送达一次;drain 等 writable/notify/退避(594-608)无忙循环;生产者 Drop→Weak 退出(678-684)无永生引用;PunchCancel RAII(1433-1446)不动。
- 回归(kcp.rs 内联):`c_punch_wouldblock_enqueues_not_fake_ok`(满/阻塞经 stats 断言 enqueued/回插,write 不再无入队假 Ok);`c_punch_rotating_learned_delivery_exactly_once`(两接收端轮换/中途 learn,各报恰好一次落点正确);`c_punch_fatal_bridges_to_dial_error`;punch 父取消 pa6/pa7 保持绿。注:macOS loopback 难稳定造真实 kernel EAGAIN,WouldBlock 分支经 drain/stats 合同断言,如实不冒充内核复现。

## D ACK 合批(按 kcp-boundary/batching-plan.md 修订版,堵三缺口)
- 改动仅 run_session 入包分支(kcp.rs:713-723):`batch.clear()` 后 `inbound.recv_many(&mut batch, BATCH_CAP=64)`(tokio 1.53.2 已核,Cargo.lock);**n==0→break**(通道关闭+空,防热转到 90s);批内逐包 `kcp.input`,整批后照旧 1×update+recv-drain+1×flush;flush 条件/512 段限额/256KiB read_buf/MAX_BACKLOG_* 不动;低负载 recv_many 有 1 条即返回、不凑满。诊断计数(wakeups/packets_consumed/input_calls/update_calls/flush_calls 等,仿 BacklogStats)先行合入、零行为变更。
- 回归:`b11_batch_ack_single_flush`(红→绿,暂停时钟,K=8 **纯 ACK 输入**——喂对已发段的 ACK,无 recv_moved/user_wrote,杜绝虚假 flush 计数);`b12_single_packet_one_round_one_flush`(冻结守护,允许修前绿);`b_channel_closed_zero_break`(关闭即退);`b_full_batch_write_fairness`(满批下用户写/notify 有界轮内被服务);e1/e2 精确上界与既有 52+52 测试全绿(断言不删改)。
- 度量与撤销:同口径 counter baseline/candidate,`cargo run --release -p rh-transport --example kcp_loopback_bench -- 32 3` ≥3 次(单/4 流+300×64B p50/p99),按 batching-plan 门槛 (a)-(f);无局部收益仅撤 C1 自身 hunk(计数器/测试/红证据保留),不因此跳过最终完整矩阵(world.run)。

## E 2007 诊断区分自身/外部(api.rs/state.rs)
- 现状:创建 2007(api.rs:843)/更新 3005(1027)不区分占用者;组共享已短路试绑(834-841/1019-1025)。
- 改:helper `classify_port_occupation`(state.rs):按协议族(tcp/udp)+端口+IP 重叠(复用 rh_core::net::bind_conflicts 口径,通配/具体双向)匹配**活**自身监听:控制面 bind_port/bind_protocol+extra_listeners(config.rs:10-44)、web(55-66)、vhost http/https/tcpmux(84-98)、ssh_gateway(177-183)、**metrics 实际监听地址**(热改,metrics_listen_start 时记入 AppState,state.rs:331-372,勿只读配置文件)。命中→消息指名自身来源,未命中→「外部进程占用」;**数字码 2007/3005 与 JSON 形状不变**仅消息细化;OS 试绑兜底保留为最终裁定;自动分配(874-884)与 port_in_use(2006/3004)路径不动。
- 回归(api.rs 测试):`e_2007_names_self_listener`(metrics/web/vhost 各格);`e_2007_external_occupation_message`(真占端口→外部措辞);`e_udp_on_tcp_held_port_no_false_conflict`;`e_update_3005_same_classification`;`e_group_shared_no_trial_misreport`;`e_auto_allocate_untouched`。

## 公开 API 回归与排他权
- 形状不变:KcpIo AsyncRead/Write 契约(仅 shutdown 语义修正)、KcpListener/punch_dial/kcp::BacklogStats 签名、api 错误码体系;`mux_relay_echo_and_half_close`(mux.rs:1044)与 ws/quic/splice/yamux 红线锁全程绿。
- 排他(两两不相交):KCP=A/B/C/D 仅 `crates/rh-transport/src/kcp.rs`+新 `tests/kcp_*.rs`+`examples/kcp_loopback_bench.rs`;服务器诊断=E 仅 `api.rs`/`state.rs` 必要 helper+内联测试;性能证据=仅 benchmark 工具;docs 本文件一人写。越集即违规。
- 命令(定向,退出码留档 `benchmark/runs/performance-closure-c84/{kcp-closure,server-diag,kcp-batching}/`):KCP `cargo test -p rh-transport --lib kcp:: && cargo test -p rh-transport --test kcp_closure`;服务器 `cargo test -p rh-server --lib api::`;每条命令外层 deadline+`--locked --offline`。
- 强验收(world.run 一次性执行,修复者不代跑):`cargo fmt --all && cargo clippy --workspace --all-targets && cargo test --workspace` + 四套 e2e + 三机矩阵(tcp MUX=0 G1;tls mux G2;六协议 G3/G5;UDP G4;G6 配对 1/4/16 中位 ≥80%;G7 ≥3 次全留)+ `py/verify_goal.py --evidence`;结果只认原始证据,失败不过滤。
