<script setup lang="ts">
// 寄存器面板:分组展示、变化高亮、双击修改
import { computed } from 'vue'
import { NTooltip } from 'naive-ui'
import { useSessionStore } from '../../stores/session'

const store = useSessionStore()

const GROUPS: { title: string; names: string[]; special?: boolean }[] = [
  { title: '通用', names: ['RAX', 'RBX', 'RCX', 'RDX'] },
  { title: '变址', names: ['RSI', 'RDI', 'RBP', 'RSP'] },
  { title: 'R8–R15', names: ['R8', 'R9', 'R10', 'R11', 'R12', 'R13', 'R14', 'R15'] },
  { title: '状态', names: ['RIP', 'RFLAGS'], special: true },
]

// ISA 驱动分组(arm64 等):按 ISA 数据包的 group 字段聚合,保持 panel 顺序
const isaGroups = computed(() => {
  const detail = store.isaDetail
  if (!detail) return null
  const groups: { title: string; names: string[]; special?: boolean }[] = []
  for (const r of detail.registers) {
    if (!store.regMap.has(r.canonical)) continue // 只显示面板实际返回的
    let g = groups.find((x) => x.title === r.group)
    if (!g) {
      g = { title: r.group || '寄存器', names: [] }
      groups.push(g)
    }
    g.names.push(r.canonical)
  }
  // 状态组(PC/标志)置底并标 special
  for (const g of groups) {
    if (g.names.includes('PC') || g.names.includes('RIP')) g.special = true
  }
  return groups
})
const groups = computed(() => isaGroups.value ?? GROUPS)

// 最近一次报告中的变化寄存器(高亮)
const changedRegs = computed(() => {
  const s = new Set<string>()
  for (const r of store.lastReports) for (const d of r.reg_diff) s.add(d.name)
  return s
})

const dec = (hex: string): string => {
  const v = BigInt(hex)
  return v <= BigInt(Number.MAX_SAFE_INTEGER) ? v.toString() : '—'
}

function toDec(hex: string): string {
  try {
    const v = BigInt(hex)
    return v <= BigInt(Number.MAX_SAFE_INTEGER) ? v.toString() : ''
  } catch {
    return ''
  }
}

async function edit(name: string) {
  const cur = store.regMap.get(name) ?? '0x0'
  const input = window.prompt(`修改 ${name}(0x 前缀十六进制):`, cur)
  if (input == null) return
  await store.patchRegister(name, input)
}
</script>

<template>
  <div class="panel" style="flex: 1 1 60%; min-height: 0">
    <div class="panel-title">寄存器 <span style="color:#555">(双击修改)</span></div>
    <div class="panel-body">
      <div v-for="g in groups" :key="g.title" class="group">
        <div class="group-title">{{ g.title }}</div>
        <div class="rows">
          <n-tooltip v-for="name in g.names" :key="name" trigger="hover" placement="left">
            <template #trigger>
              <div
                class="reg"
                :class="{ changed: changedRegs.has(name), special: g.special }"
                @dblclick="edit(name)"
              >
                <span class="rname">{{ name }}</span>
                <span class="rval">{{ store.regMap.get(name) ?? '—' }}</span>
                <span class="rdec">{{ toDec(store.regMap.get(name) ?? '') }}</span>
              </div>
            </template>
            {{ name }} = {{ store.regMap.get(name) ?? '—' }}
            <template v-if="toDec(store.regMap.get(name) ?? '')">
              (十进制 {{ toDec(store.regMap.get(name) ?? '') }})
            </template>
          </n-tooltip>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.group {
  padding: 4px 0;
}
.group-title {
  font-size: 11px;
  color: #666;
  padding: 2px 10px;
}
.reg {
  display: flex;
  align-items: center;
  padding: 1px 10px;
  font-size: 12.5px;
  cursor: default;
  border-left: 2px solid transparent;
}
.reg.changed {
  background: rgba(255, 200, 60, 0.12);
  border-left-color: #ffc83c;
}
.reg.special .rname {
  color: #c792ea;
}
.rname {
  width: 62px;
  color: #82aaff;
}
.rval {
  font-family: inherit;
  color: #e8e8e8;
  width: 150px;
}
.rdec {
  color: #555;
  font-size: 11px;
}
</style>
