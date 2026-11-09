# 设计文档索引(Phase 0)

| 文档 | 内容 | 一句话结论 |
|---|---|---|
| [PRD.md](PRD.md) | 产品需求 | MVP=模拟器模式+3 课+2 题型;验收原则「每个原理至少一个可运行实验」 |
| [ARCHITECTURE.md](ARCHITECTURE.md) | 系统架构 | Rust 后端(actor 会话)+ Vue3 工作台;五类适配器;OpenAPI 类型同步 |
| [TECH_STACK.md](TECH_STACK.md) | 技术选型 | 全栈清单+许可证矩阵+本机现状;唯一新增工具 NASM |
| [EXPERIMENT_ENGINE.md](EXPERIMENT_ENGINE.md) | 实验引擎 | 双模式(Unicorn 模拟器 / GDB 调试);diff 分层「解释可错,执行不可错」 |
| [ISA_DESIGN.md](ISA_DESIGN.md) | ISA 适配层 | ISA=数据包+适配器;x86-64 全量定义,ARM64/RISC-V 扩展路径 |
| [COURSE_DESIGN.md](COURSE_DESIGN.md) | 课程体系 | 15 步教学模板 schema;十阶段实验映射;协程专题基于真实编译产物 |
| [EXERCISE_DESIGN.md](EXERCISE_DESIGN.md) | 习题判题 | 8 题型、判题三级分型、提示梯度;通过状态只认确定性判题 |
| [SECURITY_DESIGN.md](SECURITY_DESIGN.md) | 执行安全 | 模式A结构性隔离;模式B进程级限制并如实标注;不可用模式显式禁用 |
| [TEST_PLAN.md](TEST_PLAN.md) | 测试体系 | 引擎 golden 测试、内容一致性校验、性能基线 |
| [ROADMAP.md](ROADMAP.md) | 路线图 | Phase 1 任务级拆分(T1.0–T1.13)+ 各阶段验收标准与决策门 |

技术验证记录:[../spikes/unicorn_step_diff/RESULT.md](../spikes/unicorn_step_diff/RESULT.md) —— Unicorn 2 预编译 DLL 在 Windows 上的单步状态捕获 **[PASS]**。
