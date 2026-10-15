# G1-2800 当前进度（不是最终性能报告）

最新用户要求：主代理诊断、独立复核和真实三机验收；GLM-5.3仅按具体规格写代码；只运行完整workflow；TCP非复用单流上传/下载各≥2800。

## 已完成

- 主代理完成234条有效P1诊断、独立raw形态与位速率复核；参考copy/splice、CC、批量、发送合并、pacing、窗口、线程定位、管道与有界并行，均无双向达2800的证据。没有把这些阴性结果写成环境硬上限。
- 完整GLM-5.3编码workflow及主代理具体问题修补完成。模型与职责记录见执行台账；不运行工作流片段。
- 主代理首轮独立Linux复现新增取消回归真实失败，保留红证；将精确修复契约交GLM；主代理第二轮Linux全splice：**26 passed /0 failed**，19原回归全保留，7新增。
- 主代理代码审查：只改变两个授权文件；默认统计关闭时不分配统计Arc或读时钟/统计原子，开启时新增真实Pending poll/cycle/时长、socket四元组与epoch锚；原业务计量/限速/取消/半关闭状态机未改变。
- 当前源tree_hash `85daf0be16285407d42bf5df858d658e41a9af2c23ce858007776493078aac3a`，旧3000核验器未改。新G1-2800核验器17条对抗回归通过。
- 主代理最终版本fmt/clippy/workspace检查通过；Python既有141条通过。macOS绿不代替Linux，真实Linux已另跑上述26项。

## 正在运行的真实强验收

主代理执行 `python3 benchmark/runs/g1-tcp-2800-main/tools/main_accept.py candidate-final`。其真实命令冻结当前树→三机逐项源/hash同步→Linux workspace/release→四套E2E→TCP mux0/splice默认统计关闭、双独立窗口每方向10秒×3配对→原始raw/CPU/identity+新2800判定。

日志 `benchmark/runs/g1-tcp-2800-main/candidate-final-main-gates/measure.log`；结果目录 `candidate-final/`。尚在Linux构建/测试阶段，**没有本次新构建吞吐结果，不能宣布G1达标**。

目前最新已核验产品性能仍为上轮源码精确raw1952.92/2036.57Mbps，未达到2800。此次代码属于观测准确性与默认仪表开销修补，没有承诺单项吞吐收益。最终默认三机样本及独立核验齐备后才能更新最终结论。
