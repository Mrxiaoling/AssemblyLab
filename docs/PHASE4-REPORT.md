# Phase 4 阶段报告 — C++20 协程实验室

日期:2026-11-09 · 状态:待用户验收(与 Phase 5 一并交付)

## 1. 完成内容

| 任务 | 交付 |
|---|---|
| T4.0 协程编译环境 | WSL 安装 **clang 14.0.0**;探针实证 `__coro_frame/__promise/__coro_index` DWARF 变量在 -O0 -g 下完整可见(断在协程内可取帧地址、字段、状态编号) |
| T4.1 帧布局读取 | gdb console 捕获通道(`-interpreter-exec console` + MI 流输出 C 转义还原);`coro_frame()` 解析单行 pretty print(顶层逗号分割;MI varobj 对该类型 numchild=0 不可用);帧地址 + 全字段值 |
| T4.2 挂起/恢复时间线 | `coro_timeline()`:扫描源码 co_await/co_return 行设断点 → 连续 run 收集每站(行号/原因/`__coro_index`/**该时刻协程帧快照**)→ 收集后删临时断点;`GET /api/debug/sessions/{id}/coro` |
| T4.3 前端协程页签 | DebugView「协程」页签:⏺ 录制 → 状态机示意(SVG,由真实轨迹推导,**标注「实现示意 · clang 14.0.0」**)+ 时间线站点条(点击/播放/步进)+ 每站帧字段表(帧地址高亮,DWARF 位置无效时如实标注) |
| T4.4 编译器选择 | DebugView 编译器下拉(g++/clang++);课程 `experiment.compiler` → `?cmp=` → 会话编译器直通;compile 侧编译器**白名单**(防注入) |
| T4.5 多编译器对比 | compare API 白名单加 clang++(-std=c++20 对所有编译器统一启用,协程必需) |
| T4.6 阶段七课程 3 门 | stage07-lesson01(协程帧与状态机)/ lesson02(co_await 展开·自定义 awaiter 打印三部曲)/ lesson03(挂起与恢复全景·帧生与死)——**README 第七节 10 个观察点全部有对应实验步骤**;正文强制「标准要求 vs 编译器实现」分栏表述 |

## 2. 关键决策与发现

| # | 决策/发现 | 记录 |
|---|---|---|
| D-4-1 | 协程帧观察选 **clang 而非 g++ 11**:GCC 14+ 才有 -fdump-coroutine;clang 14 在 DWARF 直接暴露帧结构 | COURSE_DESIGN §7 原则 1 的落地;页面全程标注 clang 14.0.0 |
| D-4-2 | 帧解析走 **console pretty print** 而非 varobj:`-var-create` 对 __coro_frame_ty 返回 numchild=0(gdb 12 对 clang 帧类型不展开子对象),但 console `p` 输出全部字段 | TCP 日志实证;varobj 路线已废弃 |
| D-4-3 | **HALO 现场实证**:本课源码的协程帧地址在**栈上**(0x7fff…),clang 把未逃逸协程的堆分配省略了——帧分配位置"是实现选择"有了活教材 | lesson01 观察点 2 |
| D-4-4 | 时间线每一站顺手抓帧(而非结束再抓):协程跑完即无帧可看;首站(初次挂起)DWARF 位置无效(<optimized out>)如实标注 | 顺序修正的教训 |
| D-4-5 | `std::terminate` 需显式 `#include <exception>`(<cstdlib> 不保证);编译命令补 `-std=c++20` | 错误行号映射帮助快速定位 |

## 3. 测试结果

- curl:clang++ 会话创建 → `/coro` 返回 3 站时间线(2 个挂起/恢复站 + 退出),stop1 帧字段完整(`__resume_fn/__destroy_fn/__promise/__coro_index=2/seed/local…`)
- 浏览器:clang++ 启动 → 协程页签录制 → 站点 #0/#1/#2 渲染、状态机 SVG 带编译器标注、站点 #1 帧字段表 9 项真实值
- content_tests:课程 schema + 习题结构含 stage07 三门(协程课 mode=debug 不在单测跑,由本节 curl/浏览器验收)

## 4. README 第七节 10 个观察点 → 实验映射

| # | 观察点 | 落点 |
|---|---|---|
| 1 | 协程调用时发生了什么 | lesson01 反汇编(帧分配+参数拷贝+指针装填) |
| 2 | 帧在哪分配 | lesson01 帧地址(HALO 栈上)+ hands-on 改成逃逸→堆 |
| 3 | 帧里保存了哪些数据 | lesson01/03 帧字段表(resume/destroy/promise/index/局部变量) |
| 4 | promise_type 参与 | lesson01 __promise 嵌帧 + get_return_object 时序 |
| 5 | co_await 转化 | lesson02 自定义 awaiter 打印三部曲 + 反汇编三特征 |
| 6 | 挂起保存了什么 | lesson02/03 帧=index+awaiter槽+跨挂起点变量 |
| 7 | 恢复如何找到下一步 | lesson03 __coro_index 跳转表(反汇编) |
| 8 | 局部变量穿越挂起点 | lesson03 warm/big 提升进帧,[帧+偏移] 访问 |
| 9 | resume() 最终做了什么 | lesson03 = call [__resume_fn](用户态纯调用) |
| 10 | 结束后如何释放 | lesson03 ~Task → destroy → 析构帧内对象;内存视图看帧消失 |

## 5. 待确认

1. 协程验收动线:「学习中心」→「7·协程帧」→ 打开实验(clang++ 自动选中)→ 协程页签「录制」→ 播放时间线,点站点看帧字段。
2. Phase 5 范围已一并完成(见 PHASE5-REPORT.md);Phase 6(OS/并发)待确认后启动。
