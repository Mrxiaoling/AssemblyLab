# COURSE_DESIGN — 课程体系与内容工程

> Phase 0 设计文档 · 状态:待确认
> 上游需求:README 第三、五、七、十一节。核心立场:场景驱动,不允许无实验的章节。

## 1. 设计原则

1. **每节课从一个真实问题/程序开始**,禁止「先列寄存器表再讲指令」式组织(README 第一节红线)。
2. **每节课必有实验与习题**;实验必须真实可跑(构建时强制校验,见 TEST_PLAN.md §5)。
3. **课程数据是结构化文件**,教学者(含 AI 会话)可以增量撰写、版本化、离线分发。
4. **教学模板固定 15 步**(README 第十一节),字段化进 schema,UI 按步渲染,避免每课自创结构。

## 2. 十阶段课程与实验映射(总表)

| 阶段 | 课程(每阶段 3–6 课) | 代表实验 | 交付 Phase |
|---|---|---|---|
| 1 认识执行 | ①一个 C++ 程序的一生 ②CPU 如何执行指令 ③汇编/机器码/反汇编 | 编译管线观察;逐指令跑 `a+b=c`;同一程序 Debug/Release 汇编对比 | P1(①②)、P2(③,接真实编译) |
| 2 寄存器与基础指令 | ①数据在寄存器间移动 ②算术与标志 ③位运算 ④比较与跳转 | 变量交换;整数计算器;绝对值;最大最小;溢出观察(有符号/无符号对照) | P1(①②③) |
| 3 内存与寻址 | ①地址与指针 ②数组遍历 ③结构体布局与对齐 ④端序 | 汇编遍历数组求和;读结构体成员;改指针观察地址;类型化内存视图 | P3 |
| 4 栈与函数调用 | ①PUSH/POP 与栈 ②CALL/RET 全程追踪 ③栈帧与局部变量 ④参数传递与返回值 ⑤递归 | 追踪 `add(10,20)` 完整调用;观察返回地址;递归 factorial 栈帧动画;SysV vs Win64 对比 | P2/P3 |
| 5 编译器与汇编 | ①-O0 到 -O3 ②内联与常量折叠 ③寄存器分配 ④循环优化 | 同一函数四个优化级别对比;无用代码消除观察 | P3 |
| 6 C++ 对象模型 | ①this 与成员函数 ②虚函数与虚表 ③lambda 闭包 ④RAII 与析构顺序 | 虚调用 vs 普通调用汇编对比;虚表内存视图;lambda 捕获结构 | P3 |
| 7 C++20 协程 | ①协程帧与状态机 ②co_await 的展开 ③挂起与恢复 ④协程调度 | `Task foo()` 逐指令;帧布局 DWARF 解析;挂起/恢复时间线;clang vs gcc 对比 | P4(专题页) |
| 8 上下文切换与 OS | ①syscall 之旅 ②函数调用 vs 线程切换 ③中断入口 | Linux write/exit syscall 逐步;上下文切换原型汇编 | P6 |
| 9 SIMD/原子/并发 | ①SIMD 向量加法 ②CAS 与 LOCK ③内存屏障 | 标量 vs SSE 数组加法;std::atomic 汇编;自旋锁实现 | P6 |
| 10 ARM 与嵌入式 | ①AArch64 基础 ②C 调汇编 ③启动代码与中断向量 | x86-64 vs ARM64 同功能对比;最小启动代码分析 | P5 |

每阶段的完整课表在对应 Phase 开工时细化,本文档只锁定**结构与方法**。

## 3. 15 步教学模板 → lesson.toml schema

```toml
# lesson.toml
[lesson]
id = "stage02-lesson02"
title = "算术与标志位:计算器为什么知道溢出了"
stage = 2
requires = ["stage01-lesson02", "stage02-lesson01"]   # 依赖 DAG(构建校验)
estimated_minutes = 45

[lesson.goal]
problem = "用汇编实现一个整数计算器,并解释溢出发生时 CPU 知道什么"
outcomes = ["读写 ADD/SUB 及 CF/ZF/SF/OF", "区分有符号/无符号溢出的判定"]

# 15 步模板:每步是 {kind, payload};kind 决定渲染组件
steps = [
  { kind = "problem" },      # 1 本节要解决的问题
  { kind = "scenario" },     # 2 实际场景
  { kind = "run" },          # 3 先运行一个程序 → 嵌入实验锚点
  { kind = "observe" },      # 4 观察程序行为 → 嵌入观察清单
  { kind = "asm" },          # 5 查看汇编 → 嵌入实验
  { kind = "walkthrough" },  # 6 逐条分析指令 → 嵌入实验(逐步模式)
  { kind = "state" },        # 7 查看寄存器和内存 → 嵌入实验
  { kind = "theory" },       # 8 原理讲解 → content.md 章节
  { kind = "cpp-link" },     # 9 与 C/C++ 的联系
  { kind = "pitfalls" },     # 10 常见误区
  { kind = "hands-on" },     # 11 动手实验 → 实验锚点(带任务与校验)
  { kind = "exercise" },     # 12 课后习题 → 习题锚点
  { kind = "summary" },      # 13 总结
  { kind = "links" },        # 14 关联知识 → 知识图谱节点 id 列表
  { kind = "next" },         # 15 下一步建议
]

[lesson.experiments]
swap = "experiments/calc.asm.toml"     # 实验定义文件
```

`content.md` 提供每步的正文;正文中的关键数值用 golden 模板引用(见 TEST_PLAN.md §5.4),示例:

```markdown
执行 `add rax, rbx` 后,{{exp:calc:step:3:reg:RAX}} = {{exp:calc:step:3:reg:RAX:val}},
标志位变化:{{exp:calc:step:3:flags}}。
```

## 4. 实验定义 schema(experiments/*.toml)

```toml
[experiment]
id = "stage02-calc"
title = "整数计算器:加、减与溢出"
isa = "x86-64"
abi = "sysv-amd64"
mode = "emulator"                  # Phase 2 起支持 "debug"

[experiment.source]
initial = '''
; 计算器:计算 10 + 20,再减 5
mov eax, 10
mov ebx, 20
add rax, rbx
mov [result], rax
sub eax, 5
...
'''
editable = true                     # 用户可修改代码
editable_regions = []               # 可限定可编辑区(习题用)

[experiment.observation]            # 「观察目标」驱动 UI 提示
watch_regs = ["RAX", "RBX"]
watch_flags = ["CF", "OF", "ZF"]
watch_mem = [{ addr = "result", note = "结果存放处" }]

[experiment.checks]                 # 动手实验的自动校验(判断完成)
- { kind = "reg", reg = "RAX", after = "run", value = "expr:expected" }
- { kind = "visited_line", line = 5 }        # 必须执行过某行

[experiment.golden]                 # 由引擎生成、人工复核(见 TEST_PLAN)
steps_file = "checks/expected-01.json"
```

## 5. 知识图谱 schema(data/knowledge-graph.toml)

```toml
[[node]]
id = "fn-call"
title = "函数调用"
kind = "concept"          # concept | instruction | register | experiment | exercise | lesson
[[node]]
id = "call-ret"
title = "CALL / RET"
kind = "instruction"
parent = "fn-call"

[[edge]]
from = "fn-call"  to = "stack-frame"  rel = "requires"
[[edge]]
from = "fn-call"  to = "abi"          rel = "relates"
[[edge]]
from = "fn-call"  to = "coroutine"    rel = "relates"   # 协程上下文关联(README 知识关联要求)
```

- 课程 `links` 步、习题 `related` 字段、实验 `observation` 都引用节点 id;图谱页与课程页双跳转。
- 图谱构建时校验:所有被引用 id 存在、无悬空边(接 TEST_PLAN §5.5 同一套检查)。

## 6. Phase 1 首批课程详案(开工基准)

### L1(stage01)「一个加法程序的一生」

- 场景:从 `int c = a + b;` 追问「CPU 到底在做什么」。
- 实验 A(只读跑一遍):预置 x86-64 汇编 `a+b=c`,逐步执行,观察 RIP 前进、寄存器装数、内存写结果。
- 实验 B(动手):改 a/b 的值,先预测寄存器变化再验证(预测即习题题型 1 的第一次使用)。
- 教学点:RIP 的取指-执行循环、寄存器 vs 内存、为什么 CPU 不直接执行 C++。
- 习题:预测结果 ×1、选择题 ×2(取指/执行/PC 概念)、动手实验 ×1。
- 明确标注:本课在模拟器中运行(指令语义真实,内存布局是教学沙箱,不是真实进程)。

### L2(stage02)「数据如何在寄存器与内存间移动」

- 场景:交换两个变量的三种实现(带第三个寄存器 / XOR / 内存暂存)。
- 实验:三种实现逐步对比(指令数、内存访问次数)。
- 教学点:MOV 子寄存器写语义(写 EAX 清零高 32 位——用 golden 展示)、MOV 不能内存到内存。
- 习题:补全汇编(交换)×1、预测结果 ×2、Bug 修复(XOR 交换同地址陷阱)×1。

### L3(stage02)「算术与标志位」

- 场景:计算器溢出了会怎样?(引用现实中 CVE/游戏刷分溢出案例做 scenario)
- 实验:ADD 到 0x7FFF…F、SUB 下溢、有符号/无符号对照视图(同一条 SUB,两种解读)。
- 教学点:CF/OF/ZF/SF 语义、条件跳转的两种视角。
- 习题:预测(标志位三连)×1、C++ 转汇编分析 ×1、综合编程(绝对值)×1。

每课交付物:lesson.toml + content.md + ≥1 实验 + golden + ≥3 习题 + 判题用例。

## 7. 协程专题(阶段 7 / Phase 4)内容设计原则

1. **一切素材来自真实编译产物**,页面标注编译器与版本(如 `clang 19.1.0 -std=c++20 -O0 -g --target=x86_64-linux-gnu`),不同编译器布局不同,不得混写(GCC 14+ 提供 `-fdump-coroutine`/GIMPLE dump,Clang 提供 `__coro_frame`/`__promise` DWARF 变量,均已调研确认)。
2. 明确区分「C++ 标准要求的语义」与「编译器实现选择」,页面分栏呈现(标准条款引用 vs 该编译器的汇编)。
3. 可视化组成(专题页):C++ 源码 | 等价状态机示意(由平台根据该编译器产物分析生成,标注「实现示意」)| 反汇编 | 当前执行位置 | Coroutine Frame 内存布局(来自 DWARF) | Promise/Awaiter 状态 | 挂起/恢复时间线(可逐步播放)。
4. 帧布局读取用 gimli 解析 DWARF;`resume()` 行为用真实 gdb(或模式 A 模拟器加载协程代码)演示。
5. 实验序列按 README 第七节 10 个观察点逐一定制,每个观察点都有对应的实验校验。

## 8. 教学质量规则落实表(README 十一节 10 条)

| 规则 | 落实机制 |
|---|---|
| 1 不允许只有文字没有实验 | lesson schema 强制 steps 含 run+walkthrough+hands-on;构建校验 |
| 2 不允许只讲指令定义 | 模板 steps=problem/scenario 开头;审查清单 |
| 3 不允许只给结果不给过程 | walkthrough 步 + golden 全程轨迹 |
| 4 不把编译器实现当标准 | 「标准/实现」双栏标注规范;协程专题特别执行 |
| 5 不混淆不同架构 | ISA 数据包隔离(ISA_DESIGN);每课锁定 isa 字段 |
| 6 实验代码经过验证 | 构建时真实汇编+执行(TEST_PLAN §5.1) |
| 7 参考答案经过测试 | 判题用例 + golden 复核流程 |
| 8 复杂原理逐步讲解 | walkthrough 状态化呈现;协程/调用专题分步设计 |
| 9 每章节有实验和习题 | schema 必填;content-check 校验 |
| 10 知识依赖关系 | requires DAG 校验 + 学习首页推荐按拓扑序 |

## 9. 决策记录

| # | 决策 | 备选 | 理由 |
|---|---|---|---|
| D-CR-1 | 课程 = TOML+MD 数据文件 | 数据库、纯前端硬编码 | 离线、版本化、教学者直接编辑;README 要求「结构化数据管理」 |
| D-CR-2 | 15 步模板字段化,UI 按 kind 渲染 | 自由 markdown | 强制质量底线(每课必有实验/习题/关联);渲染组件可复用 |
| D-CR-3 | 正文关键数值模板化引用 golden | 手写数字 | 教学内容与真实执行强绑定(README 十二节) |
| D-CR-4 | 知识图谱静态 TOML 起步 | 图数据库 | 规模(百级节点)远不需要图库;后续需要交互编辑再升级 |

## 10. 待确认项

1. [ ] content.md 嵌入实验锚点的最终语法(当前倾向 `{{exp:<id>}}` 行内块)。
2. [ ] 首批 3 课(L1–L3)的详案是否按 §6 执行。
3. [ ] 阶段 1 是否需要「C++ 程序编译管线」的纯展示内容(Phase 1 无真实编译器时,用预生成产物 + 标注来源,Phase 2 换成实时编译)。
