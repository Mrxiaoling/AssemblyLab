# Phase 3 阶段报告 — 内存与高级实验

日期:2026-11-09 · 状态:待用户验收

## 1. 完成内容

| 任务 | 交付 |
|---|---|
| T3.1 优化级别对比工作台 | 新页面「优化对比」(/compare):同一 C++ 源码 × O0..O3 并排汇编、每列指令数、函数过滤、`?exp=` 预载;后端 `POST /api/compare`(WSL 内真实 g++ 编译+objdump,产物目录即用即清) |
| T3.2 Win64 ABI 对比 | WSL 安装 mingw-w64 交叉编译器;compare 的 compiler 参数支持 `x86_64-w64-mingw32-g++`,一页切换 SysV(RDI/RSI) vs Win64(RCX/RDX);health 探测并暴露 wsl_gpp/mingw |
| T3.3 类型化内存查看器 | MemoryViewer 升级:byte/int8..64/uint/float32/64/ASCII 十二种视图、端序切换(小端/大端对照)、进制切换(hex/dec/bin)、RSP/RBP 一键跳转;解码逻辑在 `web/src/utils/memdecode.ts`(模式 A/B 共用) |
| T3.4 模式 B 观察点 | gdb `-break-watch` 接入(表达式含空格自动加引号);`PUT /api/debug/sessions/{id}/watchpoints`;触发时 stop_reason 附带 **旧值→新值**(如 `watchpoint-trigger [r: 21845 → 30]`);未启动时自动 run 到 main;DebugView 观察点页签(添加/移除/👁 高亮) |
| T3.5 DebugView 重构 | splitpanes 三栏布局;新增内存页签(类型化视图+RSP/RBP 跳转,看 this/虚表/闭包的真实内存);调用栈/内存/观察点三页签 |
| T3.6 阶段三课程 4 门 | 地址与指针 / 数组遍历 / 结构体布局与对齐 / 端序——全部 15 步模板 + 真实可跑实验 |
| T3.7 阶段四递归课 | stage04-lesson02:汇编 factorial(5),单步观察栈帧五层生长与对称回收 |
| T3.8 阶段五/六课程 3 门 | -O0 到 -O3(experiment mode=compare 直跳对比页)/ this 与成员函数 / 虚函数与虚表 / lambda 闭包(mode=debug 直跳真机调试) |
| T3.9 实验入口路由 | `[lesson.experiment] mode = asm/debug/compare`,LearnView 按模式跳转工作台/真机调试/优化对比(C++ 源码漏写 mode 时自动兜底 debug)——修掉 P2 遗留:stage04-lesson01(C++ 源码)点「打开实验」会误入 NASM 工作台 |
| T3.10 内容自动校验 | `lab-judge/tests/content_tests.rs` 5 项:课程 schema/模式A实验真实跑通/习题结构/判题链路参考答案自检/补全题参考答案——新增内容全被测试锁定 |
| P2 遗留 | marked 输出加 DOMPurify 消毒;DebugView splitpanes(上述 T3.5);Win64 编译器(T3.2) |

课程与习题总数:13 门课(此前 6 门)、8 道题(此前 4 道)。

## 2. 过程中的关键决策与发现

| # | 决策/发现 | 记录 |
|---|---|---|
| D-3-1 | **Unicorn 2.1.4 的自修改代码(SMC)边界**:写入地址落在「当前翻译块」(从 RIP 起的直线代码流,上限 512 字节、被分支/syscall 截断)内时,指令**静默不执行**、RIP 不前进(ctypes 探针另测得误报 WRITE_UNMAPPED)。老课程写 [0x400100] 等不踩线是因为分支使 TB 很短;新课程「数据标签紧跟代码」才触发 | 探针矩阵:写点距执行点 <0x200 卡死、≥0x200 正常;分流/带 syscall 的程序 TB 小不受影响 |
| D-3-2 | SMC 应对 = **三层**:①引擎 step 检测 RIP 停滞 ≥32 次报明确错误;②code hook 连续同址 128 次强制停(防 run 死循环);③课程数据用 NASM `align 512` 把可写数据推到 0x400200(源码注释如实标注这是模拟器边界,真实 CPU 无此限制) | unicorn.rs / stage03 三课 + 端序习题 |
| D-3-3 | **nasm 临时目录并发互踩**:并行测试下 `uid = secs + subsec_nanos` 因 Windows 时钟精度不足撞车,两测试互写对方源文件(串行全过、并行必挂)。修复:进程内原子序号 ⊕ 纳秒时间 | nasm.rs;修复后并行 3 连全绿 |
| D-3-4 | Win64 编译器选 **WSL mingw-w64** 而非 Windows 侧 MSVC cl(需 vcvars 环境且 objdump 不通用):交叉编译 PE 产物、ABI 与 MSVC 一致(Win64 约定),objdump/c++filt 全链复用 | compiler.rs 白名单校验 compiler 名 |
| D-3-5 | judge 的 predict 快照只含**面板级寄存器**(RAX/RBX…),子寄存器(EAX/EBX)不在其中——习题出题需用面板寄存器名(顺带让学员看到 movzx 零扩展) | content_tests 自检发现并修正 |
| D-3-6 | wsl_probe(Phase 2 管道诊断探针)依赖手工目录且故意复现 30s 延迟,曾拖死全量测试——已 `#[ignore]` 标注,`-- --ignored` 可复跑 | wsl_probe.rs |

## 3. 测试结果

- 后端 **32 项测试全绿(19 个套件,`cargo test --workspace` rc=0)**:unicorn 7 · nasm 5(并行 3 连验证)· compiler 4(真实 WSL g++)· gdb/MI 2(真实断点/单步/帧/变量)· iced 1 · ISA 3 · 引擎会话 5 · **内容校验 5(新增)**
- curl 验收:compare(SysV O0=18/O2=7 指令对照;Win64 O2 参数走 RCX)· 观察点链路(旧值→新值)· 端序/指针课程程序逐步正确(RAX=0x44/0x2a)
- 浏览器端到端:优化对比页四列(O0 26 / O1 18 / O2 20 / O3 81 指令,O3 为向量化展开)· DebugView 添加观察点 `r` → 继续命中 `👁 watchpoint-trigger [r: 21845 → 30]` · 工作台内存面板切 int32 视图正常解码 · 学习中心 13 门课、C++ 课「打开本课实验」正确跳转 `/#/debug?exp=…`
- 前端 `vue-tsc --noEmit` 零错误 + `vite build` 通过

## 4. README 十五节验收对照(新增)

| # | 标准 | 状态 |
|---|---|---|
| 4 | 修改变量/内存观察影响 | ✅ 模式 A 内存编辑+类型化视图;模式 B 观察点(数据断点,旧值→新值) |
| 9 | 优化级别差异对比 | ✅ 优化对比工作台(O0..O3 × SysV/Win64) |
| 10 | 指针/结构体/对齐/端序实验 | ✅ 阶段三 4 课 + 类型化内存视图 + 3 道配套习题 |
| 11 | 递归与栈帧 | ✅ stage04-lesson02 汇编递归逐帧观察 |
| — | C++ 对象模型(this/虚表/lambda) | ✅ 阶段六 3 课(真机调试,虚表可从内存面板直接读) |

## 5. 已知问题(P4 处理)

- [ ] utoipa→openapi-typescript 类型生成仍未接入(前端类型继续手写对齐)
- [ ] 模式 B 反汇编面板函数头仍显示混淆名(text 用 objdump 原行;function 字段已去混淆,仅展示层小瑕疵)
- [ ] compare 未做逐行/逐函数跨列 diff 对齐(V1 为并排+指令数+函数过滤)
- [ ] run 模式遇 SMC 代码会在 hook 层 128 次后停止(报错文案已明确,但不自动定位写点)
- [ ] DebugView 内存页签不支持编辑写回(模式 A 工作台支持)

## 6. 运行方式(同前)

```
cargo run -p lab-api                      # 后端 127.0.0.1:7600
cd web && npx vite --port 5199            # 前端
浏览器 http://127.0.0.1:5199 → 导航「优化对比」/「真机调试」/「学习中心」
```

## 7. 待你确认(进入 Phase 4 前)

1. **Phase 3 验收建议动线**:
   - 「优化对比」页直接点「编译对比」→ 看 O0 与 O2 的 sum_to 差别;把编译器切到 Win64 看参数寄存器变 RCX;
   - 学习中心打开「3·端序」→ 打开实验 → 工作台跑到写入指令后,内存面板用 uint32/字节视图切换看 44 33 22 11;
   - 「6·虚函数与虚表」→ 打开实验(自动进真机调试)→ 断进 f,寄存器 RDI 的值填进内存面板用 uint64 看对象头 vptr。
2. **SMC 边界标注方式**:当前在课程源码注释与引擎报错中如实标注「模拟器边界」;若你希望改为"默认给数据垫 padding 对用户透明",下阶段可以调整。
3. Phase 4 范围:C++20 协程实验室(协程帧 DWARF 解析、状态机示意、挂起/恢复时间线)——按 ROADMAP §4 执行即可?确认后进入 Phase 4。
