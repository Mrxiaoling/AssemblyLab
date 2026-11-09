<script setup lang="ts">
// 优化级别对比工作台(P3):同一 C++ 源码 × O0..O3(× SysV/Win64 目标编译器)汇编并排对比
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import { useRoute } from 'vue-router'
import { NButton, NSelect, NTag, NAlert, NSpin } from 'naive-ui'
import * as monaco from 'monaco-editor'
import { api } from '../api/client'
import type { CompareEntry } from '../api/types'

const DEFAULT_SOURCE = `// 同一份代码,四个优化级别能差多少?
// 试着:1) 先点「编译对比」 2) 把 n 改成 100 再对比 3) 打开 -O2 观察循环去哪了
int sum_to(int n) {
    int s = 0;
    for (int i = 1; i <= n; i++) {
        s += i;
    }
    return s;
}

int main() {
    int r = sum_to(10);
    return r;
}
`

const source = ref(DEFAULT_SOURCE)
const opts = ref<string[]>(['O0', 'O1', 'O2', 'O3'])
const compiler = ref('g++')
const entries = ref<CompareEntry[]>([])
const busy = ref(false)
const error = ref('')
const funcFilter = ref<string | null>(null)

const compilerOptions = computed(() => {
  const list = [
    { label: 'g++ · x86-64 SysV (WSL)', value: 'g++' },
    { label: 'clang++ · x86-64 SysV (协程/对比)', value: 'clang++' },
    { label: 'aarch64-linux-gnu-g++ · ARM64', value: 'aarch64-linux-gnu-g++' },
  ]
  if (health.value.mingw)
    list.push({ label: 'x86_64-w64-mingw32-g++ · Win64 ABI', value: 'x86_64-w64-mingw32-g++' })
  return list
})
const health = ref<{ mingw: string | null; wsl_gpp: string | null }>({ mingw: null, wsl_gpp: null })

const route = useRoute()

let editor: monaco.editor.IStandaloneCodeEditor | null = null
const host = ref<HTMLDivElement>()
onMounted(async () => {
  editor = monaco.editor.create(host.value!, {
    value: source.value,
    language: 'cpp',
    theme: 'vs-dark',
    fontSize: 13,
    minimap: { enabled: false },
    automaticLayout: true,
  })
  editor.onDidChangeModelContent(() => {
    if (!busy.value) source.value = editor!.getValue()
  })
  const exp = route.query.exp as string | undefined
  if (exp) {
    source.value = exp
    editor.setValue(exp)
  }
  try {
    const h = await api.health()
    health.value = h.tools
    if (!h.tools.wsl_gpp) error.value = '未检测到 WSL g++(真机对比不可用)'
  } catch { /* 忽略 health 失败 */ }
})
onBeforeUnmount(() => editor?.dispose())

async function runCompare() {
  busy.value = true
  error.value = ''
  entries.value = []
  funcFilter.value = null
  try {
    if (!opts.value.length) throw new Error('至少选择一个优化级别')
    source.value = editor!.getValue()
    entries.value = await api.compareAsm(source.value, [...opts.value], compiler.value)
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e)
  } finally {
    busy.value = false
  }
}

// 函数清单(任意列中出现过的;供过滤)
const functions = computed(() => {
  const set = new Set<string>()
  for (const e of entries.value)
    for (const l of e.asm_lines) if (l.function) set.add(l.function)
  return [...set].sort().map((f) => ({ label: f, value: f }))
})

function visibleLines(e: CompareEntry) {
  if (!funcFilter.value) return e.asm_lines
  return e.asm_lines.filter((l) => l.function === funcFilter.value)
}

function insCount(e: CompareEntry) {
  if (!funcFilter.value) return e.instruction_count
  return e.asm_lines.filter((l) => l.function === funcFilter.value && l.addr).length
}

const optColors: Record<string, string> = { O0: '#e69055', O1: '#63e2b7', O2: '#82aaff', O3: '#e05f9b' }
</script>

<script lang="ts">
export default { name: 'CompareView' }
</script>

<template>
  <div class="cmp-root">
    <div class="ctrl">
      <n-button type="primary" size="small" :disabled="busy" @click="runCompare">
        {{ busy ? '编译中…' : '编译对比' }}
      </n-button>
      <span class="opt-group">
        <label v-for="o in ['O0', 'O1', 'O2', 'O3']" :key="o" class="opt-item">
          <input v-model="opts" type="checkbox" :value="o" /> <span :style="{ color: optColors[o] }">{{ o }}</span>
        </label>
      </span>
      <n-select v-model:value="compiler" size="small" :options="compilerOptions" style="width: 280px" />
      <n-select
        v-if="functions.length"
        v-model:value="funcFilter"
        size="small"
        :options="[{ label: '全部函数', value: null as any }, ...functions]"
        style="width: 240px"
        placeholder="过滤函数"
        clearable
      />
      <div style="flex: 1" />
      <n-tag size="small" type="warning" :bordered="false">编译在 WSL 内执行(进程级限制)</n-tag>
    </div>
    <n-alert v-if="error" type="error" size="small" closable @close="error = ''">{{ error }}</n-alert>

    <div class="cmp-main">
      <div class="editor-col">
        <div class="panel">
          <div class="panel-title">C++ 源码</div>
          <div ref="host" class="panel-body editor-host" />
        </div>
      </div>

      <n-spin :show="busy" class="cols-wrap">
        <div v-if="entries.length" class="cols">
          <div v-for="e in entries" :key="e.opt + e.compiler" class="col panel">
            <div class="panel-title">
              <span :style="{ color: optColors[e.opt] ?? '#fff', fontWeight: 600 }">{{ e.opt }}</span>
              <span style="color: #666; font-size: 11px">{{ e.compiler === 'g++' ? 'SysV' : 'Win64' }}</span>
              <n-tag size="tiny" :bordered="false">{{ insCount(e) }} 指令</n-tag>
            </div>
            <div class="panel-body">
              <table class="asm">
                <tbody>
                  <tr v-for="(l, i) in visibleLines(e)" :key="i" :class="{ src: l.source_line, lab: !l.addr }">
                    <td class="a">{{ l.addr ? '0x' + l.addr.slice(2) : '' }}</td>
                    <td class="t">{{ l.text }}</td>
                  </tr>
                </tbody>
              </table>
            </div>
          </div>
        </div>
        <div v-else-if="!busy" class="placeholder">
          <div class="ph-title">同一 C++ 源码,多优化级别并排对比</div>
          <div class="ph-item">① 左侧写代码(或用预置的 sum_to)→ ② 勾选 O0..O3 → ③ 点「编译对比」</div>
          <div class="ph-item">观察点:循环去哪了?函数还在吗?哪一级把 sum_to 直接算成数学公式?</div>
          <div class="ph-item">切换编译器可对比 SysV 与 Win64 参数寄存器(RDI/RSI vs RCX/RDX)</div>
        </div>
      </n-spin>
    </div>
  </div>
</template>

<style scoped>
.cmp-root { height: calc(100vh - 41px); display: flex; flex-direction: column; overflow: hidden; }
.ctrl { display: flex; align-items: center; gap: 10px; padding: 6px 10px; background: #202020; border-bottom: 1px solid #2a2a2a; flex: none; flex-wrap: wrap; }
.opt-group { display: flex; gap: 10px; align-items: center; }
.opt-item { display: flex; gap: 3px; align-items: center; font-size: 12.5px; cursor: pointer; }
.cmp-main { flex: 1; display: flex; gap: 4px; padding: 4px; min-height: 0; }
.editor-col { flex: 0 0 34%; min-width: 0; display: flex; }
.editor-host { min-height: 200px; }
.cols-wrap { flex: 1; min-width: 0; display: flex; }
.cols { flex: 1; display: flex; gap: 4px; min-width: 0; }
.col { flex: 1; min-width: 0; }
.panel { border: 1px solid #2a2a2a; border-radius: 4px; display: flex; flex-direction: column; overflow: hidden; background: #181818; }
.panel-title { font-size: 12px; color: #999; padding: 4px 10px; background: #202020; border-bottom: 1px solid #2a2a2a; flex: none; display: flex; align-items: center; gap: 8px; }
.panel-body { flex: 1; overflow: auto; min-height: 0; }
.asm { width: 100%; font-size: 12px; border-collapse: collapse; }
.asm td.a { color: #5f5f5f; width: 90px; white-space: nowrap; }
.asm td.t { color: #c7c7c7; white-space: pre; }
.asm tr.src td.t { color: #e8e8e8; }
.asm tr.lab td.t { color: #82aaff; }
.placeholder { margin: auto; text-align: center; color: #777; max-width: 640px; }
.ph-title { font-size: 18px; color: #999; margin-bottom: 14px; }
.ph-item { font-size: 13px; margin: 6px 0; }
</style>
