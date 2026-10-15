# G1 TCP 非复用单流专项报告(final,2026-10-15)

## 判定:**G1 FAIL(未达标)**

机械判定原件 `benchmark/runs/g1-tcp-20261014/gates/verdict-final-20261015-045356-FAIL.json`(schema `g1-tcp-verdict-v1`):`evidence_valid=true`、`g1_pass=false`、`regressions_pass=true`、upload 中位 **1952.5 Mbps**、download 中位 **2036.0 Mbps**,双方向均 < 3000 Mbps 目标。独立固定审计结论为 achieved=false、g1=false,measurements 六条与机械 verdict 的六条逐字相同,issues 仅"原有 G1 判定 FAIL"——该审计是主代理收口时提供给本报告的输入材料,**不是仓库归档文件**(本轮已在 `benchmark/runs/` 全树 grep `achieved` 确认无对应 JSON 原件),读者复核请以上述 verdict JSON 为准。本报告只依据 final 原件(`benchmark/runs/g1-tcp-20261014/final/`)与 `final-change.md`/计划文档写成,未复跑任何测量或 gate。

真实结论一句话:三机链路上 TCP `mux=false`/`relay=splice` 单流上传/下载中位分别只达 3000 Mbps 目标的 **65.1%/67.9%**(另一口径——同轮 tunnel/direct 之比见 §1 末尾),双向 CPU 充裕(≤34.6% 单核)、既定回归与四套 E2E、Linux 全量测试全绿——瓶颈不在 CPU 饱和,吞吐差距**尚未归因**,下一候选方向(管线深度/发送节律)未开始。

## 1. 最终三机测量(唯一判定依据)

装置 `python3 benchmark/g1_tcp.py measure --out-dir benchmark/runs/g1-tcp-20261014/final --final`(gates/final-measure.log,exit=0,wall 1931.247s)。三机环境(test1/2/3 = 172.16.1.6/.17/.18,Linux 5.15.0-47-generic,iperf 3.17.1,RTT 62–78ms,run-index.json:86-130)。每方向单流(streams=1)10s × 3 配对 × 2 独立窗口,共 24 个样本,全部 `valid=true`、`exit=0`。

记录数口径(72 条 vs 判定 24 条):results-*.jsonl 中每窗口 36 条 `metric=="throughput"` 记录,构成为 streams∈{1,4,16} × {direct,tunnel} × {upload,download} × 3 样本 = 12 组 × 3(本轮用 python3 逐行 json 解析两窗口 results 文件按 streams/path/dir 计数核实,分布见下);G1 判定只取 streams=1 的 12 条/窗口(共 24),streams=4/16 的 24 条/窗口为多流诊断附带,run-index scope 明文"绝不混为单流",未进入本表与判定。"0 条 invalid/非零退出"指对全部 72 条逐条检查 `valid` 与 `exit` 字段的结果:

| streams | 路径 | 方向 | 每窗口条数 | 用途 |
|---|---|---|---|---|
| 1 | direct/tunnel | upload/download | 各 3 | **G1 判定(本节表)** |
| 4, 16 | direct/tunnel | upload/download | 各 3 | 多流诊断附带,不入判定 |

| 方向 | 路径 | w1 样本 (Mbps) | w2 样本 (Mbps) | 6 样本中位 | 判定 |
|---|---|---|---|---|---|
| upload | direct | 3082, 3072, 3247 | 3658, 2743, 3058 | **3077** | ≥3000,达标 |
| upload | tunnel | 1899, 1967, 2413 | 2033, 1938, 1849 | **1952.5** | <3000,**FAIL** |
| download | direct | 2659, 3093, 2749 | 3644, 3942, 2912 | **3002.5** | ≥3000,达标 |
| download | tunnel | 1910, 2106, 2056 | 2016, 2283, 1849 | **2036.0** | <3000,**FAIL** |

中位数与归档 verdict 逐字段一致(排序样本:tunnel 上行 [1849,1899,1938,1967,2033,2413]→(1938+1967)/2;tunnel 下行 [1849,1910,2016,2056,2106,2283]→(2016+2056)/2)。

两点如实说明:

- **verdict failures 数组里的 4 条 CPU 条目并非失败条件**。verdict JSON 的 failures 原文共 6 条:`[G1] upload:单流中位数 1952.5 < 3000 Mbps`、`[G1] upload server:CPU 中位数 28.3%`、`[G1] upload client:CPU 中位数 34.61%`、`[G1] download:单流中位数 2036.0 < 3000 Mbps`、`[G1] download server:CPU 中位数 30.979999999999997%`、`[G1] download client:CPU 中位数 14.100000000000001%`(gates/verdict-final-20261015-045356-FAIL.json:20-27)。装置把 `evaluate_gates` G1 gate 的 `details`(fail 与 note 混列,verify_goal.py:1024-1029)全量写入 failures(g1_tcp.py:970);CPU 中位数条目文案产自 note 分支(verify_goal.py:1086,达标 `<50%` 时记录),吞吐条目才产自 fail 分支(verify_goal.py:1069,文案含 `< 3000 Mbps`)。按此对照,6 条中只有 2 条吞吐条目是使 `g1_pass=false` 的判定条件;CPU 四项按门限 `<50% 单核` 实际**达标**(见 §2),条目文案本身(无"<50%"/覆盖率字样)与其 note 来源一致。
- **final 轮是"普通 FAIL",不是 round-4 的"环境受限 FAIL"**。环境受限退出准则要求 fresh direct 下行中位 <3000(g1_tcp.py:988-999,THRESH 来自 run-index env_limit_watch);final 轮 direct 下行中位 3002.5 ≥ 3000,该准则未命中,verdict failures 无环境受限条目。round-4 轮(03:41,direct 下行合并中位 2978<3000)曾以"环境受限"停止候选迭代(final-change.md:19-22);两轮均为真实测量且同树(tree_hash 一致),final 为最终判定轮。direct 下行 6 样本跨 2659–3942 波动幅度大,环境直连容量本身在该链路上不稳定,但 final 当轮证据不支持"3000 不可达"。

软件差距(口径 B:同轮 tunnel/direct 中位之比):上行 1952.5/3077=63.5%,下行 2036/3002.5=67.8%(口径 A"达目标的 65.1%/67.9%"的分母是 3000 目标,两口径并列出现时以分母区分)。上行 direct 达标而 tunnel 未达 → 差距主要在转发路径,未归因(§7)。

## 2. 双端 CPU(按方向分别判定,单核口径)

verify_goal.py:181 口径(窗口内样本数/期望≥95% 为覆盖;CPU%=Δticks/CLK_TCK/窗口秒)从原件 `endpoints/{server,client}/cpu.jsonl` 逐窗口复算,与 verdict 四项中位数逐一吻合:

| 方向×端 | 每样本 CPU% (w1 s1,s2,s3 / w2 s1,s2,s3) | 中位 | 覆盖率范围 | 判定(<50%) |
|---|---|---|---|---|
| upload server | 30.84, 27.17, 26.93 / 29.43, 25.54, 33.03 | **28.3%** | 98.0–98.5% | 达标 |
| upload client | 33.85, 35.11, 37.58 / 34.56, 34.07, 34.66 | **34.61%** | 98.0–98.5% | 达标 |
| download server | 31.44, 31.38, 25.34 / 34.05, 30.58, 25.38 | **30.98%** | 98.0–98.0% | 达标 |
| download client | 14.07, 14.13, 14.19 / 13.62, 15.21, 14.01 | **14.10%** | 97.5–98.5% | 达标 |

全部 12 个窗口 × 2 端覆盖率 ≥97.5%,超过 95% 门限;CPU 中位数全部 <50% 单核。CPU 不构成 G1 失败原因。

## 3. 构建、源与身份 hash(全来自 final/run-index.json)

- git HEAD `c84f63174cd2ebafd994c4fafacd5beba4a0240b`;工作区内容 tree_hash `535480dba12a0279df44b17919ef4799568efd6796f9009fc118315b15959d6e`(run-index.json:171;与 round-4 测量态一致,复核已验证零改动收敛)。
- 源快照 `final/source-final-20261015-042144.tar`,sha256 `1653712d1a8c07dced12a3cb5caa787349ce3d5d1f6b039c4378fb795c8dfbcf`(run-index.json:166),112 文件(workspace-deterministic-v1)。
- 测量二进制(同构建上传三机,run-index.json:3-5;原件 `final/build/`、各窗口 `evidence/*/build/binaries.json`):
  - rh-server `ebc44065af2048ae5aa299919ab973d5ab4e0014a3cf9985e490145f6e9c7a81`
  - rh-client `7ed52739b0c3bc7778fa899068006dc19911aae7be3533c51924f6e06ef8c335`
- 工具哈希(sha256,run-index.json:115,169,173,16 记录;本轮以 `shasum -a 256` 对当前工作区文件实测,四者逐一相符):
  - g1_tcp.py `aad32120730a1aa7ff8a5c85277bdd8c368da6fabd743d85bf79d5dba00a8203`
  - test_g1_tcp.py `9e8239d7aeba53856341c8024820e8e29f6034e402875d0c19d2eeeff9fbc0b3`
  - verify_goal.py `a580c0b6449c100e1d827331b8177fa74c5f05fabc38da1cdf7d91eccf3a89e5`
  - closure_c84.py `e6b5b9f37e072b6d245c96b5bda3290eb9e84ef5dd54954d9afed20c59d4deb6`
- "verify_goal.py 未修改"的可复核含义:其 sha256 与 closure-c84 时期快照 `benchmark/runs/performance-closure-c84/candidate-r0/source-snapshot/benchmark/py/verify_goal.py` 及 final run-index 记录三处一致(本轮 `shasum -a 256` 实测,全值见上条),即自该历史基线以来未被改动。rustc/cargo 1.98.1,nofile soft/hard 65535。
- 验收轮零开关:两窗口 `knobs={}`、`env_extra={}`(run-index.json:139,149);装置规定验收轮携带环境开关即结构性失败(g1_tcp.py:940-943)。

## 4. 默认配置 vs 实验开关(交付路径不含任何开关)

| 项 | 默认(final 实测即此) | 临时开关(仅归因,未用于 final) |
|---|---|---|
| 泵实现 | `pump_batched`(未设 env 即默认,splice.rs:145-150) | `RH_SPLICE_PUMP=legacy` 旧控制流对照臂 |
| 泵计数输出 | **关**(`RH_SPLICE_STATS` 未设→零输出,splice.rs:115-118) | 非空且非"0"→泵退出 stderr 单行 JSON |
| NOTSENT_LOWAT | 1MiB 不变(历史结论:计划 §1 转述"未证明稳定收益、不再盲扫",docs/performance-g1-tcp-plan.md:11,35;本轮在 runs 树未检索到该历史 A/B 的独立结果原件,`lab/README.md:62-64` 仅有 targeted-spec 示例——**此句为计划转述,本轮未验证**) | 既有 `RH_NOTSENT_LOWAT` 实验口,本轮未使用 |

计数累计本身始终进行(每泵本地 10×AtomicU64 Relaxed,无跨泵争用,splice.rs:79),开关只控输出。

## 5. 实际改动与回归(行号)

本轮专项唯一生产机制变更收敛于 `crates/rh-transport/src/splice.rs`(1867 行,相对输入基线累计 +788/-9,git diff 全量核对;与 round-4 测量态逐字节一致,final 零再改动)+ 新 example `crates/rh-transport/examples/g1_splice_bench.rs`(349 行,fixed-ab-v1 契约)。关键位置:

- `PumpCounters` splice.rs:79;`stats_enabled()` :115;`pump_mode_from_env()` :145;旧控制流 `pump_legacy` :333;默认臂 `pump_batched` :554;批首 biased select 每块同步 poll cancel(round-2 B1 修复):600-604;有界批次 4MiB/64ops 后确定性 `yield_now()`:768;臂选择 :780。
- 新增 7 个定向回归(全部在 splice.rs 内 linux-only 测试模块):`splice_pump_mode_defaults_to_batched`:1538、`splice_legacy_arm_transfers_and_half_close`:1545、`splice_stats_count_payload_bytes`:1590、`splice_batch_yields_to_sibling_task_on_single_worker`:1657、`splice_cancel_during_saturated_transfer`:1730、`splice_silent_period_no_busy_spin`:1796、`splice_stats_snapshot_json_fields`:1838。既有 12 个 splice 回归经默认臂全量覆盖(名单见 final-change.md:96-102)。
- 设计期自查修正并如实撤回:`consume_budget()` 让步方案在落地前修正为 `yield_now()`(预算永不耗尽的真实 bug,未进入任何测量)。

**采纳理由(不把 loopback 当最终依据)**:三机吞吐中性——baseline-retry 轮(gates/verdict-baseline-retry-20261015-015032-FAIL.json,upload_mbps=1931.0/download_mbps=2039.0;原件 `baseline-retry/evidence/*/results-*.jsonl`)与 round-4 轮(gates/verdict-round-4-20261015-041537-FAIL.json,1986.5/1992.0;原件 `round-4/evidence/*/results-*.jsonl`)tunnel 单流样本区间重叠:上行旧 [1870,2125] vs 新 [1810,2038]、下行旧 [1903,2456] vs 新 [1955,2166](两轮 6+6 样本均本轮从上述 results 原件 jq/python 实取复核)。n=6/臂、未做显著性检验,准确表述为**无证据支持控制流开销假设 H1**(final-change.md:53 原文措辞"H1 证伪"强于本报告采用)。注:此处 1931.0 为 baseline-retry 上行中位 Mbps,与 §1 final 轮 wall 1931.247s 纯属数字巧合,无关联。保留依据是真实生产行为收益——单 worker 公平性(每批让步,回归锁定)与 cancel 响应确定性:批首 biased select 每块 100% 确定 poll 一次 cancel(splice.rs:600-604);"legacy 仅 ~50% 机会"是机制推断而非三机测量,其依据为本机 select! 语义实验 970/2000(round-2-change.md:14-15,81,macOS 本机,支撑"随机分支顺序下 pending 分支仍被经常 poll"的前提)。

## 6. 固定量单因果对照(loopback,仅归因,不构成 G1 判定)

`round-4-ab/fixed-ab/`,128MiB × 3 轮交替 × 双向,同树同计数两臂(Linux,gates/round-4-ab.log exit=0)。本轮由原件 `summary.json`/`summary-smallpacket.json` 重算核实:

- 吞吐:old 上行中位 **5.086**/下行 **4.991** Gbps vs new 上行 **5.181**/下行 **5.198** Gbps(up+1.9%/down+4.1%,n=3/臂,new 含 7.208 Gbps 离群轮)→ 未观察到差异(n=3,未做显著性检验,不使用"显著"一类统计断言),与三机"无证据支持 H1"方向一致。
- 小包 RTT p50 中位:old 105.3/113.4µs vs new 97.9/95.4µs,无退化;全表最差单值 r1-new-download 131.4µs(单测锁定必须可见)。summary-smallpacket.json 自注:本地衍生防单轮误引,原始以 12 个 `r*-old/new-*.json` 为准。

## 7. 归因缺口(如实:未归因,无泵内实测样本)

- 上行差距(direct≈3077 vs tunnel≈1952.5)的**泵外归因无分析**:round-4 诊断装置已从 termark 原件恢复真实 `ss -tinm` 样本——`round-4/diagnostics/g1-tcp-nomux-diag/server-diag-ss-proc-recovered.log`(2181 行,本轮 grep 计数 cwnd×805/app_limited×312/notsent×66/rwnd_limited×42/skmem×805)与同目录 `client-diag-ss-proc-recovered.log`(3411 行,cwnd×1073/app_limited×878/notsent×112/rwnd_limited×75/skmem×1073,本轮同法计数),但**尚无人做归因分析**;EAGAIN/pending_waits 泵内分布仍无实测样本(fixed-ab 臂日志未回收、三机诊断轮未开 RH_SPLICE_STATS)。在"无证据支持 H1"之后,下一候选方向(管线深度/发送节律)未开始。
- final 轮 direct 下行中位 3002.5,但 6 样本 2659–3942 波动大;不能据此宣称环境稳定支撑 3000。

## 8. Linux 全量、四套 E2E 与最终 gates(全部真实退出码)

final 轮(同构建,`--final` 完整回收,run-index.json:20-84),本轮实读退出码原件:

| 检查 | 原件 | exit | 结果 |
|---|---|---|---|
| cargo test --workspace | final/logs/test-workspace.exit | **0** | 装置解析 suites>0、non_ok=0 |
| cargo build --release | final/logs/build-release.exit | **0** | — |
| E2E e2e | final/logs/e2e-e2e.exit | **0** | 73 passed / 0 failed |
| E2E resilience | final/logs/e2e-resilience.exit | **0** | 19/0 |
| E2E transports | final/logs/e2e-transports.exit | **0** | 12/0 |
| E2E relay-modes | final/logs/e2e-relay-modes.exit | **0** | 21/0 |

`e2e_policy.final=true`(注记"--final:同构建完整执行并回收四套 E2E 原件")。此前独立复核的 Blocker 1(四套 E2E 缺原件)已被 final 轮解除:四套原件齐全且全过,regressions_pass=true 现覆盖 workspace 测试+release 构建+E2E 四套。

关于两份 workspace 测试结果的关系:上表 `final/logs/test-workspace.exit` 是**远端 Linux**(三机之一,同构建产物)执行的 `cargo test --workspace --locked` 原件,是 verdict(regressions_pass)唯一采信的一份——splice 模块 linux-only,macOS 绿不构成覆盖(计划 §4.3);`gates/final-workspace.log` 是装置在本机(darwin)对同一命令的独立重跑(exit=0,cmd 见 gates/final-workspace.log.meta.json),二者命令相同、主机不同,后者仅作本机侧旁证。同目录另有的 final-fmt/final-clippy/final-python 为本机 gates 侧检查,均 exit=0(cmd/wall 见各自 *.meta.json,本轮实读)。

## 9. 失败与无效样本:全保留,只增不覆盖

- **final 本轮**:w1/w2 各 36 条 throughput 记录 0 条 invalid/非零 exit,无失败样本可留;装置规定验收轮 drive 非零即中止、invalid 样本静默过滤即结构性失败(g1_tcp.py:933-936),evidence_valid=true 即无此情形。
- **历史失败原件全部保留**:`gates/baseline.log`(exit=1,基线首测失败)与 `gates/verdict-baseline-20261015-010724-FAIL.json`(evidence_valid=false,缺证据根)、`gates/verdict-baseline-retry-20261015-015032-FAIL.json`、round-2 两个环境性失败日志(装置本机缺 cargo 的 FileNotFoundError,命令从未执行)、round-3 fmt exit=1(既有树 fmt 脏)、round-4 两个历史 verdict(034239/041537)并存未覆盖。证据根 `benchmark/runs/g1-tcp-20261014/` 全程只增。

## 10. 远端治理清理(身份核验通过才杀)

final 轮两窗口清理全程留痕(`final/logs/0172-0272` govcheck/govkill/relist + `final/events.jsonl`):

- **govcheck**:逐 PID `readlink /proc/$p/exe` + `/proc/$p/stat` starttime 校验后才允许清理(例:`0172-govcheck-server/cpu.jsonl.pid.log`);**govkill**:TERM→5s→仅当 starttime 仍匹配才 KILL(`0173`),全程不杀未知进程。
- **实际清理 10 个进程**(events.jsonl"已治理清理"):每窗口 rh-server×1、rh-client×1、iperf3×1、python3.10×2(cpu 采样器、target);**6 条身份不匹配跳过**(server-role/client-role/cpu.jsonl 等 exe='' 即已自然退出的 wrapper;events.jsonl 原文 phase 为"跳过治理清理(身份不匹配)",detail 注明"不杀未知进程")。
- **端口级缺口(如实)**:清理证据为进程级;未见独立的监听端口复扫原件(无 `ss -ltn` 事后快照)。

## 11. 结论与明确缺口

1. **G1 未达标**:tunnel 单流中位 1952.5/2036.0 Mbps < 3000;CPU(≤34.61%)、采样覆盖(≥97.5%)达标,§5 回归名单与 §8 六项既定检查(远端 workspace/build + 四套 E2E)及本机 gates 五项全部通过——**本轮既定检查清单未发现任何正确性失败**;这是枚举性结论,不构成对未列入检查的正确性穷尽证明。
2. **吞吐差距未归因**:H1(泵控制流开销)在三机(n=6/臂)与固定量对照(n=3/臂)中均未获得支持证据(措辞见 §5/§6,未做显著性检验);ss 样本已恢复但无人分析,泵内 EAGAIN/等待分布无实测;下一步若重启该线,应先做 §7 归因再谈管线深度/发送节律类候选(计划 §2.C/§2.D 边界内)。
3. **记录一致性遗留(轻微,独立复核指出,未修正)**:final-harness-fix.md §1.Blocker2 关于 summary-smallpacket.json"含每源文件 SHA"的描述与产物不符(文件无 sha256 字段,数字本身已 4/4 独立重算吻合);final-change.md §6.2"ss 实际样本仍缺"表述已过时(数据已恢复,见 §7)。本报告按现状如实引用。
4. 本报告只覆盖 G1 与相关正确性,不声称 G2–G7 目标完成;loopback/多流数据仅归因,未混入判定。
