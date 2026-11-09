<script setup lang="ts">
// 反汇编视图:断点点击、当前指令高亮
import { computed, nextTick, ref, watch } from 'vue'
import { useSessionStore } from '../../stores/session'

const store = useSessionStore()
const bodyEl = ref<HTMLElement>()

const currentAddr = computed(() => store.rip?.toLowerCase())

function isBp(addr: string): boolean {
  return store.breakpointsSet.has(addr.toLowerCase())
}

function isCurrent(addr: string): boolean {
  return addr.toLowerCase() === currentAddr.value
}

async function onRowClick(addr: string) {
  await store.toggleBreakpoint(addr.toLowerCase())
}

// 当前指令变化时滚动可见
watch(currentAddr, async (a) => {
  if (!a || !bodyEl.value) return
  await nextTick()
  const el = bodyEl.value.querySelector(`[data-addr="${a}"]`)
  el?.scrollIntoView({ block: 'nearest' })
})
</script>

<template>
  <div class="panel" style="height: 100%">
    <div class="panel-title">
      反汇编
      <span style="color: #555">(点击行设/删断点)</span>
    </div>
    <div ref="bodyEl" class="panel-body">
      <table class="disasm">
        <tbody>
          <tr
            v-for="l in store.disasm?.lines ?? []"
            :key="l.address"
            :data-addr="l.address.toLowerCase()"
            :class="{ current: isCurrent(l.address), bp: isBp(l.address) }"
            @click="onRowClick(l.address)"
          >
            <td class="bp-col">{{ isBp(l.address) ? '●' : '·' }}</td>
            <td class="addr-col">{{ l.address }}</td>
            <td class="bytes-col">{{ l.bytes_hex }}</td>
            <td class="text-col" :class="{ bold: isCurrent(l.address) }">{{ l.text }}</td>
          </tr>
        </tbody>
      </table>
    </div>
  </div>
</template>

<style scoped>
.disasm {
  width: 100%;
  border-collapse: collapse;
  font-size: 12.5px;
  cursor: default;
}
.disasm tr:hover {
  background: #232323;
}
.disasm tr.current {
  background: rgba(64, 160, 255, 0.16);
}
.disasm tr.current .addr-col,
.disasm tr.bp .addr-col {
  color: #ff6b6b;
}
.bp-col {
  width: 18px;
  text-align: center;
  color: #ff6b6b;
  user-select: none;
}
.addr-col {
  color: #7a7a7a;
  padding-right: 8px;
  width: 84px;
}
.bytes-col {
  color: #5a5a5a;
  padding-right: 8px;
  white-space: nowrap;
}
.text-col {
  color: #d8d8d8;
  white-space: nowrap;
}
.bold {
  color: #fff;
}
</style>
