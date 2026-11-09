# TECH_STACK — 技术选型

> Phase 0 设计文档 · 状态:待确认
> 上游需求:README 第十节「技术架构要求」。原则:强类型、模块化、可长期维护;先调研现有方案,不为技术而技术。

## 1. 结论速览

| 层 | 选型 | 版本基线 | 一句话理由 |
|---|---|---|---|
| 后端语言 | Rust(stable,MSVC host) | 本机 1.96.1 | README 建议;本机已就绪 |
| 异步运行时 | Tokio | 1.x | README 建议,生态事实标准 |
| Web 框架 | Axum | 0.8 | README 建议;tower 生态 |
| 序列化 | Serde + serde_json | 1.x | README 建议 |
| 数据库 | SQLite(rusqlite, bundled) | SQLite 3.4x | 本地零运维,离线可用 |
| API 类型源 | utoipa(OpenAPI 3)→ openapi-typescript 生成前端类型 | 5.x | 前后端类型单一来源,README 要求 8 |
| 模拟引擎 | Unicorn 2.1.4(官方预编译 DLL,运行时加载) | 2.1.4 | spike 已验证(见 EXPERIMENT_ENGINE.md §8) |
| 反汇编(x86) | iced-x86(纯 Rust) | 1.22 | MIT、活跃、解码信息全 |
| DWARF 解析 | gimli + object | 0.32 / 0.38 | 纯 Rust、活跃;源码映射与协程帧解析 |
| 进程限制(Windows) | win32job(Job Object 封装) | 0.4 | 模式 B 资源限制 |
| 日志 | tracing + tracing-subscriber | 0.1/0.3 | Axum 原生集成 |
| 前端框架 | Vue 3 + TypeScript + Vite | 3.5 / 5.x / 7.x | README 建议 |
| 状态管理 | Pinia | 3.x | Vue 3 官方推荐 |
| UI 组件库 | Naive UI | 2.x | 深色 IDE 风格友好、TS 原生、Vue3 专用 |
| 代码编辑器 | Monaco Editor | 0.5x(vite 插件加载) | README 指定 |
| 面板布局 | splitpanes | 4.x | 可拖拽调宽的多面板,轻量 |
| 知识图谱(Phase 7) | @vue-flow/core | 1.x | Vue 生态、可点击节点 |
| 单元测试 | cargo test + vitest | — | 前后端各自原生 |

## 2. 本机现状(2026-11-08 实测)

| 工具 | 状态 | 说明 |
|---|---|---|
| rustc/cargo 1.96.1(x86_64-pc-windows-msvc) | ✅ | 已装 |
| Node 20.18.3 / npm 10.8.2 | ✅ | 满足 Vite 7(需 ≥20.19,Vite 6 亦可,装依赖时确认) |
| CMake 3.28.1 | ✅ | 已装(spike 路线 A 需要,路线 B 不需要) |
| Visual Studio 18 Community(MSVC 链接器) | ✅ | Rust MSVC 工具链可用 |
| Python 3.13.1 | ✅ | 脚本/测试辅助 |
| gcc / g++ / clang / gdb / nasm | ❌ 缺失 | Windows 侧未安装,见 §5 安装指引 |
| WSL2 + Ubuntu-22.04(x86_64) | ✅ 已装 | 发行版内无 gcc/gdb,Phase 2 前需 `apt install build-essential gdb` |
| docker-desktop(WSL2) | 存在 | 备选沙箱方案,非必需 |

> Node 20.18.3 与 Vite 7 要求(≥20.19)差一个补丁号:实施时优先固定 Vite 6(无需升级 Node),或将 Node 升级到 22 LTS——待确认,默认选 Vite 6。

## 3. 关键决策记录

| # | 决策 | 备选 | 理由 |
|---|---|---|---|
| D-TS-1 | Unicorn 采用官方预编译 DLL + 运行时加载(手写薄 FFI,~10 个函数) | ① crates.io `unicorn-engine` 2.1.5;② 自研解释器 | ① 依赖 bindgen(需 libclang,本机无)+ cmake 源码编译,且 crate 为 GPL-2.0;预编译 DLL 运行时加载零构建依赖、spike 已实证可用。自研解释器作为后备保留(见 EXPERIMENT_ENGINE.md §8 决策记录) |
| D-TS-2 | 反汇编用 iced-x86(x86 阶段) | capstone-rs(需编译 C 库)、yaxpeax-x86 | iced-x86 纯 Rust、MIT、提供 Intel 语法格式化与完整指令元数据(操作数、flags 读写),恰好是教学解释层需要的 |
| D-TS-3 | 数据库用 rusqlite(bundled SQLite) | sqlx、diesel | 本地单用户无网络;sqlx 的编译期校验价值在多开发者团队才明显;rusqlite 同步 API + `spawn_blocking` 包装足够简单 |
| D-TS-4 | API 契约用 utoipa 生成 OpenAPI,前端用 openapi-typescript 生成 TS 类型 | TS-RS、手写双份类型 | OpenAPI 可直接在浏览器调试(Swagger UI);生成器活跃;README 要求「前后端接口明确类型定义」 |
| D-TS-5 | GDB/MI 集成自写轻量解析器(Phase 2) | gdbmi crate(0.0.2,实验期)、serde_gdbmi(低活跃) | MI 协议是基于行的简单文本协议,自写 ~300 行可控可测;现有 crate 均不成熟,不值得引入依赖风险 |
| D-TS-6 | UI 组件库 Naive UI | Element Plus、Ant Design Vue、TDesign | 深色技术风契合;组合式 API 原生;主题定制(JS 对象而非 CSS 变量 hack) |
| D-TS-7 | 源码↔汇编映射走 DWARF(gimli),辅以 `objdump -d -l` | 仅解析汇编文件内嵌行号注释 | DWARF 是结构化真值;Compiler Explorer 实践已验证该路线 |
| D-TS-8 | 协程可视化基于编译器真实产物:Clang `__coro_frame`/`__promise` DWARF + GCC `-fdump-coroutine`/GIMPLE dump + 反汇编 | 纯示意图、CppInsights 风格源到源转换 | README 明确「不能伪造」;真实产物 + 编译器版本标注(见 COURSE_DESIGN.md 协程专题) |

## 4. 许可证矩阵(影响分发,不影响本地使用)

| 组件 | 许可证 | 分发影响 |
|---|---|---|
| Unicorn Engine(含预编译 DLL) | **GPL-2.0** | 本地个人使用无义务;若未来以二进制分发整个后端,后端需遵循 GPL-2.0 开源;教学数据文件不 受影响 |
| iced-x86 | MIT / Apache-2.0 | 无 |
| gimli / object | MIT / Apache-2.0 | 无 |
| Axum / Tokio / Serde / rusqlite | MIT / Apache-2.0 | 无 |
| utoipa | MIT / Apache-2.0 | 无 |
| win32job | MIT | 无 |
| Vue / Vite / Pinia / Naive UI / Monaco / splitpanes | MIT | 无 |
| SQLite | Public Domain | 无 |
| NASM | BSD-2-Clause | 无 |
| GCC / GDB(binutils) | GPL-3 | 仅作为外部进程调用,不链接、不分发其代码,无传染 |

> GPL-2.0 的 Unicorn 是当前栈中唯一的强许可组件。**若未来有闭源分发需求**,后备路径是自研教学解释器(替换 EmulatorAdapter 实现,接口已隔离)。此为已识别风险,不是阻塞项。

## 5. 工具链安装指引(Windows 主路径,执行时按需安装)

| 工具 | 命令 | 注意 |
|---|---|---|
| NASM(Phase 1 必需) | `winget install -e --id NASM.NASM` | 官方已知问题:不会自动加入 PATH,需手动把 `C:\Program Files\NASM` 加入 Path 并重开终端,`nasm -v` 验证 |
| LLVM/Clang(Phase 2+,交叉汇编 ARM64 与 DWARF 工具) | `winget install -e --id LLVM.LLVM` | 同时提供 llvm-objdump / llvm-dwarfdump / lldb 可选 |
| MinGW-w64 工具链(Phase 2+,提供 Linux 风格 gdb/ELF 编译) | MSYS2: `pacman -S mingw-w64-ucrt-x86_64-toolchain` | 或全部走 WSL2(推荐) |
| WSL2 内 Linux 工具链(Phase 2+,System V 实验) | `wsl -d Ubuntu-22.04` → `sudo apt install build-essential gdb` | 发行版已装,内部工具未装 |
| Node 升级(可选) | `winget install OpenJS.NodeJS.LTS` | 仅当选择 Vite 7 时需要 |

安装动作均需用户执行或明确授权,平台代码只做**探测**(启动时检查工具是否存在并给出指引,缺失时对应功能降级为不可用,并明确提示)。

## 6. 离线能力

- 后端依赖全部经 cargo 离线可还原(`cargo vendor` 可选);前端依赖 npm 锁定。
- 课程/实验/习题为本地数据文件;进度存本地 SQLite。
- 模拟器模式不依赖网络与真实系统调用。**唯一需要联网的是 AI 助教**(OpenAI-compatible API,用户自配);未配置时其余功能完整可用(README 第九节要求)。

## 7. 待确认项

1. [ ] Vite 6(不动 Node)vs 升级 Node 到 22 LTS + Vite 7。
2. [ ] Unicorn GPL-2.0 是否可接受(本地使用场景默认可接受;若有分发计划请提出)。
3. [ ] UI 组件库 Naive UI 的最终确认(可先用原型页目视对比 Element Plus 后定稿)。
