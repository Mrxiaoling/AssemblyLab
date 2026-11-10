<script setup lang="ts">
// 栈帧可视化:数据全部来自真实寄存器快照 + 后端内存读取,
// 标注只是教学性解释(返回地址/局部变量/调用者帧),不伪造任何内存值。
import { computed, onMounted, ref, watch } from 'vue'
import { NButton, NTag } from 'naive-ui'
import { api } from '../../api/client'
import type { MemoryResp } from '../../api/types'
import { useSessionStore } from '../../stores/session'

const store = useSessionStore()
const view = ref<MemoryResp | null>(null)

const spName = computed(() => (store.isa === 'arm64' ? 'SP' : 'RSP'))
const fpName = computed(() => (store.isa === 'arm64' ? 'X29' : 'RBP'))
const sp = computed(() => store.regMap.get(spName.value) ?? null)
const fp = computed(() => store.regMap.get(fpName.value) ?? null)

async function load() {
  if (!store.sessionId || !sp.value) {
    view.value = null
    return
  }
  try {
    view.value = await api.memory(store.sessionId, sp.value, 128)
  } catch {
    view.value = null
  }
}

watch(() => [store.sessionId, store.state?.step_count, sp.value], load)
onMounted(load)

interface StackRow {
  addr: string
  qword: string
  mark: string
  note: string
}

const rows = computed<StackRow[]>(() => {
  if (!view.value || !sp.value) return []
  const hex = view.value.hex.replace(/\s+/g, '')
  const base = BigInt(view.value.addr)
  const spVal = BigInt(sp.value)
  const fpVal = fp.value ? BigInt(fp.value) : null
  const isArm = store.isa === 'arm64'
  const out: StackRow[] = []
  for (let i = 0; i + 16 <= hex.length; i += 16) {
    const bytes = hex.slice(i, i + 16)
    const le = (bytes.match(/../g) ?? []).reverse().join('')
    const addrVal = base + BigInt(i / 2)
    let mark = ''
    let note = ''
    if (addrVal === spVal) {
      mark = spName.value
      note = '当前栈顶;PUSH/CALL 会先减 SP 再写入'
    } else if (fpVal !== null && addrVal === fpVal) {
      mark = fpName.value
      note = isArm ? '帧指针(X29),保存调用者帧地址' : '帧基址;函数序言 mov rbp,rsp 建立'
    } else if (fpVal !== null && addrVal === fpVal + 8n) {
      mark = '返回地址'
      note = isArm ? '保存的 X30(LR),RET 会跳回这里' : 'CALL 压入的返回地址,RET 会跳回这里'
    } else if (fpVal !== null && addrVal < fpVal) {
      note = '被调用者帧:局部变量 / 保存寄存器 / 对齐填充'
    } else if (fpVal !== null && addrVal > fpVal) {
      note = '调用者帧:参数与上层状态'
    } else {
      note = '未建立帧指针(编译器可能省略 RBP/X29),按 SP 相对寻址'
    }
    out.push({
      addr: '0x' + addrVal.toString(16),
      qword: '0x' + le,
      mark,
      note,
    })
  }
  return out
})
</script>

<template>
  <div class="panel" style="height: 100%">
    <div class="panel-title">
      栈帧
      <n-tag size="tiny" :bordered="false" type="info">{{ spName }} {{ sp ?? '—' }}</n-tag>
      <n-tag size="tiny" :bordered="false">{{ fpName }} {{ fp ?? '—' }}</n-tag>
      <div style="flex: 1" />
      <n-button size="tiny" secondary @click="load">刷新</n-button>
    </div>
    <div class="panel-body">
      <div v-if="!rows.length" class="empty">
        {{ store.sessionId ? '栈区未映射或无可读数据' : '先创建会话并单步执行' }}
      </div>
      <table v-else class="stack">
        <thead>
          <tr><th>地址</th><th>8 字节(小端)</th><th>标注</th><th>说明</th></tr>
        </thead>
        <tbody>
          <tr v-for="r in rows" :key="r.addr" :class="{ marked: !!r.mark }">
            <td class="a">{{ r.addr }}</td>
            <td class="q">{{ r.qword }}</td>
            <td class="m">{{ r.mark || '·' }}</td>
            <td class="n">{{ r.note }}</td>
          </tr>
        </tbody>
      </table>
      <div class="legend">栈向低地址增长(↓ 图中地址递增方向为"回到调用者")</div>
    </div>
  </div>
</template>

<style scoped>
.stack { width: 100%; border-collapse: collapse; font-size: 12px; }
.stack th { color: #888; font-weight: 400; text-align: left; padding: 2px 6px; border-bottom: 1px solid #2a2a2a; }
.stack td { padding: 2px 6px; white-space: nowrap; }
.stack tr.marked { background: rgba(99, 226, 183, 0.08); }
.stack td.a { color: #7a7a7a; }
.stack td.q { color: #9ad1a8; font-family: Consolas, monospace; }
.stack td.m { color: #63e2b7; }
.stack td.n { color: #777; white-space: normal; }
.empty { color: #666; font-size: 12px; padding: 10px; }
.legend { color: #555; font-size: 11px; padding: 6px; }
</style>
