<script setup lang="ts">
// 执行轨迹:全部步进记录,可回看任意一步的 diff
import { computed, ref, watch } from 'vue'
import { NTag } from 'naive-ui'
import { useSessionStore } from '../../stores/session'

const store = useSessionStore()
const selected = ref<number | null>(null)

watch(
  () => store.trace.length,
  () => {
    // 自动滚动到底部
    const el = document.querySelector('.trace-body')
    el?.scrollTo({ top: el.scrollHeight })
  },
)

const selectedEntry = computed(() => (selected.value !== null ? store.trace[selected.value] : null))
const selectedDetails = computed(() =>
  selectedEntry.value?.kind === 'step' ? selectedEntry.value.report.explanation.detail : [],
)

function summaryOf(entry: (typeof store.trace)[number]): string {
  if (entry.kind === 'run') {
    return `运行 ${entry.summary.executed} 条指令 → ${entry.summary.stop.kind}`
  }
  const r = entry.report
  const parts: string[] = []
  if (r.reg_diff.length) parts.push(`${r.reg_diff.length} 寄存器变化`)
  if (r.flag_diff.length) parts.push(r.flag_diff.map((f) => f.name).join('/') + ' 变化')
  if (r.mem_changes.length) parts.push('内存写')
  return parts.length ? parts.join(', ') : '(无可见变化)'
}
</script>

<template>
  <div class="panel" style="flex: none; height: 180px">
    <div class="panel-title">
      执行轨迹
      <span v-if="store.traceDropped" style="color: #f0a35e">(已丢弃最早 {{ store.traceDropped }} 条)</span>
    </div>
    <div class="panel-body trace-body">
      <table class="trace">
        <tbody>
          <tr
            v-for="(e, i) in store.trace"
            :key="i"
            :class="{ sel: selected === i }"
            @click="selected = selected === i ? null : i"
          >
            <td class="t-kind">
              <n-tag size="tiny" :bordered="false" :type="e.kind === 'run' ? 'success' : 'default'">
                {{ e.kind === 'run' ? 'RUN' : `#${e.step_no}` }}
              </n-tag>
            </td>
            <td class="t-addr">{{ e.kind === 'step' ? e.report.address : '—' }}</td>
            <td class="t-dis">{{ e.kind === 'step' ? e.report.disasm : '运行模式' }}</td>
            <td class="t-sum">{{ summaryOf(e) }}</td>
          </tr>
        </tbody>
      </table>
      <div v-if="selectedDetails.length" class="sel-detail">
        <span v-for="d in selectedDetails" :key="d">{{ d }}</span>
      </div>
    </div>
  </div>
</template>

<style scoped>
.trace {
  width: 100%;
  border-collapse: collapse;
  font-size: 12px;
}
.trace tr {
  cursor: pointer;
}
.trace tr:hover {
  background: #232323;
}
.trace tr.sel {
  background: rgba(64, 160, 255, 0.14);
}
.t-kind {
  width: 60px;
  padding-left: 10px;
}
.t-addr {
  color: #7a7a7a;
  width: 90px;
}
.t-dis {
  color: #d8d8d8;
  width: 200px;
}
.t-sum {
  color: #777;
}
.sel-detail {
  border-top: 1px solid #2a2a2a;
  padding: 4px 10px;
  color: #9ad1a8;
  font-size: 12px;
  display: flex;
  flex-direction: column;
  gap: 2px;
}
</style>
