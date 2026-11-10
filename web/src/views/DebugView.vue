<script setup lang="ts">
// 模式 B:C++ 真机调试视图(WSL g++ + gdb/MI)
// P3 重构:splitpanes 布局 + 类型化内存面板 + 观察点(watch)
// 隔离等级:进程级限制(SECURITY_DESIGN §3),非强沙箱——页面明确标注
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import { useRoute } from 'vue-router'
import { NButton, NButtonGroup, NSelect, NTag, NAlert, NInput, NTabPane, NTabs } from 'naive-ui'
import * as monaco from 'monaco-editor'
import { asmLanguageId } from '../monarch/asm' // cpp 由 monaco 内置;此处仅触发注册环境
import { api } from '../api/client'
import { Splitpanes, Pane } from 'splitpanes'
import 'splitpanes/dist/splitpanes.css'
import { decodeValues, chunkCells, MEM_TYPES, typeSize, type MemType } from '../utils/memdecode'

const cppSource = `int add(int a, int b) {
    return a + b;
}

int main() {
    int r = add(10, 20);
    return r;
}
`

const optLevel = ref('O0')
const optOptions = ['O0', 'O1', 'O2', 'O3'].map((v) => ({ label: v, value: v }))
const compiler = ref('g++')
const compilerOptions = [
  { label: 'g++ (SysV)', value: 'g++' },
  { label: 'clang++ (协程帧最完整)', value: 'clang++' },
]
const targetIsa = ref('x86-64')
const isaOptions = [
  { label: 'x86-64 (本地 WSL)', value: 'x86-64' },
  { label: 'ARM64 (远程真机)', value: 'arm64' },
]
const sessionId = ref<number | null>(null)
const busy = ref(false)
const error = ref('')
const compileInfo = ref('')
const asmLines = ref<any[]>([])
const registers = ref<[string, string][]>([])
const frames = ref<any[]>([])
const variables = ref<[string, string][]>([])
const lastReport = ref<any | null>(null)
const stopReason = ref<string | null>(null)
const currentLine = ref<number | null>(null)
const breakpoints = ref<Set<number>>(new Set())

// 观察点
const watchList = ref<string[]>([])
const watchInput = ref('')

// 协程(P4):时间线 + 帧布局 + 状态机
const coroStops = ref<any[]>([])
const coroIdx = ref(0) // 时间线当前播放位置
const coroErr = ref('')
const coroBusy = ref(false)
const coroPlaying = ref(false)
let coroTimer: ReturnType<typeof setTimeout> | null = null

async function recordCoro() {
  if (!sessionId.value || coroBusy.value) return
  coroBusy.value = true
  coroErr.value = ''
  try {
    const resp = await api.debugCoro(sessionId.value)
    if (resp.timeline_error) {
      coroErr.value = resp.timeline_error
      coroStops.value = []
    } else {
      coroStops.value = resp.timeline
      coroIdx.value = 0
      // 时间线把程序跑完了,同步状态
      stopReason.value = 'exited-normally(时间线录制后)'
      currentLine.value = null
      paintCurrent(null)
    }
  } catch (e) {
    coroErr.value = String((e as Error).message)
  } finally {
    coroBusy.value = false
  }
}

function coroPlay() {
  if (coroPlaying.value) {
    coroPlaying.value = false
    if (coroTimer) clearTimeout(coroTimer)
    return
  }
  coroPlaying.value = true
  const stepPlay = () => {
    if (!coroPlaying.value) return
    if (coroIdx.value >= coroStops.value.length - 1) {
      coroPlaying.value = false
      return
    }
    coroIdx.value++
    coroTimer = setTimeout(stepPlay, 900)
  }
  coroTimer = setTimeout(stepPlay, 400)
}

// 状态机节点:Start → 去重连续的挂起状态 → Final(由真实轨迹推导)
const coroStates = computed(() => {
  const seq: { label: string; idx: number | null; stop: number }[] = [
    { label: '调用 foo()', idx: null, stop: -1 },
  ]
  let last: number | null | undefined = undefined
  coroStops.value.forEach((s, i) => {
    if (s.coro_index === null || s.coro_index === undefined) return
    if (s.coro_index !== last) {
      seq.push({ label: `状态 ${s.coro_index}`, idx: s.coro_index, stop: i })
      last = s.coro_index
    }
  })
  seq.push({ label: '帧释放', idx: null, stop: coroStops.value.length - 1 })
  return seq
})

// 内存面板
const memAddr = ref('')
const memView = ref<{ addr: string; hex: string; ascii: string } | null>(null)
const memType = ref<MemType>('i32')
const memEndian = ref<'le' | 'be'>('le')
const memRadix = ref<'hex' | 'dec' | 'bin'>('dec')

const host = ref<HTMLDivElement>()
let editor: monaco.editor.IStandaloneCodeEditor | null = null
let lineDecor: string[] = []
let bpDecor: string[] = [] // 上一次装饰的 id

const route = useRoute()

onMounted(() => {
  editor = monaco.editor.create(host.value!, {
    value: cppSource,
    language: 'cpp',
    theme: 'vs-dark',
    fontSize: 13,
    minimap: { enabled: false },
    automaticLayout: true,
  })
  editor.onDidChangeModelContent(() => {
    if (!busy.value) cppSource2 = editor!.getValue()
  })
  editor.onMouseDown((e) => {
    // 点击行号 = 断点开关(仅已启动会话)
    const t = e.target
    if (t.type !== monaco.editor.MouseTargetType.GUTTER_LINE_NUMBERS) return
    const line = t.position?.lineNumber
    if (line) void toggleBp(line)
  })
  const exp = route.query.exp as string | undefined
  if (exp) {
    editor.setValue(exp)
    cppSource2 = exp
  }
  const cmp = route.query.cmp as string | undefined
  if (cmp && ['g++', 'clang++'].includes(cmp)) compiler.value = cmp
})
let cppSource2 = cppSource

function revealSource(line: number | null | undefined) {
  if (!editor || !line) return
  editor.revealLineInCenter(line)
  editor.setPosition({ lineNumber: line, column: 1 })
  editor.focus()
}

onBeforeUnmount(() => editor?.dispose())

async function startDebug() {
  busy.value = true
  error.value = ''
  try {
    const resp = await api.debugCreate(cppSource2, optLevel.value, compiler.value, targetIsa.value)
    sessionId.value = resp.id
    asmLines.value = resp.asm_lines
    compileInfo.value = `${resp.compiler} · ${resp.opt}`
    stopReason.value = null
    currentLine.value = null
    watchList.value = []
    await refresh()
  } catch (e) {
    error.value = String((e as Error).message)
  } finally {
    busy.value = false
  }
}

async function toggleBp(line: number) {
  if (!sessionId.value) return
  const next = new Set(breakpoints.value)
  if (next.has(line)) next.delete(line)
  else next.add(line)
  try {
    await api.debugBreakpoints(sessionId.value, [...next])
    breakpoints.value = next
    paintBps()
  } catch (e) {
    error.value = String((e as Error).message)
  }
}

async function addWatch() {
  if (!sessionId.value || busy.value) return
  const expr = watchInput.value.trim()
  if (!expr) return
  const next = [...new Set([...watchList.value, expr])]
  busy.value = true
  try {
    const ok = await api.debugWatchpoints(sessionId.value, next)
    watchList.value = ok
    watchInput.value = ''
    // 设置观察点会自动 run 到 main,刷新状态
    await refresh()
  } catch (e) {
    error.value = String((e as Error).message)
  } finally {
    busy.value = false
  }
}

async function removeWatch(expr: string) {
  if (!sessionId.value) return
  const next = watchList.value.filter((x) => x !== expr)
  try {
    const ok = await api.debugWatchpoints(sessionId.value, next)
    watchList.value = ok
  } catch (e) {
    error.value = String((e as Error).message)
  }
}

function paintBps() {
  if (!editor) return
  const model = editor.getModel()!
  bpDecor = model.deltaDecorations(bpDecor, [
    ...breakpoints.value].map((line) => ({
    range: new monaco.Range(line, 1, line, 1),
    options: { isWholeLine: true, linesDecorationsClassName: 'dbg-bp' },
  })))
}

function paintCurrent(line: number | null) {
  if (!editor) return
  const model = editor.getModel()!
  model.deltaDecorations(lineDecor, [])
  if (line) {
    lineDecor = model.deltaDecorations([], [
      { range: new monaco.Range(line, 1, line, 1), options: { isWholeLine: true, className: 'asm-current-line' } },
    ])
    editor.revealLineInCenter(line)
  }
}

async function loadMem() {
  if (!sessionId.value || !memAddr.value.trim()) return
  try {
    memView.value = await api.debugMemory(sessionId.value, memAddr.value.trim(), 128)
  } catch (e) {
    memView.value = null
    error.value = String((e as Error).message)
  }
}

async function goMemReg(name: 'RSP' | 'RBP') {
  const v = registers.value.find(([n]) => n === name)?.[1]
  if (v) {
    memAddr.value = v
    await loadMem()
  }
}

const memRows = computed(() => {
  if (!memView.value) return []
  const perRow = typeSize(memType.value) >= 8 ? 4 : 8
  return chunkCells(decodeValues(memView.value.hex, memType.value, memEndian.value === 'le', memRadix.value), perRow)
})

async function refresh() {
  if (!sessionId.value) return
  const fr = await api.debugFrames(sessionId.value)
  frames.value = fr.frames
  variables.value = fr.variables
}

async function op(kind: 'run' | 'step' | 'next' | 'finish') {
  if (!sessionId.value || busy.value) return
  busy.value = true
  error.value = ''
  try {
    let resp
    if (kind === 'run') resp = await api.debugRun(sessionId.value)
    else if (kind === 'finish') resp = await api.debugFinish(sessionId.value)
    else resp = await api.debugStep(sessionId.value, kind === 'step', 1)
    lastReport.value = resp.reports[resp.reports.length - 1] ?? null
    stopReason.value = resp.state.stop_reason
    const r = lastReport.value
    currentLine.value = r?.sourceLine ?? null
    paintCurrent(currentLine.value)
    if (resp.state.terminated) {
      currentLine.value = null
      paintCurrent(null)
    }
    await refresh()
    // 寄存器:从报告 after 取
    if (r) registers.value = r.after as [string, string][]
    // 内存面板跟随(RSP 模式)
    if (memView.value && memAddr.value) await loadMem()
  } catch (e) {
    error.value = String((e as Error).message)
  } finally {
    busy.value = false
  }
}

const regGroups = computed(() => {
  const r = registers.value
  return [
    { title: '通用', names: ['RAX', 'RBX', 'RCX', 'RDX'] },
    { title: '变址', names: ['RSI', 'RDI', 'RBP', 'RSP'] },
    { title: '状态', names: ['RIP', 'RFLAGS'] },
  ].map((g) => ({ ...g, values: g.names.map((n) => [n, r.find(([k]) => k === n)?.[1] ?? '—']) }))
})

const isWatchHit = computed(() => (stopReason.value ?? '').includes('watchpoint'))
const typeOptions = MEM_TYPES.map((t) => ({ label: t.label, value: t.value }))
const coroCompilerLabel = computed(() => (compileInfo.value ? compileInfo.value : '编译器信息见顶部标注'))
</script>

<script lang="ts">
// 标注隔离等级(P2):进程级限制
export default { name: 'DebugView' }
</script>

<template>  <div class="dbg-root">
    <div class="ctrl">
      <n-button-group size="small">
        <n-button type="primary" :disabled="busy" @click="startDebug">
          {{ sessionId ? '重新编译并启动' : '编译并启动调试' }}
        </n-button>
        <n-select v-model:value="optLevel" size="small" :options="optOptions" style="width: 90px" />
        <n-select v-model:value="compiler" size="small" :options="compilerOptions" style="width: 190px" />
        <n-select v-model:value="targetIsa" size="small" :options="isaOptions" style="width: 165px" />
      </n-button-group>
      <n-button-group size="small">
        <n-button :disabled="!sessionId || busy" @click="op('run')">▶ 继续</n-button>
        <n-button :disabled="!sessionId || busy" @click="op('step')">步入</n-button>
        <n-button :disabled="!sessionId || busy" @click="op('next')">步过</n-button>
        <n-button :disabled="!sessionId || busy" @click="op('finish')">步出</n-button>
      </n-button-group>
      <n-tag v-if="stopReason" size="small" :type="isWatchHit ? 'warning' : 'info'">
        {{ isWatchHit ? '👁 ' + stopReason : stopReason }}
      </n-tag>
      <n-tag v-if="compileInfo" size="small" :bordered="false">{{ compileInfo }}</n-tag>
      <div style="flex: 1" />
      <n-tag size="small" type="warning">隔离等级:进程级限制(非强沙箱)</n-tag>
    </div>
    <n-alert v-if="error" type="error" size="small" closable @close="error = ''">{{ error }}</n-alert>
    <n-alert v-if="targetIsa === 'arm64'" type="warning" size="small" style="margin-top:6px">
      ARM64 远程真机默认关闭:后端需设置 ASSEMBLYLAB_ENABLE_UNSAFE_DEBUG=1、ASSEMBLYLAB_ENABLE_ARM64_REMOTE=1,
      并显式配置 ASSEMBLYLAB_ARM64_HOST / ASSEMBLYLAB_ARM64_USER(免密 ssh);固定使用 aarch64-linux-gnu-g++ 交叉编译,
      gdbserver 只绑远端回环并经 ssh 隧道连接。
    </n-alert>
    <n-alert v-else type="warning" size="small" style="margin-top:6px">
      模式 B 默认关闭:需以 ASSEMBLYLAB_ENABLE_UNSAFE_DEBUG=1 启动后端。隔离等级为进程级限制(非强沙箱),请勿运行不可信代码。
    </n-alert>

    <div class="dbg-main">
      <splitpanes class="default-theme">
        <!-- 源码 -->
        <pane :size="34">
          <div class="panel" style="height: 100%">
            <div class="panel-title">C++ 源码 <span style="color:#555">(点击行号设断点)</span></div>
            <div ref="host" class="panel-body" />
          </div>
        </pane>

        <!-- 反汇编(编译产物,含源码行标注) -->
        <pane :size="33">
          <div class="panel" style="height: 100%">
            <div class="panel-title">编译产物反汇编(g++ -g) <span style="color:#555">(点击带 → 的行定位源码)</span></div>
            <div class="panel-body">
              <table class="disasm">
                <tbody>
                  <tr v-for="(l, i) in asmLines" :key="i" :class="{ 'src-mark': l.source_line }"
                      @click="revealSource(l.source_line)">
                    <td class="addr-col">{{ l.addr ? '0x' + l.addr.slice(2) : '' }}</td>
                    <td class="line-col">{{ l.source_line ? '→ ' + l.source_line : '' }}</td>
                    <td class="text-col">{{ l.text }}</td>
                  </tr>
                </tbody>
              </table>
            </div>
          </div>
        </pane>

        <!-- 右列:寄存器 + 标签页(栈帧/内存/观察点) -->
        <pane :size="33">
          <splitpanes class="default-theme" :horizontal="true">
            <pane :size="40">
              <div class="panel" style="height: 100%">
                <div class="panel-title">寄存器(真实 CPU)</div>
                <div class="panel-body">
                  <div v-for="g in regGroups" :key="g.title" class="rgroup">
                    <div class="rgroup-title">{{ g.title }}</div>
                    <div v-for="[n, v] in g.values" :key="n" class="reg">
                      <span class="rname">{{ n }}</span><span class="rval">{{ v }}</span>
                    </div>
                  </div>
                </div>
              </div>
            </pane>
            <pane :size="60">
              <div class="panel" style="height: 100%">
                <div class="panel-body" style="display: flex; flex-direction: column">
                  <n-tabs type="line" size="small" style="flex: 1; display: flex; flex-direction: column" pane-style="flex:1; overflow:auto; min-height:0">
                    <n-tab-pane name="frames" tab="调用栈与变量">
                      <div v-for="f in frames" :key="f.level + f.func" class="frame">
                        <n-tag size="tiny" :bordered="false">#{{ f.level }}</n-tag>
                        {{ f.func }}
                        <span v-if="f.line" style="color: #666">({{ (f.file || '').split('/').pop() }}:{{ f.line }})</span>
                      </div>
                      <div class="vars">
                        <div v-for="[n, v] in variables" :key="n" class="reg">
                          <span class="rname" style="color: #7ec699">{{ n }}</span>
                          <span class="rval">{{ v }}</span>
                        </div>
                      </div>
                    </n-tab-pane>
                    <n-tab-pane name="mem" tab="内存">
                      <div class="mem-tools">
                        <n-input v-model:value="memAddr" size="tiny" placeholder="地址或 0x…" @keyup.enter="loadMem" />
                        <n-button size="tiny" @click="loadMem">查看</n-button>
                        <n-button size="tiny" @click="goMemReg('RSP')">RSP</n-button>
                        <n-button size="tiny" @click="goMemReg('RBP')">RBP</n-button>
                      </div>
                      <div class="mem-tools">
                        <n-select v-model:value="memType" size="tiny" :options="typeOptions" style="width: 100px" />
                        <n-select v-model:value="memEndian" size="tiny" :options="[{label:'小端',value:'le'},{label:'大端',value:'be'}]" style="width: 78px" />
                        <n-select v-model:value="memRadix" size="tiny" :options="[{label:'十',value:'dec'},{label:'十六',value:'hex'},{label:'二',value:'bin'}]" style="width: 72px" />
                      </div>
                      <table v-if="memRows.length" class="mem">
                        <tbody>
                          <tr v-for="(row, ri) in memRows" :key="ri">
                            <td class="m-off">+{{ row[0].off.toString(16).toUpperCase().padStart(4, '0') }}</td>
                            <td v-for="c in row" :key="c.off" class="m-val">{{ c.text }}</td>
                          </tr>
                        </tbody>
                      </table>
                      <div v-else style="color: #555; padding: 8px">输入地址(如局部变量 &r 的值)查看真实进程内存</div>
                    </n-tab-pane>
                    <n-tab-pane name="watch" tab="观察点">
                      <div class="mem-tools">
                        <n-input v-model:value="watchInput" size="tiny" placeholder="表达式,如 r、*0x7ffd…、&x" @keyup.enter="addWatch" />
                        <n-button size="tiny" type="primary" :disabled="!sessionId || busy" @click="addWatch">👁 添加</n-button>
                      </div>
                      <div style="color: #666; font-size: 12px; padding: 4px 8px">
                        观察点 = 数据断点:表达式被写入时停下并报告 旧值→新值(需已启动调试)
                      </div>
                      <div v-for="w in watchList" :key="w" class="frame" style="display: flex; gap: 8px; align-items: center">
                        <n-tag size="small" type="info" :bordered="false">👁 {{ w }}</n-tag>
                        <n-button size="tiny" quaternary type="error" @click="removeWatch(w)">移除</n-button>
                      </div>
                    </n-tab-pane>
                    <n-tab-pane name="coro" tab="协程">
                      <div class="mem-tools">
                        <n-button size="tiny" type="primary" :disabled="!sessionId || coroBusy" @click="recordCoro">
                          {{ coroBusy ? '录制中…' : '⏺ 录制挂起/恢复时间线' }}
                        </n-button>
                        <n-button v-if="coroStops.length > 1" size="tiny" @click="coroPlay">
                          {{ coroPlaying ? '⏸ 暂停' : '▶ 播放' }}
                        </n-button>
                        <n-button v-if="coroStops.length > 1" size="tiny" @click="coroIdx = Math.max(0, coroIdx - 1)">‹ 上一站</n-button>
                        <n-button v-if="coroStops.length > 1" size="tiny" @click="coroIdx = Math.min(coroStops.length - 1, coroIdx + 1)">下一站 ›</n-button>
                      </div>
                      <n-alert v-if="coroErr" type="error" size="small">{{ coroErr }}</n-alert>
                      <template v-if="coroStops.length">
                        <div style="color:#666; font-size:12px; padding: 4px 8px">
                          状态机(实现示意,由真实运行轨迹推导 · {{ coroCompilerLabel }}):
                          <svg
                            :viewBox="`0 0 ${64 + coroStates.length * 96} 74`"
                            preserveAspectRatio="xMinYMid meet"
                            style="width: 100%; height: 74px"
                          >
                            <template v-for="(st, i) in coroStates" :key="i">
                              <circle
                                :cx="32 + i * 96" cy="30" r="17"
                                :fill="i - 1 === coroIdx || (i === 0 && coroIdx === -1) ? '#63e2b7' : i === coroStates.length - 1 ? '#2a4a3a' : '#22343c'"
                                :stroke="i - 1 === coroIdx ? '#63e2b7' : '#3a5a52'" stroke-width="1.5"
                              />
                              <text :x="32 + i * 96" y="34" text-anchor="middle" font-size="10" fill="#dfe8e2">{{ st.label }}</text>
                              <line v-if="i > 0" :x1="32 + (i - 1) * 96 + 17" y1="30" :x2="32 + i * 96 - 17" y2="30" stroke="#4a6a60" stroke-width="1.2" />
                              <text v-if="i > 0 && coroStops[st.stop]" :x="32 + (i - 1) * 96 + 46" y="18" text-anchor="middle" font-size="8.5" fill="#6a8a80">
                                resume / 挂起
                              </text>
                            </template>
                          </svg>
                        </div>
                        <div class="coro-stops">
                          <button
                            v-for="(s, i) in coroStops" :key="i"
                            class="coro-stop" :class="{ on: i === coroIdx }"
                            @click="coroIdx = i"
                          >
                            #{{ i }} · 行{{ s.line }}<br />
                            <span style="color:#666">{{ s.reason.startsWith('breakpoint') ? '挂起/恢复' : s.reason }}</span>
                          </button>
                        </div>
                        <div v-if="coroStops[coroIdx]" style="padding: 2px 10px">
                          <div style="color:#999; font-size:12px; margin: 6px 0 2px">
                            该时刻协程帧
                            <span v-if="coroStops[coroIdx].frame_addr" style="color:#63e2b7">@ {{ coroStops[coroIdx].frame_addr }}</span>
                            <span v-else style="color:#666">(此 pc 处 DWARF 位置无效——帧变量尚未初始化/已释放)</span>
                          </div>
                          <table v-if="coroStops[coroIdx].frame_fields.length" class="mem">
                            <tbody>
                              <tr v-for="[n, v] in coroStops[coroIdx].frame_fields" :key="n" class="reg">
                                <td class="rname" style="width: 46%; color: #82aaff; overflow: hidden; text-overflow: ellipsis">{{ n }}</td>
                                <td class="rval">{{ v }}</td>
                              </tr>
                            </tbody>
                          </table>
                        </div>
                      </template>
                      <div v-else-if="!coroErr" style="color: #666; font-size: 12px; padding: 4px 8px">
                        用 clang++ 启动协程调试后点「录制」——平台在全部 co_await/co_return 行断下,收集每站的
                        __coro_frame / __coro_index(全部来自真实 gdb)。
                      </div>
                    </n-tab-pane>
                  </n-tabs>
                </div>
              </div>
            </pane>
          </splitpanes>
        </pane>
      </splitpanes>
    </div>

    <!-- 解释卡(最近一步) -->
    <div class="explain" v-if="lastReport">
      <b>{{ lastReport.disasm }}</b> — {{ lastReport.explanation.summary }}
      <div v-for="(d, i) in lastReport.explanation.detail" :key="i" style="color: #9ad1a8; font-size: 12px">
        · {{ d }}
      </div>
    </div>
  </div>
</template>

<style scoped>
.dbg-root { height: calc(100vh - 41px); display: flex; flex-direction: column; overflow: hidden; }
.ctrl { display: flex; align-items: center; gap: 10px; padding: 6px 10px; background: #202020; border-bottom: 1px solid #2a2a2a; flex: none; flex-wrap: wrap; }
.dbg-main { flex: 1; display: flex; gap: 4px; padding: 4px; min-height: 0; }
.dbg-main > * { min-width: 0; }
.panel { border: 1px solid #2a2a2a; border-radius: 4px; display: flex; flex-direction: column; overflow: hidden; background: #181818; width: 100%; }
.panel-title { font-size: 12px; color: #999; padding: 4px 10px; background: #202020; border-bottom: 1px solid #2a2a2a; flex: none; }
.panel-body { flex: 1; overflow: auto; min-height: 0; }
.disasm { width: 100%; font-size: 12px; border-collapse: collapse; }
.disasm tr.src-mark { color: #cfcfcf; }
.addr-col { color: #666; width: 110px; }
.line-col { color: #63e2b7; width: 44px; }
.text-col { color: #c7c7c7; white-space: pre; }
.rgroup-title { font-size: 11px; color: #666; padding: 2px 10px; }
.reg { display: flex; padding: 1px 10px; font-size: 12.5px; }
.rname { width: 70px; color: #82aaff; }
.rval { color: #e8e8e8; }
.frame { padding: 2px 10px; font-size: 12.5px; }
.vars { border-top: 1px dashed #2a2a2a; margin-top: 4px; padding-top: 4px; }
.explain { flex: none; padding: 8px 14px; border-top: 1px solid #2a2a2a; background: #141414; font-size: 13px; max-height: 140px; overflow: auto; }
.mem-tools { display: flex; gap: 6px; padding: 4px 8px; align-items: center; flex-wrap: wrap; }
.coro-stops { display: flex; gap: 6px; padding: 4px 8px; flex-wrap: wrap; }
.coro-stop { background: #1c2c26; border: 1px solid #2a4a3a; color: #cfcfcf; border-radius: 4px; padding: 4px 8px; font-size: 11.5px; cursor: pointer; }
.coro-stop.on { border-color: #63e2b7; color: #63e2b7; background: #16241f; }
.mem { width: 100%; font-size: 12px; border-collapse: collapse; }
.mem .m-off { color: #666; padding-right: 8px; white-space: nowrap; }
.mem .m-val { color: #cfcfcf; padding: 0 8px 0 0; white-space: nowrap; font-variant-numeric: tabular-nums; }
:global(.dbg-bp) { background: #ff6b6b; width: 8px !important; margin-left: 3px; border-radius: 2px; }
:global(.default-theme .splitpanes__splitter) { background: #2a2a2a; }
:global(.default-theme .splitpanes__pane) { background: transparent; }
</style>
