# Phase 7 阶段报告(全部完成)— 学习系统完善

日期:2026-11-09 · 状态:待用户验收(第一批见 git 历史同文件;本版合并第二批)

## 第一批(此前已交付,摘要)

8 种题型判题模型全落地(QuizSpec/SelfCheckSpec 新增)+ 6 道新题 + 知识图谱页(113 节点自动聚合,掌握度四色着色)+ 错题本 + /api/knowledge、/api/mistakes + content_tests 全覆盖。**本版补充修复:ExerciseView 路由复用不响应 ?id=**(同款第 3 处)。

## 第二批(本版新增)

| 任务 | 交付 |
|---|---|
| T7b.1 综合项目课 | project-lesson01「解剖 printf 的完整调用链」:main → PLT(延迟绑定)→ __vfprintf_internal(stdio 缓冲)→ write → syscall;附**可复用的"实际项目关键函数分析"方法论**(带符号编译→调用栈读层→关键指令定位→分层卡片) |
| T7b.2 综合编程大题 | project-q01-programming-strcpy:双用例(RDI 预置源/修正后目标),断言复制后含终止 0 的完整字节(0xCC 填充验证"复制到 0 含 0"边界) |
| T7b.3 AI 助教(本地检索式) | `POST /api/tutor`:**确定性检索引擎**——知识源全部来自仓库真实数据(两架构 ISA notes 74 条/27 门课程正文/20 题解析/知识图谱 113 节点),token 命中打分;回答三层组织(30 秒版/深入版/动手版)+ 盲点分析(真实作答记录聚合)+ 推荐下一步(未做题 top3,lesson 限定排序)。**零模型、零网络、零编造**——README 第九节"AI 必须基于真实信息"由架构保证 |
| T7b.4 助教前端页 | 新导航「AI 助教」(/tutor):提问框 + 三层回答卡片 + 盲点分析 + 推荐习题直达 + 关联知识点/课程跳转 |
| T7b.5 离线课程包导出 | `GET /api/export/course/{id}`(单课)/ `GET /api/export/all`(全 26 门合集):**自包含单文件 HTML**(课程正文+实验源码+习题解析内嵌,file:// 直接可开,零网络);页面 footer 如实标注"动手实验请回平台(真实引擎)" |

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
- curl:/api/tutor("lock cmpxchg")命中 CMPXCHG/lock 前缀节点、三层回答、盲点 3 条(真实作答记录)、推荐 strcpy;`/api/export/course/stage08-lesson01`(6.3KB 单文件 HTML)、`/api/export/all`(175KB 全 26 门合集)
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
| 14 | 十四阶段课程 | ✅ 27 门课覆盖阶段 1-10(每阶段 2-4 门) |
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
| 测试 | WSL 依赖测试在沙箱/CI 大面积失败、vitest 0 用例 | WSL 运行时探测跳过(wsl_available);vitest 覆盖 memdecode(8 用例);TEST_PLAN 环境依赖说明 |

修复后:19 套件 35 项测试全绿(rc=0);vitest 8/8;vue-tsc 零错误。
