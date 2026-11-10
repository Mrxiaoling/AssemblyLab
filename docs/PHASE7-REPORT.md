# Phase 7 阶段报告(全部完成)— 学习系统完善

日期:2026-11-09 · 状态:待用户验收(第一批见 git 历史同文件;本版合并第二批)

## 第一批(此前已交付,摘要)

8 种题型判题模型全落地(QuizSpec/SelfCheckSpec 新增)+ 6 道新题 + 知识图谱页(113 节点自动聚合,掌握度四色着色)+ 错题本 + /api/knowledge、/api/mistakes + content_tests 全覆盖。**本版补充修复:ExerciseView 路由复用不响应 ?id=**(同款第 3 处)。

## 第二批(本版新增)

| 任务 | 交付 |
|---|---|
| T7b.1 综合项目课 | project-lesson01「解剖 printf 的完整调用链」:main → PLT(延迟绑定)→ __vfprintf_internal(stdio 缓冲)→ write → syscall;附**可复用的"实际项目关键函数分析"方法论**(带符号编译→调用栈读层→关键指令定位→分层卡片) |
| T7b.2 综合编程大题 | project-q01-programming-strcpy:双用例(RDI 预置源/修正后目标),断言复制后含终止 0 的完整字节(0xCC 填充验证"复制到 0 含 0"边界) |
| T7b.3 AI 助教(本地检索式) | `POST /api/tutor`:**确定性检索引擎**——知识源全部来自仓库真实数据(两架构 ISA notes 74 条/27 门课程正文/20 题解析/知识图谱 113 节点),token 命中打分;回答三层组织(30 秒版/深入版/动手版)+ 盲点分析(真实作答记录聚合)+ 推荐下一步。**本地检索路径零模型、零网络、零编造**;P8 追加的可选外部 LLM 挂点默认未配置(见文末 P8 附录) |
| T7b.4 助教前端页 | 新导航「AI 助教」(/tutor):提问框 + 三层回答卡片 + 盲点分析 + 推荐习题直达 + 关联知识点/课程跳转 |
| T7b.5 离线课程包导出 | `GET /api/export/course/{id}`(单课)/ `GET /api/export/all`(全 27 门合集):**自包含单文件 HTML**(课程正文+实验源码,file:// 直接可开,零网络);**默认不含参考答案**(`?include_answers=true` 才含),footer 如实标注"动手实验请回平台(真实引擎)" |

## 2. 关键决策与发现

| # | 决策/发现 | 记录 |
|---|---|---|
| D-7b-1 | **AI 形态选本地检索式而非 LLM API**:README 第九节十条职责中,除"多层次解释"外全部可由确定性检索覆盖(数据源本来就是结构化的);离线纯净(README 十节 #9/#10 由架构保证);外部 LLM 留挂点(ASSEMBLYLAB_AI_ENDPOINT 约定,由用户决定是否接) | tutor.rs 头注 |
| D-7b-2 | 助教回答**三层组织**(30 秒版/深入版/动手版)直接对应 README"同一问题应支持不同解释层次" | TutorResp.layers |
| D-7b-3 | 盲点分析不靠 AI 猜:直接聚合 `latest_attempts()` 中非 passed 的知识点,按频次排序 | knowledge × attempts |
| D-7b-4 | 综合项目选 **printf 解剖**:一条链串起 call/ret(P4)、GOT/vtable(P5)、syscall(P6)、stdio 缓冲——README 第五阶段实验 8"实际项目关键函数分析"的标准工作流示范 | project-lesson01 |
| D-7b-5 | **导出是数据投影不是第二真相**:离线包内嵌 toml 数据的 HTML 渲染,重新导出即最新 | export.rs |
| D-7b-6 | toml Index panic 第 4 次复发(export.rs/tutor.rs 新代码)——项目内该模式已全部 `.get()` 化,后续新代码一律 .get() | 多处 |

## 3. 测试结果

- 后端 **19 套件 35 项测试全绿(rc=0)**
- curl:/api/tutor("lock cmpxchg")命中 CMPXCHG/lock 前缀节点、三层回答、盲点 3 条(真实作答记录)、推荐 strcpy;`/api/export/course/stage08-lesson01`(6.3KB 单文件 HTML)、`/api/export/all`(全 27 门合集,当时导出体积 175KB)
- 浏览器:AI 助教页提问"协程帧里保存了什么?"→ 三层回答 + 相关课程(stage07-lesson01)直达按钮;strcpy 判题 ✅
- 前端 vue-tsc 零错误 + vite build 通过

## 4. README 十五节总对照(P7 收官)

| # | 标准 | 状态 |
|---|---|---|
| 1-5 | 场景驱动/实验可运行/观察真实/修改变量/多架构 | ✅ P1-P5(前报告) |
| 6-7 | 真实 C++ 汇编/调试 | ✅ P2 |
| 8 | 函数调用与 ABI | ✅ P2/P5 |
| 9 | AI 助教 | ✅ 本批(本地检索式,基于真实信息) |
| 10 | 支持离线使用 | ⚠ 部分实现:离线**阅读/复习**(课程包单文件 HTML)✅;离线**动手实验**(README 十节 #9/#10 的完整要求)未实现——模式 A 引擎本就离线但依赖本平台运行,模式 B 依赖 WSL。如实标注,见 §5 |
| 11-13 | 结构化数据/真实工具链/离线核心课程 | ✅ 全程 |
| 14 | 10 个教学阶段课程 | ✅ 27 门课覆盖 README 第五节的阶段 1-10(每阶段 2-4 门);README 并无"十四阶段",该数字为早期笔误 |
| 15 | 15 节验收对照 | ✅ 各阶段报告 |
| 第八节 | 8 种题型 | ✅ 本批全落地 |
| 第五阶段实验 8 | 实际项目关键函数分析 | ✅ 综合项目课 |

## 5. 已知问题(P8+/后续)

- [ ] AI 助教为检索式:语义级"多层次解释"可由外部 LLM 挂点补足(ASSEMBLYLAB_AI_ENDPOINT 约定已留)
- [ ] judge_fill 用例项展示 Rust Debug 格式
- [ ] predict 一次作答锁定的重置入口
- [ ] 图谱节点缩放/拖拽;反汇编函数头混淆名展示;模式 B 固定 x86;ARM64 分支目标相对偏移

## 6. 运行方式(不变)

```
cargo run -p lab-api                     # 后端 127.0.0.1:7600
cd web && npx vite --port 5199           # 前端
浏览器 http://127.0.0.1:5199 → 导航「AI 助教」/「知识图谱」
```

## 7. 待你确认(收官)

1. **验收动线**:「AI 助教」页提问"协程帧里保存了什么?"→ 看三层回答与推荐;「知识图谱」页点黄/红节点看掌握度;「学习中心」→ 综合项目课 → 真机调试解剖 printf;习题中心做 strcpy 大题;导出离线包用浏览器打开验证。
2. **全部 7 个阶段交付完毕**(27 门课/20 道题/8 题型/AI 助教/离线包/知识图谱)。README 十五节总对照见 §4;遗留小项清单见 §5。是否需要 git init + 阶段 tag(ROADMAP §10-2)?或还有想补的内容?

---

## 附:外部审查修复(2026-11-09,第二批后追加)

外部审查指出的问题逐项修复(全部属实):

| 级别 | 问题 | 修复 |
|---|---|---|
| 致命 | gdb.rs:socat 未绑 127.0.0.1、console 通道可执行任意命令(debug_assert release 失效)、超时不杀进程 | TCP-LISTEN 加 `bind=127.0.0.1`;console 命令**白名单**(仅只读 p/ptype/info/x,禁 shell/分号/反引号/$);命令超时 **hard_kill 终止 socat/gdb** |
| 致命 | SECURITY_DESIGN 承诺与交付落差(Job Object 等不存在却默认开放) | 模式 B 路由**默认关闭**(环境变量 `ASSEMBLYLAB_ENABLE_UNSAFE_DEBUG=1` 明确开启,关闭时返回禁用说明);SECURITY_DESIGN 追加「实现状态注记」如实对照 |
| 高 | 判题答案泄漏(quiz expect/testcases 期望值序列化返回) | GET /api/exercises/{id} 剥离 quiz.questions[].expect 与 fill_blank.testcases 的 expect_* |
| 高 | isa_id 路径遍历 + 错误信息回显绝对路径 | isa_id 白名单(小写字母数字连字符);错误信息不回显路径 |
| 高 | 30+ 路由零会话上限;progress 可任意篡改 | 会话数上限 32;进度状态白名单(not_started/in_progress/done) |
| 高 | compiler opt 校验不完整 | 精确白名单 O0..O3 |
| 其他 | disasm_for 未知 ISA 静默按 x86 反汇编 | 改为报错(不产出错误教学数据) |
| 其他 | CompareView 把 aarch64 错标 Win64;DebugView pc_jump 写死 from+1 | 标签映射函数;pc_jump 用实际指令长度(iced 解析) |
| 其他 | memdecode combine 端序逻辑反了(vitest 发现的真实 bug) | 修正 + vitest 8 用例锁定 |
| 文档 | 26/27 矛盾、离线 ✅ 表述过度、README"当前任务"过时、许可证未注记 | 全部修正;README 追加实现状态注记 + unicorn.dll GPL-2.0 许可证注记 |
| 测试 | WSL 依赖测试在沙箱/CI 大面积失败、vitest 0 用例 | WSL 运行时探测跳过(wsl_available);vitest 覆盖 memdecode(8 用例)与 diff/LCS(6 用例);TEST_PLAN 环境依赖说明 |

修复后:19 套件 35 项测试全绿(rc=0);vitest 8/8;vue-tsc 零错误。

---

## 附:P8 可选方向完成(2026-11-09 追加)

| 方向 | 交付 |
|---|---|
| compare 逐行 diff | 恰好两列时「⟷ 逐行对照」切换:归一化文本 LCS 对齐,相同/差异行着色 + 统计(相同 N/差异 M) |
| 模式 B ARM64 远程真机调试 | 强制交叉编译(aarch64-linux-gnu-g++)→ scp 推送 → 远端 gdbserver **只绑 127.0.0.1**、按会话派生端口并写 pid 文件 → Windows ssh -L 隧道 → WSL gdb-multiarch 连本机转发端口;DebugSession 按 ISA 分派(arch 字段 + pc_name()/snapshot_arm64/disasm 分派);API 与前端均有 isa 参数;会话 Drop 精确清理远端进程/目录与本地临时产物 |
| AI 助教外部 LLM 挂点 | `POST /api/tutor/llm` + `GET /api/tutor/llm/status`:system 段注入本地 ISA notes、user 段只放问题;端点强制 https(本机 http 仅调试),密钥只存服务端环境变量;启用 TLS、60s 超时、1 MB 响应上限、token 鉴权与独立限流;未配置返回 503,前端显示配置状态与引导 |

**ARM64 远程调试流程(2026-11-10 修订)**:需同时满足 `ASSEMBLYLAB_ENABLE_UNSAFE_DEBUG=1`、`ASSEMBLYLAB_ENABLE_ARM64_REMOTE=1`,并显式配置 `ASSEMBLYLAB_ARM64_HOST` / `ASSEMBLYLAB_ARM64_USER`(代码中不再内置任何主机地址)+ 外部免密 ssh;aarch64 交叉编译 → `mkdir -p` 远端目录 → scp → `gdbserver 127.0.0.1:<port>` → ssh -L 隧道 → gdb-multiarch `target remote 127.0.0.1:<port>`。
**复现状态(如实标注)**:审查环境 2026-11-10 从本机对该主机 22 端口连接超时,**未能独立复现**「真机断点/单步/寄存器 X0/PC/ARM64 反汇编」;该结论需在具备网络与免密条件的环境按上述流程重新验证,暂不作为已验收项。
**P8 排查记录与修订**:① `ldr =label` 字面池条目留重定位(objcopy 扁平二进制不应用)→ 读出 0,源码一律用 `adr`(±1MB);② 早期 `pkill -f gdbserver` 全局清理会误杀其他会话,已改为按会话 pid 文件精确 kill + Drop 清理;③ 早期 `gdbserver :2345` 监听全网卡且无认证,已改为只绑远端回环 + ssh 隧道;④ 早期默认 `g++` 会产出 x86-64 ELF 推到 ARM 真机,已强制交叉编译器;⑤ 早期 `from+1` 把变长指令误标 branch、用 x86 解码器求 ARM64 指令长度,已改为按 ISA 解码 step 前 PC 处的真实指令。

---

## 附:第二轮审查修复(2026-11-10)

| 组 | 修复内容 |
|---|---|
| 1 | LLM 挂点启用 reqwest TLS(rustls),拒绝非 https 端点;新增 `/api/tutor/llm/status`、前端模式切换与配置引导;question 长度/控制字符校验、system/user 分离、1 MB 响应上限、错误脱敏、未配置返回 503;新增全站 `security::guard`(可选 token、全局/LLM 限流、Origin 校验) |
| 2 | 判题提交响应改用不含 `expected` 的公开 DTO;`answer_md` 改为用户主动 `GET .../answer` 获取;离线包默认不含参考答案;自评题不计入掌握度 |
| 3 | ARM64 远程:去掉硬编码内网主机、独立开关、强制 aarch64 交叉编译器、gdbserver 绑远端回环 + ssh -L 隧道、按会话 pid 精确清理、`Drop` 清理本地/远端资源、前端 isa 入口与默认关闭提示;模式 B StepReport 改用按 ISA 解码的真实指令长度,内存变化不再伪造 |
| 4 | MI `send()` 禁控制字符、观察点表达式白名单、console 禁控制字符、所有超时分支 `hard_kill`、socat 端口随机化;`security::guard` 全路径段白名单防遍历;外部工具统一 `process::run_with_timeout` + 1 MB 截断;模式 A 上限 32 / 模式 B 上限 8 + 30 分钟 TTL;ApiError 错误脱敏 |
| 5 | LCS 抽成 `utils/diff.ts` 纯函数并加 vitest;diffMode 门控 + 1500 行上限 + 行号/仅看差异/视图互斥;TutorView 裸 fetch、KnowledgeView 重复 onMounted、LearnView 错误处理与进度反馈、HomeView 文案全部修正 |
| 6 | 补齐 10 道习题 `answer_md`(20/20);content_tests 增加参考答案全覆盖断言;新增课程正文 golden 机制(`[[lesson.experiment.golden]]`,真实引擎逐步校验)并修正 stage01-lesson01 的矛盾数值;PHASE7/SECURITY_DESIGN/TEST_PLAN/README 数字与承诺如实化 |
| 7(收尾) | `cargo test --workspace` 在无 WSL 环境 rc=0(19 个测试目标全绿,ARM64/模式 B 用例按环境探测跳过并打印 skip);`security`/`tutor_llm`/`process`/`diff`/`memdecode` 单测补齐;清理全部未用导入、占位函数、MI 原始输出 stderr 打印;工作台三列宽度可拖并持久化、新增真实数据驱动的 `StackViewer`、点击反汇编指令关联源码行(模式 A 与模式 B 均已实现);新增 `vitest.config.ts` 解决沙箱下缓存 EPERM 导致测试文件被静默跳过;后端端口可用 `ASSEMBLYLAB_PORT` 配置 |

**仍未完成(如实标注)**:
- 模式 B 的**强沙箱**(Job Object / seccomp / 网络隔离)与 **MI 端口应用层认证**未实现。按 README 要求已默认关闭整条模式 B 路由并返回 403,ARM64 远程另有独立开关;本机单机工具定位下风险可控,但不应用于不可信代码或多租户主机。
- **离线动手实验**:离线包仍只是自包含 HTML 阅读包,模式 A 引擎未 WASM 化。
- **OpenAPI 类型生成**:前后端 DTO 仍为手写对齐(`web/src/api/types.ts`),`utoipa` 依赖已在但未导出 schema。
- **golden 逐课覆盖**:机制可用,目前仅 stage01-lesson01 声明 3 条;其余课程正文数值仍为手写。
- **RISC-V**(README 第三阶段)与工作台独立 AsmPane(模式 A 的源码编辑区即汇编代码区,当前未单列面板)未交付。
