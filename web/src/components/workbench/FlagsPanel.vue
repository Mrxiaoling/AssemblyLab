<script setup lang="ts">
// 标志位面板:CF/PF/AF/ZF/SF/OF/DF,变化闪烁提示
import { computed } from 'vue'
import { useSessionStore } from '../../stores/session'

const store = useSessionStore()

const FLAGS = [
  { name: 'CF', bit: 0, hint: '无符号进位/借位' },
  { name: 'PF', bit: 2, hint: '奇偶校验(低 8 位)' },
  { name: 'AF', bit: 4, hint: '半进位' },
  { name: 'ZF', bit: 6, hint: '结果为零' },
  { name: 'SF', bit: 7, hint: '符号位' },
  { name: 'DF', bit: 10, hint: '串操作方向' },
  { name: 'OF', bit: 11, hint: '有符号溢出' },
]

// ISA 驱动的标志集合(arm64=NZCV)
const isaFlags = computed(() => {
  const d = store.isaDetail
  if (d && store.isa === 'arm64') {
    return d.flags.map((f) => ({ name: f.name, bit: f.bit, hint: f.semantics }))
  }
  return FLAGS
})

const rflags = computed<bigint>(() => {
  try {
    return BigInt(store.regMap.get(store.flagsRegister) ?? '0x0')
  } catch {
    return 0n
  }
})

const flagStates = computed(() =>
  isaFlags.value.map((f) => ({
    ...f,
    value: ((rflags.value >> BigInt(f.bit)) & 1n) === 1n,
  })),
)

// 最近报告中的标志变化(高亮)
const changedFlags = computed(() => {
  const m = new Map<string, [boolean, boolean]>()
  for (const r of store.lastReports) for (const f of r.flag_diff) m.set(f.name, [f.before, f.after])
  return m
})
</script>

<template>
  <div class="panel" style="flex: none">
    <div class="panel-title">标志位({{ store.flagsRegister }})</div>
    <div class="flags">
      <div
        v-for="f in flagStates"
        :key="f.name"
        class="flag"
        :class="{ on: f.value, changed: changedFlags.has(f.name) }"
        :title="`${f.name}: ${f.hint}`"
      >
        <span class="fname">{{ f.name }}</span>
        <span class="fval">{{ f.value ? 1 : 0 }}</span>
      </div>
    </div>
    <div v-if="store.lastReports.length" class="recent">
      <span v-for="[name, [b, a]] in changedFlags" :key="name" class="recent-item">
        {{ name }}: {{ b ? 1 : 0 }} → {{ a ? 1 : 0 }}
      </span>
    </div>
  </div>
</template>

<style scoped>
.flags {
  display: flex;
  flex-wrap: wrap;
  gap: 4px;
  padding: 6px 10px;
}
.flag {
  display: flex;
  gap: 4px;
  align-items: center;
  padding: 2px 8px;
  border: 1px solid #333;
  border-radius: 3px;
  font-size: 12px;
  color: #666;
  cursor: help;
}
.flag.on {
  color: #ffc83c;
  border-color: #ffc83c66;
  background: rgba(255, 200, 60, 0.08);
}
.flag.changed {
  outline: 1px solid #ffc83c88;
}
.fname {
  font-weight: 600;
}
.recent {
  padding: 0 10px 6px;
  font-size: 11px;
  color: #ffc83c;
  display: flex;
  gap: 10px;
  flex-wrap: wrap;
}
</style>
