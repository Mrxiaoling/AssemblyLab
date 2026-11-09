# Phase 1 阶段报告 — 最小可用实验室

日期:2026-11-08 · 状态:待用户验收

## 1. 完成内容

| 里程碑 | 交付 |
|---|---|
| M1 引擎骨架 | cargo workspace(6 crate);Unicorn 适配层(预编译 DLL 运行时加载,常量代码生成);NASM 适配(行号/符号映射);iced 反汇编;Session/StepReport/轨迹;axum API 全链路 |
| M2 工作台 | Vue3 + Vite + Pinia + Naive UI + Monaco(自写 x86-nasm 语法高亮)+ splitpanes;八面板工作台:控制栏/源码/反汇编(断点点击)/寄存器(分组+变化高亮+双击修改)/标志位/内存(hex+ASCII+编辑)/轨迹/解释卡 |
| M3 课程与习题 | SQLite 进度存储;课程框架(lesson.toml + 内容流渲染 + 实验锚点);首批 3 课;习题引擎(预测/补全两题型,确定性判题 + 提示梯度 + 参考解析);学习中心与习题中心页面 |

## 2. 架构与代码变更

```
crates/
├── lab-core        领域类型、错误、ISA 数据模型(含 deny_unknown_fields 严格校验)
├── lab-adapters    unicorn(运行时 FFI)+ nasm + iced + consts(生成)
├── lab-engine      Session、StepReport(diff+解释分层)、轨迹、判题会话
├── lab-judge       预测/补全判题(隔离会话真实执行)
├── lab-storage     rusqlite(lesson_progress / exercise_attempt)
└── lab-api         axum 全路由(会话/ISA/课程/习题/进度)+ /api/health 工具探测
data/
├── isas/x86-64/isa.toml         寄存器(18 面板+子寄存器写语义)/标志/双 ABI/30 条指令注释
├── courses/*.toml               3 课(15 步模板)
└── exercises/*.toml             4 题
web/                              Vue3 前端(7 视图/组件文件族)
tools/                            gen_unicorn_consts.py + nasm 2.16.03
vendor/unicorn.dll                GPL-2.0,来源 PyPI wheel(见 .gitignore 放行)
```

## 3. 测试结果

- **后端 21 项测试全绿**:unicorn 语义 7(单步/标志/宿主改寄存器/syscall 桩/断点/未映射/ret 终止)+ nasm 5 + ISA 3 + iced 1 + 引擎会话 5
- **curl 验收**:会话→单步(diff+解释)→运行(exit 7)→断点→改寄存器→单步续跑→内存/轨迹/反汇编 全链路
- **浏览器端到端(实测截图)**:工作台单步 3 次观察 RAX/PF diff 与解释卡;断点设置→运行→精确停在 0x40000a;课程→「打开本课实验」预载源码;习题错误答案→逐项对比+梯度提示(第 3 次给 level2),正确答案→通过+解析,补全题→真实执行判题
- 前端 vue-tsc 类型检查 + 生产构建通过

## 4. 过程中的关键修复(都有测试或截图佐证)

1. `uc_mem_map` 4 参数版 API(常量生成策略因此确立)
2. INSN hook 触发时 RIP 已自动推进(syscall 桩不得手动 +2)
3. code hook 命中停止必须调 `uc_emu_stop`(否则多执行一条指令)
4. 单步无视断点(bp_enabled 标志,step-over 语义)
5. 终态语义(断点≠终态,可继续;退出/错误/上限=终态)
6. naive-ui `useMessage` 无 provider 崩溃(曾伪装成 Vue 补丁层诡异错误)
7. vite 陈旧转换误诊一小时的教训:三次自查失误(单引号正则、漏改模板字段、静默 replace 失配)——**验证必须用双独立来源**

## 5. 已知问题(P2 处理)

- [ ] 解释卡/课程正文的 Markdown 以纯文本渲染(无粗体/表格)
- [ ] 反汇编把尾部数据字节当代码显示(真实行为,待标注数据区)
- [ ] OpenAPI(utoipa)未接——前端类型手写,M2 末期计划改为生成
- [ ] 单步批量 UI(单步 ×N 输入框已有,步入/步出按钮未做——引擎已支持语义)
- [ ] 课程正文数值未接 golden 模板( TEST_PLAN §5.4 机制,内容量增大前落地)

## 6. README 十五节验收对照(P1 范围)

| # | 标准 | 状态 |
|---|---|---|
| 1 | 输入汇编并编译运行 | ✅ |
| 2 | 逐条执行指令 | ✅ |
| 3 | 观察寄存器变化(前后+解释) | ✅ |
| 4 | 观察内存变化 | ✅(hex+ASCII+写入事件;类型化视图 P3) |
| 10 | 实验自动反馈(动手校验) | ⚙️ 部分(习题判题✅;实验内校验 P2) |
| 11 | 习题检验(2 题型) | ✅ |
| 13 | 离线使用 | ✅(后端+课程+判题全离线) |

## 7. 运行方式

```
# 后端(需要 tools/nasm/ 与 vendor/unicorn.dll,均已就位)
cargo run -p lab-api          # http://127.0.0.1:7600
# 前端
cd web && npm install && npx vite --port 5199   # http://127.0.0.1:5199
# 测试
cargo test --workspace
```

## 8. 待你确认(进入 Phase 2 前)

1. Phase 1 验收是否通过(建议亲自按「运行方式」体验 10 分钟)。
2. P2 的 System V 真机实验路线:安装 WSL2 内工具链(推荐,`apt install build-essential gdb`)还是仅 Windows 原生?
3. 前端类型是否切换为 OpenAPI 生成(M2 末期计划项)。
