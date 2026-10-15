# rabbithole goal-repair 最终简明报告(2026-10-14 报告轮,零源码/零测试改动)

> 本轮仅重写本报告;旧长篇 §1–§15(集成轮→e2e 修复轮全部过程证据)
> 逐字保留于文末「历史附录」,原始日志/JSON/散列一律不改不删;性能
> 数字的历史错误只在 `benchmark/runs/goal-repair/final-gaps/ERRATA.md`
> 勘误,不改 raw。最终独立复核由主代理本人另行执行,本报告不构成
> 「性能达标」「绝对无缺陷」或「软件无优化空间」声明。

## 1. 一句话结论
正确性与测试侧修复全部落地:Linux 测试床四套件全绿(final-linux-r2),
workspace 测试/clippy/验证器自测全 0 退出(主脚本事实输入);性能
G1/G2/G6 未达门槛——round-2 真实验收总体 FAIL,最终源码性能未复测,
**不得宣称性能达标**;全部失败样本与证据保留;需要用户授权的环境决策
(sysctl rmem/wmem、vmxnet3 USO/换网卡、拓扑变更)均未授权未实施。

## 2. 最终测试状态(真实退出码)
| 套件 | 结果 | 退出码 | 证据来源 |
|---|---|---|---|
| cargo test --workspace --locked --offline | 全过 | 0 | 主脚本 final 事实输入(非本会话亲跑) |
| cargo clippy --workspace --all-targets --locked --offline | 全过 | 0 | 同上 |
| verify_goal.py --self-test + py unittest(71) | 47/47、71 OK | 0 | 同上 |
| Linux tests/e2e.sh | 73/0(旧 6 失败全转绿) | 0 | final-linux-r2/logs/(事实输入) |
| Linux tests/e2e-resilience.sh | 19/0 | 0 | 同上 |
| Linux tests/e2e-transports.sh | 12/0 | 0 | 同上 |
| Linux tests/e2e-relay-modes.sh | 21/0 | 0 | 同上 |
| verify_goal --evidence round-2 | 总体 **FAIL** | 1 | round-2/verify-goal-report.txt(真实失败保留) |
| darwin tests/e2e.sh(本会话亲跑) | 73/0 | 0 | relay/e2e-fix-local/02-e2e-macos.log |
| darwin --self-test(本会话亲跑) | 4/4 | 0 | 本会话终端输出 |
| darwin 端口预检拦截遗留孤儿(亲跑) | 报错不杀 | 2 | relay/e2e-fix-local/01-e2e-macos.log |

final-linux-r2 前提核验(事实输入):生产源码 74 文件 SHA256 与上次
final-linux 构建零变更→复用同构建(rh-server 26c4704d…/rh-client
087bcb8c…);修正版 e2e.sh 同步(c2be73f4…,bash -n 通过);跑前三机
无相关进程、e2e 端口段 TCP+UDP 空闲;跑后无残留、快照唯一命名、未扫杀。

## 3. 逐项映射:请求 → 修复 → 测试 → 退出码 → 原始文件 → 覆盖
(证据根目录均在 `benchmark/runs/goal-repair/`;红日志尾部 `RETURN_CODE=101`)

1. **relay 取消/错误路径有界收敛**(集成轮请求+实时复核):mux.rs/
   data.rs/forward.rs `close_with_late_cancel`/`shutdown_with_late_cancel`,
   CANCEL_CLOSE_GRACE=1s;clean EOF 半关闭无界(红线)→
   `copy_rule_cancel_after_eof_converges` 等 → goal_repair_mux 10 过
   2 ignored RC=0 → relay/integration/01-40 → darwin 亲跑,Linux 由
   workspace=0 覆盖。
2. **cancel-after-eof**(主代理复现 101/timed_out):修后主代理 harness
   5/5 exit=0(断言未改)→ /tmp/rh-main-independent-review/ 日志在档。
3. **A5 退款/B7 copy 闸门**:`token_reservation_wait_cancel_refunds`
   (断言归零)、`relay_mode_copy_disables_chunk_gate` → rh-core 15/15、
   rh-client 7/7 RC=0 → integration/34 红在档。
4. **B1 nonmux 回落 prefix 回包**:`mux_steal_or_rewrap` →
   `nonmux_fallback_preserves_prefix_and_guard`、
   `nested_prefix_rewrap_preserves_order` → rh-server 12/12 RC=0。
5. **vendor yamux B2/B3/B4/C1-C5 + round-2 预授信**:vendor 28/28 RC=0;
   独立审查以 /tmp 反向突变重建 4 红,证实机理与计数(24+4=28)。
6. **验证器 r4/r5 加固**(整目录克隆/时间平移绕过):self-test 47/47、
   unittest 71 OK、负例 fixtures 全红 → benchmark/py/。
7. **e2e.sh 六失败修复**(BSD sed/metrics 断言过时/PIDS[4] 覆盖/广域
   扫杀/归档覆盖/密钥泄漏):详历史附录 §15;--self-test 4/4;darwin
   73/0;Linux r2 73/0(六项全转绿)。
8. **close-before-first-write**(final-gaps 轮;主代理复现 exit=101):
   vendor `Frame::close_stream(syn)` SYN|FIN 合帧、`poll_close` 在
   poll_ready 后消费未发 Syn、SendClosed 幂等 → vendor 4 红→28/28;
   final_gaps 2 红→4/4;主代理 6 用例复跑全 exit=0 → final-gaps/logs/
   (PREFIX-RED 在档)。
9. **ws.rs R5 不变量锁**(旧审查缺口):仅 `#[cfg(test)]` 增 4 测试
   (send_queued 64B 双工背压不重不丢/手动 poll 单帧/前缀同批首帧保留+
   跨批拆分)→ 8 passed RC=0;突变红 ws-mutation-red.log 在档、已精确
   还原;生产状态机零改动。
10. **copy 错误路径有界收敛**:`ERROR_CLOSE_GRACE=1s`(data.rs/forward.rs)
    → `copy_error_path_converges_when_shutdown_pends`、
    `client_copy_error_converges_when_shutdown_pends` → 12/12、7/7
    RC=0(修前各 1 红,3s 超时);clean EOF 半关闭无界保持(红线)。
11. **红线扫描工具化**:`final-gaps/redline_scan.py` 剥注释/字符串分桶
    → code 命中 0/exit 0;自植 SO_SNDBUF 负例 code 命中 1/exit 1 →
    redline-scan{,-negative}.txt;64KiB/65536 另有单元常量锁兜底。
12. **ERRATA 六条勘误**:final-gaps/ERRATA.md(内容见 §6 末);raw 未改。

## 4. 六协议性能门槛真实状态(round-2 真实验收;最终源码性能未复测)
| 门槛 | 要求 | tcp | tls | ws | wss | quic | kcp | 判定 |
|---|---|---|---|---|---|---|---|---|
| G1 TCP nomux splice 单流 | 双向各≥3000Mbps | 1943/2113 | — | — | — | — | — | **FAIL**(direct 3072/3010) |
| G1 CPU | <50% 单核 | 28.1/33.5% | — | — | — | — | — | PASS |
| G2 TLS mux 单流 | 双向各≥2500Mbps | — | 1172/1752 | — | — | — | — | **FAIL**(CPU 中位 140.2%/132.7%) |
| G3 正吞吐 | >0 | P | P | P | P | P | P | PASS |
| G4 UDP 1200B | 损失<1% | 0% | 0% | 0% | 0% | 0% | 0% | PASS(n=3 中位) |
| G5 4096 真实隧道 | ×3 verified | P | P | P | P | P | P(+tcp nomux 对照) | PASS |
| G6 配对保留率 ≥80% | 36 档 | 3F/6 | 3F/6 | 2F/6 | 5F/6 | 6F/6(19-32%) | 6F/6(3-6%) | **FAIL(25/36)** |
| G7 ≥3 独立配对中位数 | 全部 | — | — | — | — | — | — | PASS(r4/r5 加固后) |

- 总体 **FAIL**(verify_goal RC=1);数字为 round-2 时点源码,其后仅
  正确性修复(未动任何性能参数/门槛),最终源码性能验证未执行。
- G6=25/36 以 ERRATA E1 勘误为准(tcp3+tls3+ws2+wss5+quic6+kcp6);
  ROUND2-REPORT.md:19 原文"24/36"为叙述错误,raw 保留不改;wss/d16
  转达标与 tls/u4 转不达标相抵,真实变化 25→25。
- 客观约束:wss 轮 direct 单流 upload 中位 2686<3000(测试床直连天花板
  ~3-3.4G);quic 596-810Mbps@169%/174% CPU;kcp 3-6% 全档。
- round-2 修复真实效果:tcp-mux 单流仅 +8%、tls 持平;wss 16 流
  +16%/+22%;单流钳制假设未在真实测试床兑现,归因矩阵未闭合。

## 5. 旧审查项处置(不把已补修消除项称仍存在)
| 旧审查发现 | 现状 |
|---|---|
| §6b R5 ws.rs send_queued/from_partially_read 测试缺失(高) | **已消除**:final-gaps 轮 4 测试+突变红;独立审查 verified(ws.rs:289-568) |
| 红线 3 grep"0 命中/exit 1"不可逐字复现(注释命中) | **已取代**:redline_scan.py 分桶(code 0/exit 0+负例 exit 1) |
| ROUND2-REPORT G6"24/36"叙述错误 | **已勘误**:ERRATA E1=25/36,raw 不改 |
| e2e-relay-modes.sh 未实际执行 | **已消除**:Linux r2 实跑 21/0 exit=0 |
| 全 workspace test/clippy 未跑 | **已消除**:主脚本 final 均 0 |
| verify_goal 复跑与存档报告输出不逐行等同(kcp 配对 0<3 行;判定一致 FAIL/RC=1) | **仍开放**:存档疑似旧版验证器生成,生成时刻未核实 |
| Linux-only splice 测试/musl 交叉未跑 | **仍开放**(见 §6) |
| 三机性能复测/最终源码性能验证 | **仍开放**:G1/G2/G6 未过 |

final-gaps 轮独立审查结论 approved,非阻断项与修复者自报一致
(splice Linux 测试未跑、扫描器字面量局限、SendClosed 幂等 wire 变化
已由 repeated_close_sends_single_fin 锁定、on_drop_stream Open 态
无 SYN RST 为一致性分析非缺口、性能未达标复核属实)。

## 6. 未完成与不宣称(如实)
- **G1/G2/G6 未达**:红线内估计剩余空间 ≤+15% vs 需 +43%(该 ≤+15%
  是两旋钮之和的推断非定理——ERRATA E4);出路(sysctl rmem/wmem 解锁、
  vmxnet3 USO fixed/换网卡、拓扑变更)全部需**用户授权,未授权不实施**。
- 最终源码性能验证未执行(final-linux-r2 按指令仅正确性 e2e)。
- `splice_sibling_aborted_on_pump_panic`(splice.rs,cfg(linux))
  darwin 不编译、musl 交叉失败(无 musl-gcc/目标 std)→ 待 Linux 测试
  床,不声称已验证;其依赖 join_relay_pumps 已由 darwin 2 个可移植
  测试覆盖。
- vendor/final_gaps/copy 新测试的修前红仅存会话终端输出+主代理
  PREFIX-RED 日志(拒绝回退修复补录红日志)。
- 生产红线内未测假设:RH_NOTSENT_LOWAT=64KiB 收益未验证(下轮 env
  矩阵;mux/copy 是否补装 LOWAT 依赖该对照,无证据不实施);yamux
  信用回路时延实测未闭合;tls 上行 1341-1450 vs 下行 ~1950-2007 差额
  归因不闭合(无 perf/pidstat);quic/kcp 平台级限制未解;refrelay2
  第一版单连接 hang 深因未解释(回退旧线程结构后正常)。
- omniscient forger(一致重写 started_at+run_id 全套伪造)超出离线
  验证器能力(已声明残余);G4/G5 自报证据强度与 config introspection
  限制见历史附录 §10。
- e2e.sh 密钥 env 传递无运行中 ps 动态取证(Linux 实跑日志 grep 64-hex
  0 命中);auto_range 20000-21000 不在静态预检(绑定失败自然暴露)。
- redline_scan 字符串字面量并入 comment 桶,数值常量(如 0x1001)可
  绕过(docstring 自述);64KiB/65536 红线另有单元常量锁兜底。
- ERRATA 六条:E1 G6=25/36 非 24;E2 两轮对比基线/口径混用(2484 实为
  16 流 direct、tcp/u4 引用错档、wss/d16 口径混用);E3 refrelay pipe
  实际 64KiB(裸 pipe 无 F_SETPIPE_SZ);E4"红线内 ≤+15%"为推断;
  E5 ethtool USO off[fixed]≠内核 GSO/UDP_SEGMENT 不可用(未探测);
  E6 151%/131% 单核归一≠整机饱和。

## 7. 无提交/无推送/无环境遗留
- **无 commit/push**:`git log -1` 停在 c3ca4bd(与开工一致);全部交付
  为工作区改动(主代理事实输入 git status:M 36 项含 tests/e2e.sh,
  ?? 9 项:benchmark/、vendor/、splice.rs、rh-transport/tests/、
  docs/×4、tests/e2e-relay-modes.sh),无入库。
- **环境遗留**:final-linux-r2 后 test1/2/3 复核 CLEAN、无 rh-server/
  rh-client/python3-(含 DNS stub)残留、端口空闲(修正版 PID 治理
  生效,未扫杀);本会话 darwin 跑后 ps/lsof 复核无进程/监听残留;
  /tmp 快照唯一命名保留;旧 /tmp/rh-e2e-last 为上轮旧脚本自留未删;
  /tmp/rh-*-review* 审计目录有意保留;**无系统配置修改**(sysctl/内核
  参数零改动)。
- 历史数字错误只在 ERRATA 勘误,raw(日志/JSON/散列)零改动;
  final-linux 旧日志密钥泄漏件已作脱敏副本(e2e.log.redacted)供外发,
  原件保留本地不动。

## 8. 指针
- 历史附录:本文件末尾(原 §1–§15 逐字保留:红线核查、集成核对、
  各轮独立审查处置、cancel-after-eof、验证器 r4/r5、e2e 六修复)。
- 原始证据:`benchmark/runs/goal-repair/{relay/,round-2/,final-gaps/,
  final-linux/,final-linux-r2/,diagnosis-after-round-2/}`;验证器
  `benchmark/py/`;勘误 `final-gaps/ERRATA.md`。
- 工作流结论(事实输入):完成了本轮修复与测量,但仍有未通过项;已
  保留全部失败证据,需主代理复核后继续修订,不能声称性能达标。

---

# 历史附录(旧长篇 §1–§15,自 2026-10-13 集成轮至 2026-10-14 e2e 修复轮,逐字保留、不作任何改动)

> 以下为原《goal-repair 集成轮状态(实现范围与命令证据)》全文,
> 作为过程证据历史附录原样保留;其中任何与上文简明报告/ERRATA 冲突
> 的数字(如 §12 的 G6"24/36"转述)以 ERRATA 勘误为准。


# goal-repair 集成轮状态(实现范围与命令证据)

> 集成轮(2026-10-13):合入三份并行交付(vendor/yamux P1、relay 取消收敛 P2、
> benchmark 测量方法 P3),逐项处理主代理实时复核
> (`/tmp/rh-main-independent-review/live-review.md`),按本轮指示补齐
> mux close 队列满 / poll_shutdown 永久 Pending 的故障注入与
> 「取消/错误路径有界收敛、clean EOF 保留半关闭」保证,并创建 P2 遗留的
> `tests/e2e-relay-modes.sh`。所有命令真实退出码留档于
> `benchmark/runs/goal-repair/relay/integration/`(runner:
> `benchmark/runs/goal-repair/relay/rule-cancel/run_with_deadline.py`,
> Python `start_new_session` + 超时 killpg,日志尾部有 `RETURN_CODE=` 行)。
> 全 workspace 默认全套测试按约定由脚本 `world.run` 统一执行,本轮只跑定向回归
> (未用任何全局 `--test-threads=1` 替代)。

## 1. 本轮实现改动(文件清单)

| 文件 | 改动 | 目的 |
|---|---|---|
| `crates/rh-transport/src/lib.rs` | `AbortOnDropHandle` 增加 `panicked()` 透视(吸收 JoinError 为状态而非吞掉);新增 `join_relay_pumps()`(单向 panic → 立即终止对向;双向正常 → 等两向自然结束) | live-review #2:单向 panic 对向不得永存 |
| `crates/rh-transport/src/mux.rs` | ① `pump_up`/`pump_down` 返回 clean 标志(Ok(None)=clean EOF;Err/写失败=unclean);② t1/t2 收尾:取消**或错误**路径 `timeout(CANCEL_CLOSE_GRACE, close/shutdown)` 有界,**clean EOF 仍无界等待完整 FIN(红线不动)**;③ `up_errored`/`dn_errored` 标志 + 收尾 select:单向 panic 或单向错误退出 → 守卫 Drop → abort 对向;④ clippy 清理(needless_return、const-block assert、`Waker::noop()`) | ask:取消/**错误**路径有界收敛;live-review #1/#2 |
| `crates/rh-server/src/data.rs` | `copy_relay_pair` 改用 `join_relay_pumps`(panic 收敛);新增 `copy_relay_panic_contained` 测试;clippy 清理(const-block、`Waker::noop()`) | live-review #2 对 copy 路径 |
| `crates/rh-client/src/forward.rs` | `splice()` 改用 `join_relay_pumps`;新增 `client_copy_panic_contained` 测试 | 同上(client copy 路径) |
| `crates/rh-server/src/control.rs` | clippy 机械清理(`ptr::read(&md.x)` 免 mut、`md` 免 mut) | B1 交付代码 clippy 清零(P2 遗留未跑 clippy) |
| `crates/rh-transport/tests/goal_repair_mux.rs` | 新增 3 项故障注入测试(§3);`WedgeableIo`/`WriteErrShutdownPendIo`/`PanicWriteIo` 故障 IO | ask 点名的故障注入 |
| `tests/e2e-relay-modes.sh` | **新增**(P2 unresolved 列表遗留;方案 §7.2 补缺、§7.3-3):tcp/tls × {默认,copy} × mux,登录/上线/转发/半关闭 + `rabbithole_relay_path_total` 计数器断言;Linux 附加非复用 splice 轮(本机跳过,脚本内显式判 `uname -s`) | 补齐遗漏 |
| `cargo fmt -p rh-transport/-p rh-server/-p rh-client` | 格式归一(并行交付遗留 diff) | `cargo fmt --check` 过 |

未改动:vendor/yamux 全部源码(除临时诊断探针,已逐字节移除并重验 20/20)、
`ws.rs`(红线)、`AGENTS.md`、既有 `tests/*.sh`、benchmark 源。

## 2. 主代理实时复核(live-review.md)逐项处理

1. **规则取消 select 后 close/shutdown 无界等待** — P2 已加 `CANCEL_CLOSE_GRACE`;
   本轮(a)把**错误路径**(非取消、非 EOF:读/写 Err)也纳入有界收尾,
   (b)补上「yamux 命令队列满」故障形态的独立用例(见 §3-T1),
   (c)确认 clean EOF 分支保持无界(半关闭红线),由既有
   `mux_relay_echo_and_half_close`(300KiB+shutdown+read_to_end)与
   `copy_rule_cancel_converges_when_shutdown_pends` 锁定。
   注意事项(实现层发现):vendor yamux 连接死亡(cleanup→`drop_all_streams`,
   connection.rs:411-415)在流上表现为 **clean EOF**(`State::Closed → can_read=false →
   Ok(None)`),不会触发错误路径——该路径因此维持无界半关闭语义,外层由
   父任务 abort / 规则取消兜底,与红线一致。
2. **AbortOnDropHandle 吞 JoinError / 单向 panic 对向永存** — 已修(§1)。
   测试:`mux_relay_sibling_aborted_on_pump_panic`、`copy_relay_panic_contained`、
   `client_copy_panic_contained`,三者红注入取证(修前 plain `join!` 形态 →
   3s Elapsed 失败 exit 101:01-*/03-/04- 日志)。错误返回面:守卫吸收 panic
   不再向上传播新 panic;`panicked()` 提供可观测状态。
3. **ChunkLimiter 新 API 所有调用点** — grep 全树核对(实现:
   `forward.rs` ClientBucketLimiter、`data.rs` mk_limiter 双桶、splice.rs DirHook
   消费;测试:goal_repair_mux CountingLimiter、splice.rs SlowLimiter):
   全部走 `reserve(n).await` → `consume(k)`(送达逐步消费)→ drop 退还三态,
   无旧 `acquire` 误用(copy 路径保留旧 `acquire` 为既定语义)。
   本轮新增 T1 用例同时锁 **up 侧 fill 口径**(charged>0 而送达更少)。
4. **pool 测试默认并行可靠性** — `cargo test -p yamux` 默认并行连跑 3 次
   20/20 exit 0(09-green-yamux-default-parallel-{1,2,3}.log);池测试用私有
   `Pool::new()` 实例(pool.rs:154-260),与全局池隔离,已核实源码。
5. **pool 容量口径** — pool.rs:19/42 与 vendor README 均为
   `MAX_IDLE(256) × CLASS_MAX(72KiB) ≈ 18MiB`(静态断言 ≤20MiB),无 16MiB 残留。
6. **read_chunk_into 大 buffer 实际行为锁** — `mux::tests::read_chunk_from_cap_lock`
   直接注入 256KiB `BytesMut`(绕过池,池拒收后不恒真):本轮重做红注入
   (`limit = spare.len()` 去掉 `.min(cap)`)→ 失败「超过 64KiB 上限(A4)」exit 101
   (02-red-read_chunk_cap_lock.log),恢复后绿。
7. **QuotaIo 守卫与 prefix** — `assign_forward` 中 `_quota_guard` 活到函数尾
   (覆盖 chunk/splice/copy 全部中继路径与早退 Drop;data.rs:245-246);
   mux 路径 prefix 经 `set_front_prefix` 最先交付;既有
   `chunk_path_engages_through_quota_and_prepended`(QuotaIo<PrependedIo<RhMuxStream>>
   全链:guard 存活计数 + 前缀 32B 最先 + 364KiB 载荷完整)本轮重跑绿。
   splice 回落路径遇 PrependedIo 残留时保守回落 copy(data.rs:302-309),不掉 prefix。
8. **PrependedIo::peel 非匹配消费原 IO** — 笔记已过时:当前实现非匹配返回
   `(Vec::new(), 原 io)`(lib.rs:136-157),调用点 data.rs:277 先 peel 再判 mux,
   失败回落不误调;`prepended_peel_roundtrip` 锁定,绿。
9. **Reservation 计量口径分离** — up=读侧 fill(pump_up 读完成即扣并消费)、
   down=送达(pump_down 冲刷前预留、按实际写出消费):T1 新用例断言楔死时
   charged > 客户端送达字节;`pump_down_no_double_charge` 断言净扣费==送达;
   splice 钩子 `meter_on_fill` up=true/down=false 与此一致(splice.rs:295-330,读核对)。
10. **e2e-transports.sh 进程清理** — 确认 tests/e2e-transports.sh:15 会扫杀全部
    `/tmp/rh-tr-*`;本轮未改该脚本(测试机事项)。新 e2e-relay-modes.sh 用独立
    `/tmp/rh-rm-` 前缀并只清理自己的进程,不放大该风险;已提醒:三机执行
    e2e-transports.sh 前须确认无本轮外匹配进程、先归档日志。
11. **release 证据不覆盖旧日志** — 本轮对**最终代码**重跑:
    `cargo test -p yamux --release` 20/20 exit 0(10-*)、
    `cargo test -p rh-transport --release` 26+8+1+1 exit 0(11-*)、
    e2e 用 `target/release` 真实二进制(20-*/22-*)。旧 vendor 日志 00-11b 未改动。

## 3. 新增故障注入回归(全部修前红 → 修后绿)

| 用例 | 故障形态 | 断言 | 红证据(修前形态注入) | 绿 |
|---|---|---|---|---|
| T1 `mux_relay_cancel_converges_when_close_queue_full` | 载体写楔死(`WedgeableIo`,连接建立后置位)→ yamux 命令队列满 → `poll_close_half` 永久 Pending;规则取消 | ≤3s 收敛、IO 释放、refunded==0、**charged > 客户端送达字节**(fill≠送达) | 01-red-…queue_full.log(exit 101;注入:收尾去 grace,即 P2 之前形态) | 05/23 |
| T2 `mux_relay_error_path_converges_when_shutdown_pends` | 用户侧 `poll_write` 恒 Err + `poll_shutdown` 恒 Pending(错误路径,无取消) | ≤3s 收敛、IO 释放、对向(读等待)被终止 | 01-red-…error_path.log(exit 101;注入:错误收尾无界+无对向终止=P2 形态) | 05/23 |
| T3 `mux_relay_sibling_aborted_on_pump_panic` | 用户侧 `poll_write` panic | ≤3s 收敛、IO 释放 | 01-red-…pump_panic.log(exit 101;注入:plain join!) | 05/23 |
| T4 `data::tests::copy_relay_panic_contained` | 服务端 copy 泵写 panic | 收敛+双侧 IO 释放 | 03-red(exit 101,同上注入) | 06/23 |
| T5 `forward::tests::client_copy_panic_contained` | 客户端 copy 泵写 panic | 收敛+双侧 IO 释放 | 04-red(exit 101) | 07/23 |
| T6 `mux::tests::read_chunk_from_cap_lock`(既有) | 256KiB 超限缓冲注入 | 每块 ≤64KiB 且载荷完整 | 02-red(去 `.min(cap)` 注入,exit 101) | 05/23 |
| T7 e2e `tests/e2e-relay-modes.sh` | B7 门移除(release 重编译) | copy 轮 path="copy"≥1 且 path="chunk"==0 | 21-red-e2e(4 FAIL exit 1) | 22-green(14/14 exit 0) |

T1 实现层发现(记录):vendor yamux poll 循环在 pending 控制帧(如 RTT Ping 的
Pong)未写穿前不读入站帧(connection.rs:453-483 `pending_read_frame` 门),
因此**自始楔死载体会饿死 accept**——T1 采用「连接建立后置位楔死」构造,
该 vendor 读饿死行为本身未在本轮修改(非本轮范围,已记录)。

## 4. 定向回归与检查(全部真实退出码,日志在 `benchmark/runs/goal-repair/relay/integration/`)

| 命令 | 结果 | 日志 |
|---|---|---|
| `cargo test -p rh-transport`(默认并行) | 26+8+1+1 通过,exit 0 | 05/12/15/23 |
| `cargo test -p rh-server` | 8 通过,exit 0 | 06/13/16/23 |
| `cargo test -p rh-client` | 5 通过,exit 0 | 07/13/16/23 |
| `cargo test -p rh-core`(core test availability) | 15 通过,exit 0 | 08/16/23 |
| `cargo test -p yamux`(默认并行 ×3) | 20/20 ×3,exit 0 | 09-{1,2,3} |
| `cargo test -p yamux --release` | 20/20,exit 0 | 10 |
| `cargo test -p rh-transport --release` | 26+8+1+1,exit 0 | 11 |
| `cargo check --workspace --all-targets` | exit 0(仅 4 条基线警告:Batch 字段、inner_ref、vendor quickcheck、build-script 提示) | 17/23 |
| `cargo clippy -p rh-transport -p rh-server -p rh-client --all-targets` | exit 0;余量均为记录在案的基线(mux.rs:729 Batch 语义标记、control.rs:582 inner_ref linux 门、main.rs:83、ws_mux_repro.rs:221、vendor 2 条 P1 基线) | 14 |
| `cargo fmt -p rh-transport -p rh-server -p rh-client --check` | exit 0 | 23 |
| `python3 py/verify_goal.py --self-test`(新增验证器) | 37/37 对抗用例全 PASS,fail-closed,exit 0 | 18 |
| `python3 -m unittest discover -s py/tests`(benchmark) | 60 tests OK,exit 0 | 19 |
| `bash tests/e2e-relay-modes.sh target/release` | 14/14 PASS,exit 0(非 Linux 跳过 2 轮,显式打印) | 22 |
| `bash -n tests/e2e-relay-modes.sh` | exit 0 | — |

## 5. 红线核查(本轮实际执行)

- **quinn 并发流上限 65536**:quic.rs:23/42-43,锁测试 `quic.rs:249-257`
  (`QUIC_STREAMS_CAP==65536`、钳制路径),随 26 项 lib 测试绿。
- **转发读取缓冲 ≤64KiB**:`CHUNK_CAP`/`COPY_MAX_BUF` 常量锁测试
  (mux.rs `chunk_cap_is_64kib`、data.rs `copy_buf_cap_is_64kib`)绿;
  行为锁 T6(注入超限缓冲)绿。
- **不设 SO_RCVBUF/SO_SNDBUF**:`grep -rn "SO_RCVBUF\|SO_SNDBUF|
  set_recv_buffer_size\|set_send_buffer_size" crates/ vendor/ --include="*.rs"`
  → **exit 1(0 命中)**(直接跑 grep 取真实退出码,无管道掩盖)。
- **ws.rs send_queued/from_partially_read 状态机**:本轮零改动;状态机在位
  (ws.rs:92,156,183,248,254,265)。
- **yamux driver open_rx Ready(None) 不终止存量流**:mux.rs driver 批收集循环内
  `Poll::Ready(None) => break`(仅退出收集,连接继续服务),
  `yamux_open_handles_released_serves_existing_streams` 绿。
- 未删测试、未加 ignore、未改阈值;全部新增回归以注入式红取证(修前形态)。

## 6. 集成核对结论(三份交付)

- **P1(vendor/yamux)**:API 零变化,无需接线;默认并行/串行/release 均 20/20
  (本轮重跑);容量口径 18MiB 统一;C2 补修注入红证据(11b)在档。
- **P2(relay)**:整树可编译已达成;本轮补齐其「错误路径有界」与「panic 对向
  终止」两处缺口 + e2e-relay-modes.sh;ChunkLimiter 接线全部调用点核对无误。
- **P3(benchmark)**:验证器/负载/采样工具本轮仅**使用**未改码;
  self-test 37/37(从 30 扩至 37 为交付时状态,以本轮实跑为准)、unittest 60 通过。
  未连远端、未跑三机基准。

## 7. 追加轮(2026-10-13 晚):主代理新增两项定向修复

### 7.1 修复 A:assign_forward 非 mux 回落丢 prefix(data.rs)

- **缺陷**:`assign_forward` 剥 `PrependedIo` 后仅在 mux 偷取成功时交付 prefix;
  非 mux / 偷取失败回落 `channel = ch` 直接丢弃剩余 prefix(先行字节丢失)。
- **修复**:新增 `mux_steal_or_rewrap(prefix, ch) -> Result<halves, BoxedIo>`:
  mux 偷取成功 → prefix 注入读半 front;**任何回落 → Err(回包产物)**,非空
  prefix 以 `PrependedIo::wrap` 原样回包(顺序:外层残留→内层残留→流数据,
  与原包装链逐字节等价),assign_forward 以 match 接住回落,`channel = fallback`。
- **splice 安全**:回包产物是 PrependedIo,`splice::plain_tcp` 为严格
  `downcast_ref::<TcpStream>`(splice.rs:94-96)必然不匹配 → 回落 copy,
  **尚有 prefix 的流不可能走内核 splice**(代码路径层面保证,macOS 无法执行
  Linux-only plain_tcp,以读核对+回包产物类型断言锁定)。
- **回归**:`data::tests::nonmux_fallback_preserves_prefix_and_guard`
  (正常 server 包装链 QuotaIo<PrependedIo<裸 duplex>> 非 mux+非空 prefix:
  prefix 最先、线路字节随后、QuotaGuard 全期 data_conn_count 不提前递减、
  释放即递减)与 `nested_prefix_rewrap_preserves_order`(嵌套 prefix
  B→A→流顺序)。红注入(`Err(ch)` 丢 prefix 修前形态)→ 双双 FAILED
  exit 101(26-/27- 日志),修复后绿(28/32)。

### 7.2 修复 B:EOF 收尾期间抵达的取消不再被忽略(mux + server copy)

- **缺陷**:`by_cancel` 只在泵循环 select 时记录;泵先 clean EOF 进入
  close/shutdown 收尾后,规则取消永不再被监听 → 收尾楔死(poll_shutdown
  永久 Pending 等)时 relay 卡死、IO 不释放。主代理复现:
  `/tmp/rh-main-independent-review/src/main.rs` cancel_after_eof,
  修前日志 `cancel-after-eof-before.log`(exit 101,timed_out=true
  io_dropped=false)。
- **修复**:mux.rs `close_with_late_cancel` / data.rs `shutdown_with_late_cancel`
  (同义):clean EOF 收尾 = `select { close/shutdown, cancel }`——**未取消:
  无界等完整半关闭传递(红线,不对正常关闭硬加短超时防截断)**;取消抵达:
  已推进的 close/shutdown 仅在 CANCEL_CLOSE_GRACE 内继续等待,超时放弃释放。
  取消/错误路径收尾维持既有 grace 有界不变。
- **回归**:`mux_relay_cancel_after_eof_converges`(与主代理复现同款拓扑/
  节奏:F IN → 100ms → 取消)与 `data::tests::copy_rule_cancel_after_eof_converges`。
  红注入注意:仅回退收尾监听不够——上一轮的「单向异常退出→终止对向」已可
  收敛该场景,故红注入回退到**完整修前形态**(plain join + 无错误标志 +
  无收尾监听)→ mux 用例 FAILED 3.11s exit 101(24- 日志)、copy 用例
  FAILED 3.11s exit 101(25- 日志),修复后绿。
- **主代理 harness 复跑**(其源 `/tmp/rh-main-independent-review/src/main.rs`
  原样拷贝至 path-dep 工程 `/tmp/rh-readonly-review.eFSs9d` 重编译,断言零改动):
  double-charge / abort-parent / cap / cancel-close / **cancel-after-eof**
  全部 `fixed: ...` RC=0(29- 日志;cancel-after-eof 输出
  `relay exited and IO dropped`)。注:首轮复跑曾误用工程内旧
  `rh-main-independent-check` 二进制(其 src/main.rs 为旧 API 版,新构建
  产物名为 `rh-readonly-review`),已以拷贝源重编译后复跑为准。
- 既有 cancel-before-shutdown 用例(`mux_relay_cancel_close_converges`、
  `copy_rule_cancel_converges_when_shutdown_pends`)保持通过(28/32)。

### 7.3 追加轮命令证据(`benchmark/runs/goal-repair/relay/integration/`)

| 日志 | 内容 | 结果 |
|---|---|---|
| 24/25/26/27-red-* | 4 项新回归修前形态红注入 | FAILED exit 101 |
| 28-green-all-post-fix | transport(26+9+1+1)/server(11)/client(5)/core(15)/yamux(20) | 全 ok,exit 0 |
| 29-main-harness-all5-rebuilt | 主代理 harness 5 例(源码原样) | 全 RC=0 |
| 30-linux-musl-check | `cargo check --workspace --all-targets --target x86_64-unknown-linux-musl` | exit 101:ring 构建脚本缺 `x86_64-linux-musl-gcc`(环境缺 musl CC,非代码错误,与 P2 日志 17 同) |
| 31-release-build-and-e2e | release 重编 + `bash tests/e2e-relay-modes.sh target/release` | 14/14 PASS,exit 0 |
| 32-final-all-suites | 全定向套件 + workspace check + clippy | 全 ok,exit 0;clippy 余量仍为基线集(Batch 标记行号移至 mux.rs:765、inner_ref、main.rs:83、ws_mux_repro.rs:221、vendor 2 条) |

JoinError/panic 收敛核查:三项 panic 回归(mux/service copy/client copy)
与 JoinError 透视随 28/32 全绿。fmt:`cargo fmt -p rh-transport -p rh-server
-p rh-client --check` exit 0。

## 8. 未尽事项(如实)

0. **musl 交叉编译环境缺失**(本轮实测,30- 日志):本 Mac 无
   `x86_64-linux-musl-gcc`,ring 构建脚本失败 → Linux-only 代码
   (splice.rs、forward.rs splice 块)仍需 Linux 机 `cargo check
   --workspace --all-targets` 确认。
1. **三机真实链路验收未执行**(本机 macOS):G1/G2 性能、G4 UDP、G5 4096、
   CPU/proc/exe 证据均属验收机;CPU 采样在 darwin 已验证 fail-closed。
2. **forward.rs Linux-only splice 块在本 Mac 无法编译验证**(ring 需 musl 交叉
   C 工具链,P2 日志 17 为环境失败);已按 splice.rs 结构人工核对,需 Linux 构建
   确认(`cargo check --workspace --all-targets` 在 Linux 全量覆盖)。
3. **e2e-relay-modes.sh 的 Linux 非复用轮**(path="splice" 断言)未在本机执行
   (splice 为 Linux-only);Linux 上跑 `bash tests/e2e-relay-modes.sh target/release`
   即覆盖。
4. copy 路径(data.rs/forward.rs)的**错误路径**收尾仍无界(本轮仅 mux 按点名
   实施;copy 泵 `copy_limited` 不区分 clean/unclean)。实际风险低(取消/父 abort
   均有界),如需与 mux 完全对齐可后续按同式补。
5. vendor yamux「pending 控制帧未写穿时不读入站帧」的读饿死行为(§3 备注)
   未修——超出本轮集成授权(vendor 行为归 P1 域),建议单独评估。
6. G1 CPU 门槛取「同方向 ≥3 样本中位数」(G7 口径)vs 逐样本 <50% 仍待主代理
   裁定(P3 unresolved 第 3 条);门槛数值未动。

## 9. 追加轮 2(2026-10-13 深夜):全套脚本暴露问题修复(独立审查二轮)

### 9.1 验证器(独立审查二轮否决项,benchmark/py/verify_goal.py r4)

- **[高] 整目录克隆绕过 G7「≥3 次独立配对测量」**:新增 `cross_dir_independence()`
  ——同一 (proto,mux,relay[,path,dir,streams] / [path] / [n]) 的吞吐/UDP/并发
  样本在**全部目录间(含目录内)时间窗必须两两不重叠**(固定三机拓扑串行执行,
  独立测量的物理时间窗不可能重叠);重叠 → 涉事目录 issue(目录不合格,
  全部门槛 FAIL)。取证复现:独立审查 exp_clone_dir(samples=1 基线 FAIL →
  克隆×2 修前全过);现 self-test 负例「整目录克隆冒充多次独立测量 → FAIL」。
- **[高] UDP/concurrent 无独立性查重(同目录复制计入样本数)**:
  `_check_udp`/`_check_concurrent` 增加 (path,sample)/(n,sample) 重复拒收 +
  同 key 时间窗重叠拒收(UDP 的 started_us/ended_us 升级为必填字段,缺失即
  FAIL——无法查独立性);跨目录由上面的统一机制覆盖。self-test 负例:
  「UDP 记录复制(改号保时间窗)」「concurrent 记录复制(改号保时间窗)」
  「UDP 缺时间窗字段」;unittest 另加非等值部分重叠负例(锁 overlap 谓词)。
- **[低] schema.json run_id 不与 meta 交叉**:解析后强制一致,不一致报
  「目录混装」FAIL(负例:schema run_id=fx-other-round)。
- **[信息] G1 CPU 覆盖达标但 CPU% 不可算被静默跳过**:改 fail-closed
  (该分支在合法参数下不可达,只防操纵;门槛数值未动)。
- **结构性限制(如实,不改门槛、不伪装修复)**:①G4/G5 数字来自 loadgen
  自报告 JSON,无第三方原始文件可重算(吞吐有 iperf3 raw 三件套)——验收
  流程需三机现场监督补强;②「实际配置」是端点 toml 回读而非进程内省;
  G5 的单连接多流形态由 multiplex=true 配置回读间接支持。
- self-test 37→**42/42**(新增 5 负例);unittest 60→**66 全 OK**
  (`python3 -m unittest discover -s benchmark/py/tests`,exit 0)。
- 修 fixture 顺带缺陷:UDP 样本时间线不推进(所有样本同窗)——与新独立性
  检查冲突,已改为串行推进(与 peer.sh 真实行为一致);`build_fixture_dir`
  新增 `base_us` 参数(同配置第二轮目录时间线后移,CPU 不混配负例改用)。

### 9.2 Rust 侧(独立审查一轮非阻断项 + 测试强度)

- **R1 relay_mode=copy 未关客户端 chunk 快路径**(forward.rs:115 修前只查
  COMPRESSED):新增 `client_chunk_gate(use_compression, relay_mode)` 真实
  判定函数接入 handle_start_forward(与 data.rs assign_forward 同式,兑现
  benchmark/README.md:223 copy 对照承诺);测试直接锁该函数(非镜像)。
  红注入(忽略 relay_mode 修前形态)→ FAILED exit 101(34- 日志)。
- **R2 mux_rate_wait_cancel_refunds 弱于声明**:NeverLimiter 的 refunded
  计数此前从未断言;现 `assert_eq!(refunded, 1)`(等待中取消 → 在途预留
  全额退款,净扣费==已送达==0)。红注入(write_metered mem::forget 泄漏
  预留)→ FAILED exit 101(35- 日志)。
- **R3 goal_repair_pool_repro 恒真**(B3 后 256KiB 播种为 no-op):重写为
  公共入口合成红线锁并如实改注(Bytes 冻结后容量不可观,长度即公共入口
  可观测量;分解锁=B3 vendor pool 测试 + A4 mux.rs 私有缝)。
  红注入(B3 拒收回退 + A4 clamp 回退,即原始 repro 双缺陷形态)→
  「超过 64KiB 上限(修前经公共入口可读出 262144)」FAILED exit 101
  (36- 日志)。
- 独立审查一轮的「未运行项」维持如实:Linux-only splice 测试与全工作区
  test/clippy 由脚本在 Linux 执行(本轮 workspace check/clippy 本机已过,
  见 37- 日志);性能门槛未验证。

### 9.3 本轮命令证据(`benchmark/runs/goal-repair/relay/integration/`)

| 日志 | 内容 | 结果 |
|---|---|---|
| 33-green-r1r2r3 | R1/R2/R3 修复后定向(goal_repair_mux 9/pool 1/client 6/server 11) | 全 ok exit 0 |
| 34/35/36-red-* | R1 客户端门控 / R2 退款合同 / R3 公共入口上限 红注入 | 全 FAILED exit 101 |
| 37-green-final-all | transport 26+9+1+1 / server 11 / client 6 / core 15 / yamux 20 + workspace check + fmt --check | 全 ok exit 0 |
| (self-test/unittest) | verify_goal --self-test 42/42;unittest discover 66 tests | exit 0 |

clippy 余量仍为基线集(mux.rs:765 Batch、control.rs:582 inner_ref、
main.rs:83、ws_mux_repro.rs:221、vendor stream.rs:291)。

## 10. 追加轮 3(2026-10-13 夜):克隆+全局时间平移绕过修复(独立审查三轮 [中])

### 10.1 缺陷与修复(verify_goal.py r5)

独立审查三轮实验 D 实证:r4 的 `cross_dir_independence` 只查同 key 时间窗
**重叠**,不锚定绝对时间——每目录克隆 2 份并把 results 窗口/raw 时间戳/
cpu.jsonl t_us 全部机械平移 +24h/+48h 后,克隆目录两两不重叠 → 修前总体
PASS(1 次真实测量冒充 3 次独立)。r5 三道锚(全部基于已文档化的执行规程,
README:201-203「换协议/MUX/RELAY 须重启两端、用**新的 PEER_RUN_ID**另开
目录」):

1. **绝对时间锚**(`EvidenceDir._check_time_anchor`):peer.sh 在下发测量前
   写 `meta.started_at`(drive 同机时钟)。全部有效吞吐/UDP/并发窗口必须
   落在 [started_at−300s, started_at+6h];started_at 缺失 = 无法锚定,
   fail closed。+24h/+48h 平移后窗口越界 → FAIL。
2. **run_id 全局互异**(cross_dir_independence r5-1):两个合格目录同
   run_id = 同一轮被克隆(规程要求每目录新 PEER_RUN_ID)。锚窗内小平移
   (+20min)且不与兄弟目录重叠的克隆由它抓住。
3. **跨配置窗口互斥**(r5-2):同一三机拓扑上任何测量都串行执行(换协议
   需串行重启)——全部合格目录的全部测量窗口两两不重叠,不同配置目录
   重叠即 FAIL。

配套:fixture `started_at` 改为本目录时间线基点推导(原静态
2026-10-13 与 FIX_BASE_US 的 2027-01-15 不一致,r5 锚会误伤好集);
`build_good_set` 各协议目录时间线串行错开(1h 间隔,镜像真实串行执行)。
检查次序:窗口类证据先落档、run_id 互异最后(克隆场景同时留下
「窗口重叠/越界」与「run_id 重复」两类证据)。

**残余(如实)**:全知伪造者把 started_at 与全部时间戳一致平移+改写
run_id 时,离线核验器无可信时钟源无法绝对免疫(审查原话);此时攻击等价
于从零伪造整套自洽证据。G4/G5 自报告证据强度与配置回读两条结构性限制
沿用上轮记录,未变。

### 10.2 新增回归

self-test 42→**47/47**:克隆+锚外平移(保 run_id)、克隆+锚窗内平移
(run_id 互异)、克隆+平移+改写 run_id(started_at 锚)、started_at 缺失、
started_at 置后(晚于窗口)——即审查实验 D 及其强化形态全部转负例。
unittest 66→**71 全 OK**(同五例镜像,`_shift_dir_time` 平移 results 窗口/
raw timestamp/cpu.jsonl 三类文件,与审查实验 D 同攻击面)。

### 10.3 命令证据(`benchmark/runs/goal-repair/relay/integration/`)

| 日志 | 内容 | 结果 |
|---|---|---|
| 38-green-release-e2e-rust | release 重编 + e2e-relay-modes + 全 Rust 套件 + workspace check + fmt | e2e 14/14;26+9+1+1/11/6/15/20 全 ok;exit 0 |
| 39-verifier-selftest-r5 | verify_goal --self-test | 47/47,exit 0 |
| 40-benchmark-unittest-r5 | unittest discover | 71 tests OK,exit 0 |

e2e 说明(独立审查一轮注):脚本断言服务端 `rabbithole_relay_path_total`
计数;客户端侧 B7 门控(上轮 client_chunk_gate)无计数器可观测,由
`forward::tests::relay_mode_copy_disables_client_chunk_gate`(真实函数锁)
覆盖,本轮随 rh-client 6 项测试绿。本轮无 Rust 行为改动(仅验证器/测试)。

## 11. 追加轮 4(2026-10-13 深夜续):round-1 判定后的 yamux 初窗预授信修正

背景:round-1 真实验收总体 FAIL(G1 tcp-nomux 单流 1951/2047<3000、
G2 tls-mux 1225/1784<2500、G6 25/36 档保留率<80%;G3/G4/G5/G7 PASS,
证据 `benchmark/runs/goal-repair/round-1/`)。round-2 窗口预授信初版被
主代理否决,本轮按其 5 点反馈修正。**本改动只覆盖 yamux(mux 路径),
不声称修复 nomux(G1)/QUIC/KCP(G6)。**

### 11.1 反馈逐项处理

1. **挂起测试(反馈 1)**:修前 `mux_initial_window_pregrant_unlocks_single_stream`
   先 `open_stream` 再 `accept`,而 yamux SYN 是 lazy 的(`Flag::Syn` 随首个
   外发帧置位,vendor stream.rs:82/211-216),accept 永久 Pending——
   `01-red-initial-window.log` 实况仅 "running for over 60 seconds",
   **无任何 262144 断言输出**,原注释「断言 FAILED」不实,已纠正。修后:
   先写 1 字节驱动 SYN 再 accept;全流程 30s deadline(挂起=确定失败);
   真红取证 `02-red-initial-window.log`(对 upstream 无预授信实现恒
   written=196608=3×64KiB,FAILED exit=101,2s 内)。残留测试进程
   (PID 6367 挂起二进制/6350 cargo,命令行与 window-pregrant 日志路径
   逐字匹配)已核对身份后 kill,复检无残留。
   **判据重设计(实测驱动)**:原「接收端消费 1MiB 后停读」形态在本地
   duplex 上不可区分修前/修后——RTT 测量连接即启动(connection.rs:427
   首个 poll 即发 ping),回路时延 < 2×RTT 时 BDP 自增窗合法触发,实测把
   无预授信形态也抬到 written=1.75MiB(该次运行为本会话实测,日志已被
   单次读设计的正式红取代)。改为**单次读**:接收端只弹随 SYN 到达的
   1 字节即永久停读——全程唯一窗口更新=预授信 WU,自增窗永不介入,
   判据确定性:修前恒 196608,修后恒 2031616(31×64KiB),与计时无关。
2. **预算/溢出/守恒/倍增(反馈 2)**:
   - `FlowController::new` 预授信先吃连接预算剩余额度
     (`max_connection_receive_window − 256KiB×max_num_streams − 已占用`,
     全程 saturating),额度不足退回规约保底 256KiB;不再无条件入账
     (修前 `next_window_update` 连接限额原生减法会下溢——debug panic/
     release 回绕)。`Drop` 按 (max−DEFAULT_CREDIT) 等额扣回,守恒由
     `pregrant_respects_connection_budget` 锁定(流释放→额度回收→新流复用)。
   - u32/usize 溢出组合:`connection_budget` 与 lib.rs 两个 setter 的
     `256KiB×max_num_streams` 全部 saturating(max_num_streams=usize::MAX
     时乘积封顶、预算归 0、退保底,不 panic 不回绕);
     `pregrant_overflow_combinations_saturate` 锁定。
   - **首 WU 不触发 BDP 倍增**:新增 `pregrant_pending`——预授信差额
     授出前 `last_window_update` 语义是「建流时刻」,elapsed≈0 < 2×RTT
     恒真,修前首个 WU 即按假速率翻倍(实测 3997696=翻倍后额度 vs 应
     1900544);修前差额授出(首个 Some 更新)后 flag 清零,此后真实
     消耗驱动的更新照常自增。`first_pregrant_window_update_does_not_double`
     锁定(含「机制未关死」的后继断言)。
   - 4096 流不受控预留:mux.rs 接线从 `set_max_connection_receive_window(None)`
     改为 `Some(256KiB×流数 + 256MiB 共享池)`——超出规约保底的信用总额
     有界(256MiB ≈ 4096 流保底敞口 1GiB 的 1/4),预授信与既有自增窗
     同池竞争;保底每流 256KiB 仍由 yamux 断言保证。注意 setter 相互
     断言当前值,必须先窗口后流数(反序在默认 1GiB 窗下必炸,已注释)。
   - 旧端混用:发送侧零改动(`new_outbound` send_window 仍 DEFAULT_CREDIT,
     stream.rs:170-172 未动),预授信只经标准 WU 帧授出(入站流 ACK-on-WU
     为 upstream 既有合法组合);pregrant 测试本身即「新接收端+发送侧
     不变」的行为级混用证据。
3. **证据/假设分离(反馈 3)**:mux.rs/lib.rs/测试注释全部改标——
   【实测】round-1 单流 1051-1225/1468-1962 Mbps、16 流 2160-2970(来源
   `round-1/single-stream-medians.txt`);【假设,未闭合】信用回路时延
   > 2×ping RTT 致自增窗不触发(「25Gbps loopback」「回路 1-2ms」等
   无留痕数字已删除或改为待测项);合成 RTT 实验
   `mux_single_stream_credit_stall_under_synthetic_rtt`(#[ignore])本地
   复现钳制机制:RTT≈2ms 下载前 196608 → 修后 2031616,每轮钳制算术
   256KiB/2ms≈1049 Mbps 与测试床上行同量级(明示为一致性佐证非因果证明)。
4. **下一轮装置要求(反馈 4,记录待执行)**:①relay path 指标落盘——
   新流必须确认命中 `rabbithole_relay_path_total` 路径计数(服务端已有,
   客户端 B7 门控无计数器,需在下一轮三机运行时双端实际核验);
   ②COPY 开关两端核验(relay_mode=copy 时服务端 assign_forward 与客户端
   client_chunk_gate 都要观测到 path=copy);③实测测试床 ping RTT 与
   信用回路时延,闭合/否证「回路时延 > 2×RTT」假设;④读/writev batching、
   splice ready loop 等方向按实测选,单轮只留可归因变化。
5. **定向交付(反馈 5)**:未跑全 workspace 套件(脚本职责);本轮定向
   `cargo test -p yamux`(23 passed)、`cargo test -p rh-transport`(5 目标
   全 ok)×4 次、`cargo check -p rh-server -p rh-client` exit 0、
   rustfmt 归一;红绿取证全部真实退出码。

### 11.2 命令证据(`benchmark/runs/goal-repair/window-pregrant/`,README 有全表)

| 日志 | 内容 | 结果 |
|---|---|---|
| 01-red-initial-window.log | 修前测试缺陷历史留痕(挂起,无断言输出) | 挂死 60s+(不覆盖) |
| 02-red-flowcontrol-budget.log | 状态 B(评审起点实现):3 项新单元回归 | 全 FAILED,exit=101 |
| 02-red-initial-window.log | 状态 A(upstream 无预授信):written=196608 | FAILED,exit=101 |
| 02-red-credit-stall.log | 状态 A,RTT≈2ms 合成载体:written=196608 | FAILED,exit=101 |
| 03-green-yamux.log | `cargo test -p yamux`(含 3 新回归) | 23 passed,exit=0 |
| 03-green-rh-transport.log | `cargo test -p rh-transport` 全目标 | 全 ok,exit=0 |
| 03-green-initial-window.log | written=2031616(=31×64KiB) | ok,exit=0 |
| 03-green-credit-stall.log | RTT≈2ms:written=2031616 | ok,exit=0 |

稳定性:goal_repair_mux 全文件连跑 3 次全绿(2.01s/次,无挂起)。

### 11.3 假设与证伪方法(本轮新增假设的全部清单)

| 假设 | 依据 | 证伪方法 |
|---|---|---|
| 测试床 mux 单流卡在信用回路(窗口/回路时延)而非每字节成本 | round-1 单流与 16 流差、TLS 无关;本地合成 RTT 复现钳制机制 | 下一轮实测:测试床 ping RTT + 回路时延;若预授信后单流仍 ~1.1G,假设否证,归因转向别处(如载体唤醒/调度) |
| 256MiB 共享池足够 16 流自增 | BDP 算术(16×~8MiB=128MiB<256MiB) | 下一轮 G6 16 流若较 round-1 回退,池不足,需调大或按流数缩放 |
| 预授信不恶化 G5(4096 流 RSS/verified) | 窗口是信用限额非分配;4096 流仅 ~149 流获全额预授,其余保底 | 下一轮 G5 RSS 对比 round-1(110-310MiB) |
| 首 WU 不倍增更正确 | 倍增判据本意是实测吞吐驱动 | 无直接外部观测;由单元回归语义锁定 |

### 11.4 未尽事项(如实)

- 三机下一轮运行、G1-G7 复测、relay path/COPY 双端核验:性能工程师统一
  执行,本轮未连远端。
- 全 workspace test/clippy:脚本 `world.run` 统一执行(本机定向已过;
  `Batch(usize)` dead-code 等 4 条基线警告为本轮之前遗留,未动)。
- G1(nomux)/G6 QUIC、KCP 档:本轮无改动,差距仍在(见 round-1 判定)。

## 12. 追加轮 5(2026-10-14):round-2 判定后——基于诊断的裁定、本地仿真与升级

背景:round-2 验收总体 FAIL(G1 1943/2113<3000、G2 1172/1752<2500、
G6 **25/36** 档 <80%——两轮均 25,round-2 报告"24/36"为叙述错误,以
验收器 25 条 FAIL 行为准;真实翻转仅 wss/d16 79.1%→88.0% PASS 与
tls/u4 87.3%→72.1% FAIL)。三机性能工程师完成有界定位
(`benchmark/runs/goal-repair/diagnosis-after-round-2/`,2.0MB 全留痕),
本轮按主代理指示:读原始证据、只做数据支持的改动、预备授信裁定、
可升级则明确升级。

### 12.1 诊断证据核验(本会话重读原始文件,非转述)

- 单链路基线:9 组×3 全 raw,中位 2956-3327 Mbps
  (analysis.txt:1-7,抽查 t3/out/refrelay-*.json 与报告一致)。
- 最小对照 refrelay(读源 refrelay.rs:公平——双向各一线程、64KiB
  copy / 256KiB splice、半关闭透传):两跳 copy 2015/2003、
  splice 2026/1992,与 rh tcp-nomux 同窗 1920/2010 无差异。
- 同窗 ss/线程 CPU/ethtool/UDP 计数如报告;rh binary 身份核验过
  (/proc/PID/exe)。pidstat/perf 缺失已如实声明。
- **硬事实:两跳中继(全部实现,含 128 行最小参照,含 copy 与
  splice)单流天花板 ~2.0-2.2G,单链路 2.9-3.5G。**

### 12.2 预授信(round-2 修复)裁定:**保留,但不再作为吞吐手段**

- 单流未兑现(诊断 + 本轮复核:tcp-mux u1 +8.7%、tls u1 −4.3%,
  均噪声级)→ 信用回路确认不是单流瓶颈,后续不再调窗。
- 保留依据:多流族稳定增益(wss u16 +16.4%/d16 +22.1% 且 d16 翻
  PASS、tcp-mux d16 +12.4%、ws d16 +6.5%;单流族无损害——tls/ws
  −1~−4% 噪声内);内存有界(256MiB 共享池,G5 RSS tcp-mux
  213→174-175MiB 反而改善);线路兼容(仅标准 WU 帧)。
- 风险声明(如实):kcp 1/4 流跨轮回退(u1 186→116、u4 210→117、
  d1 183→105,16 流持平)——与预授信同轮落地,但 kcp 载体自身
  ~120Mbps + ACK 风暴(11.3k/s)下样本波动大(round-1 u1 样本
  [118,186,214]),因果未证。下一轮远端 A/B(mux=1 kcp、初窗
  256KiB 对照)判别;若证实回退,回退路径 = mux.rs 一行
  `set_initial_stream_receive_window(256*1024)`,全部缺陷修复
  (预算钳制/首 WU 不倍增/溢出饱和)保留且在 256KiB 下自然惰性。
- 兼容性:旧端混用无新帧型;预算钳制对新配置也安全(见 §11)。

### 12.3 本地两跳仿真(新增诊断,证据 benchmark/runs/goal-repair/local-twohop-emulator/)

新增 `two_hop_relay_upload_emulator`(#[ignore],tests/goal_repair_mux.rs):
duplex 4MiB/跳 + 200µs 双向延迟泵,copy(refrelay 同形)与
mux(生产 mux_relay)两种中继同路径 A/B、双向、全量字节校验。
发现(全部留日志):

1. **release 下 mux 路径无 ~2G 内在上限**:0.4ms RTT 双跳 9.1-9.8Gbps、
   无延迟 20.4Gbps;debug 构建伪上限 2.2G(01 vs 02 日志)——
   剖析必须 release,留档为方法论教训。测试床 mux 下行 2110 ≈ 环境
   天花板,非 mux 软件上限。
2. **测试床上行方向不对称(72.7% vs 105%)不能由纯用户态节律复现**:
   本地四轮 mux/copy 比 up 82.7-94.5% / down 91.2-116.6%,无稳定
   不对称 → 缺口主要来自与真实 TCP 发送侧排队/拥塞的交互,
   判别实验需远端 RELAY=copy A/B。
3. **无单点热点**:sample 9s 叶帧 kevent 3077/等待 4678/memmove 1423
   (~15.5%),memmove 为最大叶帧(搬运固有拷贝),无病态函数。
4. **不交付臆测优化**:候选(满尺寸 chunk 免拷贝直写)预期 ≤1-3%,
   小于仿真器同码逐轮方差(82.7↔94.5%),不满足「微基准证明变化」
   标准,本轮不实施、不声称。

### 12.4 升级(硬数据,请主代理裁定)

G1/G2/G6 门槛全部高于两跳环境天花板:

| 项 | 门槛 | 两跳天花板(实测) | 缺口 |
|---|---|---|---|
| G1 tcp-nomux 单流 | 3000 Mbps | ~2.0-2.2G(所有实现) | −27~−50% |
| G2 tls-mux 单流 | 2500 Mbps | ~2.0-2.1G(下行已达 1752-1980) | −20%+ |
| G6 1 流档 80% | 隐含 ≥2.4-2.6G | ~2.0-2.2G | 不可达 |

软件侧(红线内:不动 sysctl/NIC/SO_*BUF)不可达;环境级出路
(iperf3 -w 显式窗口实验、TCP 缓冲/节律调优、拓扑/目标重定标)超出
本轮授权,属用户决策。剩余软件边际 = mux 上行缺口 ~0.5G(诊断项②,
本地仿真表明非纯每字节成本),即使全部回收也不翻转任何门槛。

### 12.5 下一轮远端实验清单(按判别力排序,性能工程师执行)

1. RELAY=copy 对照轮(mux=1 tcp/tls,独立目录)——判别 mux 上行缺口
   属 chunk 路径还是 TCP 交互(B7 装置已支持同语义 A/B)。
2. kcp 预授信 A/B(初窗 256KiB 对照)——判别 kcp 1/4 流回退因果。
3. iperf3 -w 256K/512K 跑 refrelay 两跳 + copy 块 64KiB→256KiB 对照
   (仅 iperf/探针参数,不动 sysctl)——判别两跳天花板机理
   (缓冲深度节律 vs 批量)。
4. tls 下行窗口 t2 侧线程 CPU 对称采样——判别上行 600Mbps 差。
5. relay path 计数双端核验(rabbithole_relay_path_total / 客户端
   B7 门控观测)+ COPY 开关双端实际生效确认(上轮遗留要求)。

### 12.6 本轮改动与回归

- 生产源码零改动(预授信保留原样;无臆测优化)。
- 新增:仿真器 + 证据 7 份(01-07 + README,真实退出码);
  fmt 过;`cargo test -p rh-transport` 5 目标全绿(2 个 #[ignore]
  诊断为显式运行已留档)、`cargo test -p yamux` 23 全绿;
  全 workspace 套件按约定未跑(脚本职责);未连远端。

## 13. 追加轮 6(2026-10-14):targeted-before-round-3 判别实验裁定——生产源码不变

输入:`benchmark/runs/goal-repair/targeted-before-round-3/`(REPORT.md +
analysis.txt + 95 个有效 JSON raw;本会话已抽读并从 t3/out 原始 JSON
重算 copy/chunk 与 rr2 全档中位数,与报告逐项一致,含 copy 档大方差
样本 [1122,1805,2351])。

### 13.1 diagnostics→change 因果与处置表(本表即本轮裁定依据)

| 诊断项 | 因果链 | 预期收益 | 处置 |
|---|---|---|---|
| copy vs chunk 无差异(tcp ±3-6%,tls 同;chunk 样本方差反而更紧) | chunk 路径非两跳单流瓶颈来源 | 无 | **chunk 保留,零改动**(同二进制 copy 不优于 chunk,按指示不以猜测删改;正确性/计量路径不动) |
| refrelay2 +TCP_NODELAY(3v3 样本上行完全不重叠 1980→2085,下行全重叠无效应) | Nagle 与中继写节律交互拖慢上行 | up ~+5% | **rh 已全量设置,无需改动**:server 代理口 accept(data.rs:885)、client 目标 dial(forward.rs:229)、vhost http/https(vhost.rs:21/470)、代理 dial(proxy_dial.rs:122)、TCP 载体(tcp.rs:51/266)——rh 本就处于"+NODELAY 档",该收益已含在现有数字中 |
| refrelay2 +NODELAY+LOWAT64K vs +1MiB(up 2192/2191 vs 2074,样本基本分离;down 2218 vs 2178 重叠) | NOTSENT_LOWAT 档位影响背压回传节律 | up ~+5%,down ~+2%(未证) | **不改代码**:splice 路径已有 RH_NOTSENT_LOWAT 环境变量(splice.rs:37-45,默认 1MiB);按实验报告以 env 矩阵下轮远端验证后定,避免抢跑改默认值 |
| mux/copy 路径 socket 无 NOTSENT_LOWAT(SpliceSock::dup 仅 splice 路径) | 无对应 A/B 数据(rr2 各档为 splice 模式) | 未知 | **不实施**:无对照证据即属臆测(若下轮远端 env 矩阵在 nomux 上证实 64K 档收益,再评估是否扩展到 mux 路径并补测试) |
| iperf -w 窗口决定性(direct 2911→1054@64K;两跳同比例);压小基线不改善保留率(78%反而更差) | 窗口/缓冲深度是吞吐主参数 | — | 参数属 iperf 端点;rh 侧无线索;-w≥256K 被 wmem_max=212992 clamp,解锁=用户 sysctl 决策(收益未证明,仅解锁未测空间) |
| tls 下行 t2 加密端 86% 单核(3×29%) | TLS 单流双向均 CPU 重载 | — | 无具体热点定位(三机无 perf/pidstat);无可证伪软件改动 |
| quic USO off [fixed] / kcp ACK 风暴 | 平台/协议栈特性 | — | 用户环境决策(虚拟网卡类型、平台) |

### 13.2 本轮结论(如实)

1. **生产源码零改动**:所有有明确对照证据的档位,rh 要么已具备
   (NODELAY 全量、splice 路径 LOWAT),要么实验报告明确指定以既有
   环境变量在下一轮远端验证(LOWAT 64K 矩阵)——按指示不伪造一次
   "优化交付"。chunk 路径经同二进制 A/B 证明与 copy 等价且方差更稳,
   保留。
2. **量化剩余空间**:红线内可改项(NODELAY 已含 + LOWAT 64K 若证实)
   叠加预估 ≤+15%;从 ~2.1G 到 G1 的 3000 需 +43%——红线内不可达。
   措辞遵循实验报告:已证伪的是"探针允许的选项档内节律无法显著超越
   ~2.2G",未测项(socket 缓冲显式扩档被红线禁止、-w≥256K 被系统
   wmem_max clamp)不宣称"环境硬上限"。
3. **需用户授权的环境决策(有据,非本角色可执行)**:
   a. sysctl net.core.rmem_max/wmem_max(默认 212992)解锁 -w≥256K
      与发送缓冲上界(提高后收益未证明);
   b. vmxnet3 tx-udp-segmentation off [fixed] → quic UDP 批量发送受限,
      换虚拟网卡类型属虚拟化平台决策;
   c. 测试床拓扑变更(专用性能虚机/物理机)则两跳节律与 NIC 特性
      需重评。
4. 下一轮远端(性能工程师独占):RH_NOTSENT_LOWAT=65536/1048576/0
   矩阵(nomux+tcp-mux 各一轮,其余参数不动,单变量归因)——这是
   当前唯一有直接对照证据支撑的待验证生产参数;结果回填本表后再定
   是否改默认值(届时补针对性测试)。

### 13.3 本轮改动清单

- `docs/performance-repair-status.md`(本节):裁定与因果记录,
  **非生产修复**。
- 无其他文件改动;未跑重复套件(生产源码未变,上轮定向回归仍有效:
  rh-transport 5 目标/yamux 23 全绿留档于 local-twohop-emulator/07);
  未连远端、未动 sysctl/网卡/SO_*BUF、未删测试未降阈值。

## 14. 最终报告(2026-10-14,追加轮 7:仅报告,零源码/零测试改动)

> 本节为全部修复轮(§1-§13)的收口报告:请求→修复→测试→命令退出→原始
> 证据的逐项映射、本地与 Linux 覆盖划分、六协议逐门槛真实结果(round-2)、
> 未解决项、无提交/无推送与无环境遗留证据。**结论:G1/G2/G6 未达标,
> 总体验收 FAIL,不能声称性能达标;亦不声称绝对无缺陷。后续以主代理
> 亲自复核为准**(见 §14.7)。本轮除本文档外无任何文件改动
> (`git status` 仅新增本文档差异)。

### 14.1 请求→修复→测试→命令退出→原始证据 总映射

| 轮(请求) | 修复(对应章节) | 关键测试名(全部修前红→修后绿) | 命令退出 | 原始证据文件 |
|---|---|---|---|---|
| 集成三交付 + live-review 11 项 + 故障注入(§1-§6) | relay 取消/错误/panic 收敛、QuotaGuard/prefix、ChunkLimiter 接线、pool 口径、read_chunk cap、e2e-relay-modes.sh 新建 | T1 `mux_relay_cancel_converges_when_close_queue_full`、T2 `mux_relay_error_path_converges_when_shutdown_pends`、T3 `mux_relay_sibling_aborted_on_pump_panic`、T4 `copy_relay_panic_contained`、T5 `client_copy_panic_contained`、T6 `read_chunk_from_cap_lock`、T7 e2e B7 门 | 红注入 exit 101 ×6、e2e 红 4 FAIL exit 1;绿:transport/server/client/core/yamux/release/check/clippy/fmt/self-test 37/unittest 60 全 exit 0 | `relay/integration/01-04`(红)、`05-23`(绿);日志尾部 `RETURN_CODE=` 行(runner `relay/rule-cancel/run_with_deadline.py`) |
| 主代理定向修复 A(prefix 回落)+ B(cancel-after-eof)(§7) | `mux_steal_or_rewrap` 回包保序、`close_with_late_cancel`/`shutdown_with_late_cancel` 收尾监听取消(clean EOF 仍无界) | `nonmux_fallback_preserves_prefix_and_guard`、`nested_prefix_rewrap_preserves_order`、`mux_relay_cancel_after_eof_converges`、`copy_rule_cancel_after_eof_converges`;既有 cancel-before-shutdown 两例保持绿 | 红 26/27/24/25 exit 101;绿 28/32 全套 exit 0;主代理 harness(源码原样重编译,断言零改动)5 例全 RC=0(29) | `relay/integration/24-32`;主代理复现 `cancel-after-eof-before.log`(exit 101)与其 harness 源 `/tmp/rh-main-independent-review/src/main.rs` |
| 全套脚本暴露问题(verifier r4 + R1/R2/R3)(§9) | 克隆目录 G7 绕过、UDP/concurrent 查重、schema run_id、CPU fail-closed;客户端 chunk 门控、退款断言、pool_repro 重写 | self-test 负例(整目录克隆/UDP 复制/concurrent 复制/目录混装)、`relay_mode_copy_disables_client_chunk_gate`、`mux_rate_wait_cancel_refunds`(`assert_eq!(refunded,1)`)、`goal_repair_pool_repro` 公共入口上限 | 红 34/35/36 exit 101;绿 33/37 exit 0;self-test 42/42、unittest 66 全 exit 0 | `relay/integration/33-37` |
| 克隆+全局时间平移绕过(verifier r5)(§10) | 绝对时间锚(started_at)、run_id 全局互异、跨配置窗口互斥 | self-test 47/47(含实验 D 三形态负例)、unittest 71(`_shift_dir_time` 镜像攻击面) | 38/39/40 全 exit 0 | `relay/integration/38-40` |
| round-1 判定后 yamux 初窗预授信(§11) | FlowController 预算钳制/saturating/pregrant_pending 防倍增;2MiB 单流预授 + 256MiB 共享池接线 | `mux_initial_window_pregrant_unlocks_single_stream`(单次读判据:修前恒 196608/修后恒 2031616)、`pregrant_respects_connection_budget`、`pregrant_overflow_combinations_saturate`、`first_pregrant_window_update_does_not_double`、`mux_single_stream_credit_stall_under_synthetic_rtt`(#[ignore] 诊断) | 红 02 系列 exit 101;绿:yamux 23、rh-transport 全目标 ×4 exit 0 | `window-pregrant/01-03`(8 份日志 + README) |
| round-2 判定后:诊断裁定 + 本地两跳仿真(§12) | **生产源码零改动**(预授信保留但不再作吞吐手段;不交付臆测优化) | `two_hop_relay_upload_emulator`(#[ignore],copy/mux 同路径 A/B) | 仿真 01-07 真实退出码;rh-transport/yamux 定向绿 | `local-twohop-emulator/`(9 份)、`diagnosis-after-round-2/`(2.0MB) |
| targeted-before-round-3 判别实验裁定(§13) | **生产源码零改动**(NODELAY 已全量、LOWAT 待远端 env 矩阵;chunk 保留) | 无新增(生产未变) | 定向回归沿用上轮绿档 | `targeted-before-round-3/`(REPORT.md + analysis.txt + 95 raw JSON) |
| 本轮(追加轮 7) | 仅本报告 | 无 | 验收器复跑 RC=1(见 §14.4);无测试执行 | 本节 + `checks/final-*`(脚本终检 result.json) |

脚本终检(本轮事实输入,`benchmark/runs/goal-repair/checks/` result.json
逐一核读):`cargo test --workspace --locked --offline` exitCode **0**、
`cargo clippy --workspace --all-targets --locked --offline` exitCode **0**、
`verify_goal --self-test` exitCode **0**(47/47)。

### 14.2 本地(macOS)与 Linux 覆盖划分(如实)

**本地已执行**:全部定向 Rust 回归(debug/release)、yamux 套件、
`cargo check/clippy/fmt`、e2e-relay-modes.sh macOS 部分(14/14,Linux 非
mux 轮显式跳过并打印)、验证器 self-test/unittest、脚本终检三件(上表)。

**Linux 已执行(三机验收链,round-2,ROUND2-REPORT 构建节)**:test1 上
`cargo test --workspace` exit=0(13 目标一次通过)、`cargo build --release`
exit=0、build_manifest 73 文件 SHA256 与工作区逐条一致、7 处
`/proc/PID/exe` 核验——**Linux 编译/测试面已由三机链覆盖**。

**仍未覆盖(如实保留)**:①e2e-relay-modes.sh 的 Linux 非 mux 轮
(path="splice" 断言)未在任何 Linux 机执行(三机验收走 benchmark
harness,非该脚本);②本机 musl 交叉 `cargo check --target
x86_64-unknown-linux-musl` 因缺 `x86_64-linux-musl-gcc` 失败(30- 日志,
环境缺件,非代码错误);③`two_hop_relay_upload_emulator` 为本地仿真,
不替代三机实测。

### 14.3 六协议 × 性能门槛(round-2 真实数字,验收器判定)

判定命令(ROUND2-REPORT):`RH_VERIFY_REPO=<工作区> python3
benchmark/py/verify_goal.py --evidence benchmark/runs/goal-repair/round-2`
→ **总体 FAIL(退出码 1)**。逐门槛:

| 门槛 | 结果 | 数字(n=3 中位) |
|---|---|---|
| G1 TCP nomux 单流双向 ≥3000 + CPU<50% | **FAIL(吞吐)/CPU 全过** | up 1943 / down 2113;CPU 28.14/33.5/26.37/14.71% |
| G2 TLS mux 单流双向 ≥2500 | **FAIL** | up 1172 / down 1752 |
| G3 六协议单流正吞吐 | **PASS** | tcp-mux 1197/1885、tls 1172/1752、ws 999/1942、wss 1101/1481、quic 569/601、kcp 105-116 |
| G4 1200B UDP 损失 <1% | **PASS** | 六协议中位 0%(drain 5s) |
| G5 mux=1 各 4096 verified + tcp 对照 | **PASS** | 六协议 + 对照全部 4096×3 verified,alive_by_round 逐轮核对;RSS 峰值 server ≤309MiB(quic)/client ≤182MiB |
| G6 逐协议×方向×流数保留率 ≥80% | **FAIL:25/36 档 <80%** | 详表如下(ROUND2-REPORT 叙述"24/36"为笔误,以验收器 25 条 FAIL 行为准) |
| G7 ≥3 配对中位数、样本全留痕 | **PASS** | 无效样本 0,raw 全留痕 |

G6 逐档(round-2 `single-stream-medians.txt`,r2 保留率为 **medians 口径**
= tunnel 中位/direct 中位之比,与验收器逐对口径不同,见下注;G6 的
36 档 = 六个 mux=1 协议 × 2 方向 × 3 流数,tcp 行即 tcp-mux;tcp-nomux
行为 G1 参照信息,不计入 G6):

| 目录 | u1 | d1 | u4 | d4 | u16 | d16 |
|---|---|---|---|---|---|---|
| tcp-mux | 37.5%✗ | 57.4%✗ | 66.3%✗ | 92.0%✓ | 92.3%✓ | 102.6%✓ |
| tls-mux | 37.6%✗ | 55.5%✗ | 72.1%✗ | 91.2%✓ | 105.9%✓ | 105.4%✓ |
| ws-mux | 30.1%✗ | 60.8%✗ | 82.0%✓ | 108.5%✓ | 86.8%✓ | 114.3%✓ |
| wss-mux | 41.0%✗ | 45.4%✗ | 47.4%✗ | 70.8%✗ | 79.7%✗ | 85.3%✓ |
| quic-mux | 19.3%✗ | 19.8%✗ | 20.5%✗ | 32.2%✗ | 19.2%✗ | 24.9%✗ |
| kcp-mux | 3.7%✗ | 3.0%✗ | 4.1%✗ | 4.4%✗ | 4.3%✗ | 5.6%✗ |
| (tcp-nomux,G1 参照) | 63.2% | 70.2% | 77.6% | 77.3% | 105.1% | 117.9% |

(✗=<80%,六 mux 行合计 3+3+2+5+6+6 = **25 档**,与验收器 25 条 FAIL
行一致。**口径说明(勘误:先前版本误称"四舍五入差异",本节改正)**:
本表保留率 = 中位数之比,验收器 = 逐对保留率(tunnel/direct 逐对配对)
的中位数——两者是**不同统计口径,非四舍五入关系**,同一档差可达
~2.7pp(实例 wss/d16:medians 口径 85.3% = 2321/2721,验收器口径
88.0%,后者来自 round-2 复核事实,验收器报告只打印 FAIL 档数值);
两口径下各档判定一致(wss/d16 均 PASS、wss/u16 均 FAIL),G6 失败档数
均 25。直连基线本身两轮 2686-3474,部分轮次 direct 单流亦 <3000。)
差距归因与红线内不可达论证见 §12.4/§13.2:两跳环境天花板
~2.0-2.2G(全部实现含 128 行参照),G1 需 +43%;环境级出路(sysctl
rmem/wmem、vmxnet3、拓扑)属用户决策。**本报告不将环境未调优表述为
"已达标"或"无法达标",只陈述:在授权红线内未达标。**

### 14.4 验收器复跑与存储报告差异(本轮新核验:主要差异已定位,残差未全部解释)

本会话以最终工作区复跑 `verify_goal.py --evidence
benchmark/runs/goal-repair/round-2`(两种形态,真实退出码):

1. 不设 `RH_VERIFY_REPO`:7 目录全部"无法定位源码树"fail-closed,
   RC=1(验证器正确行为)。
2. 设 `RH_VERIFY_REPO=<最终工作区>`:RC=1(总体 FAIL 与存储报告一致),
   但 7 目录均报"源哈希不符:`crates/rh-transport/tests/goal_repair_mux.rs`
   (清单 2669ab91…,当前 2f232358…)",级联使 G1-G6 明细行变为
   "无有效配对样本/配对 0 < 3"。

**主要根因**:round-2 验收后,§12 向该测试文件追加了 #[ignore] 诊断仿真器
(`two_hop_relay_upload_emulator`)——生产源码零改动(§12.6/§13 逐轮
声明并经脚本终检复核),但源-二进制绑定对**测试文件**同样敏感,复跑
自然不再逐行等同存储报告。含义(如实):round-2 性能证据绑定 round-2
时点源树;最终树与之差异仅为该诊断测试文件;下一轮验收须整体重跑以
重建绑定,不得把 round-2 数字当最终树的达标证据引用。
**残差未全部解释(如实,不宣称"根因已定位"的完整性)**:①复跑输出
还含存储报告没有的 G7 口径行(kcp 等档"配对 0 < 3(G7;直连基线无效
也算配对缺失)"——本会话 diff 实见,超出上文"G1-G6 明细级联"所述
范围;存储报告同位置 G7 段为 PASS 且无此细节行);②存储报告的生成
时刻与所用验证器版本未核实(疑似旧版验证器生成,未确认)。两项均不
改变两态总体判定一致(FAIL/RC=1)与"下轮整轮重跑"的保守处置。

### 14.5 未解决项(全部如实,含独立审查标记)

1. **[高·计划前置缺口] ws.rs §6b R5 锁测试缺失**:
   `docs/performance-repair-acceptance.md:366-371` 要求对
   `send_queued` 状态机与 `from_partially_read` 前缀语义的针对性测试;
   实际 `ws.rs` 测试仅 `ws_handshake_and_stream`、`ws_wrong_path_rejected`
   两项(本轮 grep 核实)。**本报告不声称 §6b 全部通过**;ws.rs 状态机
   本身零改动(红线),缺口属测试覆盖,待后续轮补齐(须修前红取证)。
   **顺序偏差(此处补披露)**:验收方案前置条款(performance-repair-
   acceptance.md:29「§6a 缺陷回归与 §6b 不变量锁全部通过,才允许进入
   性能验收」)未满足——round-1 与 round-2 两轮性能验收均在本前置
   缺口存续状态下执行;§14.3 的性能判定因此属"前置未闭合下的实测
   结果",验收流程合规性由主代理复核裁断。
2. **G1/G2/G6 未达标**(§14.3):红线内软件边际 ≤+15%(§13.2),
   不翻转任何门槛;环境级决策(sysctl/网卡/拓扑/目标重定标)待用户。
3. **红线-3 grep 现状与 §5 历史记录不一致**:对 `crates/ vendor/` 全树
   `.rs` grep `SO_RCVBUF|SO_SNDBUF|set_recv_buffer_size|
   set_send_buffer_size` 现为 **exit 0、命中 1 行**——`splice.rs:888` 注释
   ("不为制造背压触碰任何 SO_RCVBUF/SO_SNDBUF(红线)")。该命中是
   **描述红线的注释,非 API 调用**;全树无任何 set_*_buffer_size 调用,
   功能红线未违反。§5 的"exit 1(0 命中)"为当时树状态,以本节现状为准。
4. **ROUND2-REPORT.md 叙述错误**(不改动该历史文件,以验收器为准):
   :19 "24/36 档"应为 **25/36**(verify-goal-report G6 段 25 条 FAIL);
   :34-35 "tcp/u4 81.1%→66.3%"中 **81.1% 不是 G6 的 tcp-mux u4 轮回值**
   ——它是 round-1 tcp-nomux(mux=0,非 G6 档)u4 的 medians 口径
   (2280/2812,round-1/single-stream-medians.txt:4);G6 口径下 round-1
   tcp-mux u4 验收器为 **76.16% 本就 FAIL**(round-1/verify-goal-report.txt:83),
   故 tcp/u4 实为 **FAIL→FAIL,无翻转**。两轮间真实翻转仅两档:
   **tls/u4**(round-1 PASS:medians 口径 86.4%=2304/2666、验收器口径
   87.3%[复核事实] → round-2 72.1% FAIL)与 **wss/d16**(round-1
   79.05% FAIL[round-1/verify-goal-report.txt:94]→ round-2 验收器口径
   88.0% PASS;:34 "79.05%→85.3%"混用了验收器/medians 两口径)。
   另 single-stream-medians 中 2484 等低值为 16 流档 direct,非单流。
   (本条为对上一版勘误的再勘误:上一版曾把 81.1% 当作 tcp-mux u4 的
   r1 值,系同样失误,已改正。)
5. **验收器结构性限制**(§9.1/§10.1):G4/G5 为 loadgen 自报告 JSON、
   "实际配置"为 toml 回读、全知伪造者一致平移无法绝对免疫——需三机
   现场监督补强,不以本报告宣称已闭环。
6. **musl 交叉环境缺件**(§8.0)与 **e2e-relay-modes Linux 非 mux 轮
   未执行**(§14.2)。
7. **归因未闭合**:kcp 预授信 1/4 流回退因果(下轮 A/B)、tls 上下行
   不对称(下轮 RELAY=copy A/B + 对称采样)、RH_NOTSENT_LOWAT 64KiB
   生产收益(下轮 env 矩阵)——§12.5 清单待性能工程师执行。
8. **诊断工具层未闭合**:refrelay2 第一版单连接 hang 的深层原因未解释
   ——仅回退旧 per-connection 线程结构后恢复正常
   (benchmark/diag/refrelay2.rs:94-95 注释「修正版曾把 handle 内联进
   主循环并 join,阻塞 accept 被实证 hang」、:116 h2 方向反转注记;
   targeted-before-round-3/REPORT.md:7 记录该探针 bug 造成的复现与重跑
   开销),实验报告自述未进一步解释。属诊断探针自身问题,不影响 rh
   生产代码结论,但归因未闭合,如实披露。
9. **低风险记录项**:copy 路径错误收尾无界(§8.4)、vendor pending
   控制帧读饿死(§8.5)、G1 CPU 口径中位数 vs 逐样本待主代理裁定
   (§8.6)。
10. **三机状态**:round-2 轮末三机复核无残留(ROUND2-REPORT 过程管理,
    TEST*_CLEAN);此后未再连接,状态以该时点为准。

### 14.6 无提交/无推送与无环境遗留证据(本会话实际执行)

- **无 commit/push**:`git log -1` → `c3ca4bd`(与开工前快照一致);
  `git reflog` 最后一次 commit 为 2026-10-12 13:06(早于全部修复轮,
  其后仅三次 `reset: moving to HEAD` 均停在 c3ca4bd);`git stash list`
  空;`git status --porcelain` 44 条全部为工作区 `M`/`??`,无一入库。
- **无本机进程残留**:`ps aux | grep "rh-server|rh-client|rh-round|
  rh-rm-|rh-tr-|goal_repair|window-pregrant|local-twohop"` → 0 条。
- **临时产物**:`/tmp/rh-rm-*`(e2e 前缀)、探针目录均已清除(本轮
  `ls` 核实不存在);`/tmp/rh-readonly-review.eFSs9d`、
  `/tmp/rh-main-independent-review` 为**指定审计证据**,有意保留;
  `/tmp` 其余 `rh-*` 目录(rh-bench、rh-e2e 等)属其他并行交付产物,
  非本轮创建,未触碰。`/tmp/vg-rerun*.txt` 为本轮复跑输出(14.4 依据)。
- 全部运行器超时 killpg(整组 SIGKILL),日志含 `RETURN_CODE=` 真实退出码。

### 14.7 后续(明确)

1. **主代理亲自复核为准**:逐项读取交付源码与日志、重执行关键独立测试
   (主代理已有 harness 与复现装置,见 §7.2/§14.1),不沿用本报告与
   各轮自述作为结论;本报告亦不构成"绝对无缺陷"声明——已知未解决项
   见 §14.5,未知风险由独立复核暴露。
2. 脚本下一轮对最终树全套复验(world.run);若含 round-3 远端实验
   (§12.5/§13.2 清单),性能证据须整轮重取并重建源-二进制绑定(§14.4)。
3. 本轮(追加轮 7)除本文档外零改动:未改源码、未改测试、未降阈值、
   未删失败证据、未连远端、未动系统配置。

## 15. 追加轮 8(2026-10-14):Linux e2e 首跑 6 失败的测试侧修复

背景:主代理确认最终 Linux e2e 第一套 PASS=65/FAIL=6,6 项全部为
`tests/e2e.sh` 测试脚本缺陷而非产品缺陷(`benchmark/runs/goal-repair/
final-linux/REPORT.md`)。本轮经明确授权对 `tests/e2e.sh` 做最小修改
(此前"不改旧 tests"限制仅对本项解除),不碰生产 Rust、不改任何验收
门槛、不删除失败用例。`git diff --stat`:仅 `tests/e2e.sh`
(+142/−20)与新增证据/脱敏文件。

### 15.1 修复项 1:BSD sed → 便携密钥写入

- 旧 L774 `sed -i '' "s/^secret = .../"` 是 BSD 专用语法;GNU sed 把
  脚本文本当文件名:secret 未写入 client.toml(→"新密钥重连上线"FAIL
  及 4 项级联),且报错信息把轮换密钥打印进日志(final-linux/logs/
  e2e.log:47,即红证据)。
- 新 `update_secret`:python3 读文件 → 正则 `^secret\s*=\s*".*"\s*$`
  命中**恰好 1 行**才原子写回(`os.replace`),否则报错不写回;密钥经
  环境变量 `UPDATE_SECRET_VAL` 传递,不进命令行(ps 不可见)、不进
  错误输出。轮换处新增 `check $?` 真实断言写入结果;密钥轮换返回新密
  钥、旧会话下线、新密钥重连三项断言原样保留。
- 便携自测:`bash tests/e2e.sh --self-test` 4/4(恰好一行替换且其余
  行不动/无 secret 行拒绝且不写回/两行 secret 拒绝/密钥经 env 传递)。

### 15.2 修复项 2:metrics 断言与产品架构对齐

- 产品自 commit 0fe4069 起指标在**独立无鉴权端口**暴露
  (`config.rs` MetricsConfig 默认 127.0.0.1:9100;`state.rs`
  `metrics_listen_start` 裸 axum router `/metrics`,无鉴权中间件),
  旧断言仍打 API 端口 `/metrics`(带令牌数指标、无令牌数 0)已过时。
- `server.toml [metrics]` 显式 `bind_addr = "127.0.0.1"` +
  `port = $METRICS_PORT`(18450,与其他端口无冲突);新断言:
  1. 独立端口**不带任何认证头** HTTP=200 且 >3 行格式合法样本
     (`^rabbithole_\w+({…})? 数字`)——空 body/404 显式不算成功;
  2. API 端口 `/metrics` 带令牌与不带令牌 body 均为 0 行 rabbithole_
     指标(实际状态码写入标签供人工核对)。
- 本机实测:独立端口 HTTP=200/5 行;API 端口 HTTP=200/0 行(产品鉴权
  行为零改动)。

### 15.3 修复项 3:PID 治理与预检(不再广域扫杀)

- 首启记录 `SRV_PID`/`CLIENT_PID`(`$!`),重启/轮换按**精确 PID**终
  止旧实例、新 PID 一律 `PIDS+=` 追加;删除旧 `PIDS[4]=$!`(覆盖 DNS
  stub 槽位导致 DNS 残留)与两处 `ps -eo … | grep` 广域匹配。
- 删除对 `/tmp/rh-*` 的广域扫杀,改为**端口预检**:全部静态 TCP/UDP
  端口(含 metrics 18450)python3 bind 探测,任一占用即 `exit 2` 报错
  退出、绝不杀未知进程;auto_range 20000-21000 动态段不预检(由服务
  端绑定失败自然暴露)。
- 归档唯一化:`/tmp/rh-e2e-last-<时间戳>-<pid>`,不再 `rm -rf
  /tmp/rh-e2e-last` 覆盖其他会话证据;`--self-test` 不产生归档。
- **预检实证**:本机首跑 `01-e2e-macos.log` RC=2,预检拦截 UDP 19553
  占用——查实为 10-09 遗留孤儿 DNS stub(`python3 - 19553`,父进程
  launchd,特征与本脚本 heredoc 完全匹配),定向 kill 该已验证身份进程
  后重跑;预检"报错、不扫杀"行为得到真实运行验证。

### 15.4 修复项 4:旧日志密钥脱敏

- `final-linux/logs/e2e.log` 原件**保留不动**(本地证据);新增
  `e2e.log.redacted`(唯一 1 处密钥替换为 `<REDACTED-ROTATED-SECRET>`
  占位,其余逐字节一致)与 `README-redaction.md` 说明:外发只允许用
  脱敏副本,不伪装原始值。新脚本日志实测无 64-hex 密钥
  (02-e2e-macos.log grep 计 0)。

### 15.5 本轮证据(真实退出码)

| 检查 | 结果 | 证据 |
|---|---|---|
| `bash -n tests/e2e.sh` | 通过 | 本轮执行输出 SYNTAX-OK |
| `tests/e2e.sh --self-test` | 4/4,RC=0 | 终端输出(见 15.1) |
| 预检拦截遗留进程 | RC=2 报错不杀 | `relay/e2e-fix-local/01-e2e-macos.log` |
| 本机全量 e2e(release) | **PASS=73 FAIL=0,RC=0** | `relay/e2e-fix-local/02-e2e-macos.log`(900s runner) |
| 运行后残留 | 无进程/无监听(含 19553/18450),归档历史保留 | lsof/ps 复核输出 |

红=final-linux 首跑 6 FAIL(旧脚本在 Linux 的真实失败);绿=02 本机
全量通过。**Linux 远端复跑属性能工程师**,本轮未连远端。

### 15.6 边界(如实)

- `--self-test` 的"密钥经 env 传递不泄漏"为静态自证(辅助从
  os.environ 取值、argv 无密钥);未做运行中 `ps` 动态取证。
- API 端口 `/metrics` 本机返回 200(console 兜底路由),断言只认
  body 指标内容;不同版本若返回 404 亦满足"不暴露",状态码仅入标签。
- 未验证 Linux 真机(授权范围外,由性能工程师执行);GNU 侧风险点
  (sed/grep/mktemp/date)已全部替换或双侧兼容。
