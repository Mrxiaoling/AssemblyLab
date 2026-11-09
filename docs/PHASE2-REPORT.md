# Phase 2 阶段报告 — C++ 与汇编联动 + 真机调试

日期:2026-11-08 · 状态:待用户验收

## 1. 完成内容

| 任务 | 交付 |
|---|---|
| T2.0 WSL2 工具链 | Ubuntu-22.04 内安装 g++ 11.4.0 + gdb 12.1 + socat(root 免密,无交互) |
| T2.1 CompilerAdapter | WSL g++ 调用(-g/-O0..O3)、错误行号映射、ulimit+timeout 包裹、临时目录生命周期(会话清理 + 启动清扫) |
| T2.2 源码↔汇编映射 | objdump -d -l -M intel 解析(Compiler Explorer 同思路);指令级源码行标注 + c++filt 符号去混淆 |
| T2.3 GdbMiAdapter | gdb --interpreter=mi2 经 socat TCP 传输;自写 MI 值解析器(元组/列表/C串,含防停滞护栏);命令白名单 |
| T2.4 模式 B 会话 | DebugSession(断点/运行/指令级步入步过步出/寄存器快照/内存);DebugView 页面(C++ 编辑器、行号断点、反汇编+源码行、寄存器、调用栈与变量、解释卡、隔离等级标注) |
| T2.5 栈帧可视化 | 调用栈列表(level/函数/文件:行)+ 当前帧局部变量(真实值) |
| T2.6 阶段四课程 | 「追踪一次完整的函数调用」——README 核心场景在真机实现:call 前的 RDI/RSI 装参、call 压返回地址、序言建栈帧、SysV 约定 |
| P1 遗留 | 课程页 Markdown 渲染(marked);步入/步过/步出(模式 B) |

## 2. 过程中的关键决策与发现

| # | 决策/发现 | 记录 |
|---|---|---|
| D-2-1 | **wsl.exe stdin 管道中继存在 ~30s/命令的投递延迟怪癖**(首条命令即时,后续全部延迟)——探针实证后,传输层改为 **WSL 内 socat 把 gdb/MI 挂 TCP**,后端 TcpStream 直连,问题彻底消除 | 首条即时/后续 30s 的规律、探针对照实验均记录于测试 wsl_probe.rs |
| D-2-2 | MI 结果分类必须**先解析 token 前缀**(`12^done`)再查特殊前缀——曾把带 token 的响应当未知行丢弃,造成全部命令"超时" | gdb.rs classify() |
| D-2-3 | gdb 寄存器名是 `eflags`,面板显示映射为 `RFLAGS` 与模式 A 对齐 | gdb.rs snapshot() |
| D-2-4 | 源码映射选 objdump -l 而非 gimli 直解 DWARF(文本解析更贴近 Compiler Explorer 实践,gimli 留给 Phase 4 协程帧) | compiler.rs |
| D-2-5 | 端口基址按后端进程 ID 派生——避免重启后撞上 WSL 残留僵尸 socat | gdb.rs next_gdb_port() |
| D-2-6 | build_report 禁止再入 gdb 锁(曾死锁 run 请求) | debug.rs |

## 3. 测试结果

- 后端 **27 项测试全绿**:unicorn 7 · nasm 5 · compiler 4(真实 WSL g++)· **gdb/MI 2(真实断点/单步/帧/变量)** · iced 1 · ISA 3 · 引擎会话 5
- curl 验收:编译→断点(源码行)→run 命中(source_line=6)→步入落至 `mov edi,0Ah`→调用栈 [add, main]→变量(真实未初始化值 r=21845)
- 浏览器端到端:DebugView 启动调试→自动断 main→步入 4 次进入 add→调用栈/变量/解释卡全部真实数据
- 教训记录:一次一小时的"陈旧模块"误诊(Phase 1)与本次 wsl 管道怪癖,共同验证了「探针对照实验」是最快定位手段

## 4. README 十五节验收对照(新增)

| # | 标准 | 状态 |
|---|---|---|
| 5 | 观察栈帧变化 | ✅ 调用栈+局部变量(真机);栈内存标注 P3 深化 |
| 6 | 查看真实 C++ 编译产生的汇编 | ✅ |
| 7 | 调试真实 C++ 程序 | ✅(断点/单步/帧/变量,WSL 真机) |
| 8 | 理解函数调用与 ABI | ✅ 阶段四第一课 + SysV 真机观察(Win64 对比 P3) |
| 12 | x86-64/ARM64 切换 | P5 |

## 5. 已知问题(P3 处理)

- [ ] DebugView 未复用 splitpanes 布局(独立紧凑布局)
- [ ] 模式 B 内存写入事件缺失(无 mem hook;P3 用观察点补)
- [ ] Win64 ABI 对比实验需 Windows 侧编译器(MSVC cl /FA 或 LLVM 安装后接入)
- [ ] marked 渲染本地可信内容,未加 sanitize(内容来源为仓库内文件)
- [ ] utoipa 类型生成仍未接入(前端类型手写对齐)

## 6. 运行方式(同 Phase 1,新增真机调试入口)

```
cargo run -p lab-api                     # 后端(需 WSL2 Ubuntu-22.04 + g++/gdb/socat,已装)
cd web && npx vite --port 5199           # 前端
浏览器 http://127.0.0.1:5199 → 导航「真机调试」
```

## 7. 待你确认(进入 Phase 3 前)

1. Phase 2 验收(建议在「真机调试」页单步观察一次完整函数调用)。
2. **一件抱歉的事**:排查 wsl 管道问题时,我清理 node 进程误杀了你在 5173 的 UnderTheHood Lab dev server(当时有多个僵尸 vite 实例难以区分)。它需要你手动重启一次;之后我所有清理都按端口 PID 精确操作。
3. Phase 3 范围:内存查看器增强/指针/结构体/递归/优化级别对比/C++ 对象模型实验——按 ROADMAP §3 执行即可?
