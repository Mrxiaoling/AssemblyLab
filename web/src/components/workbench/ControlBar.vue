<script setup lang="ts">
import { computed, ref } from 'vue'
import { NButton, NButtonGroup, NInputNumber, NTag, NSpace, NTooltip, NPopconfirm, NSelect } from 'naive-ui'
import { useSessionStore } from '../../stores/session'

const store = useSessionStore()
const stepCount = ref(1)

const isaOptions = [
  { label: 'x86-64 (NASM)', value: 'x86-64' },
  { label: 'ARM64 (GNU as)', value: 'arm64' },
]
async function onIsaChange(v: string) {
  await store.switchIsa(v)
}

const stopText = computed(() => {
  const stop = store.state?.stop
  if (!stop) return null
  switch (stop.kind) {
    case 'program_exit':
      return `程序退出(返回码 ${stop.code ?? '-'})`
    case 'breakpoint':
      return `断点 @ ${stop.address}`
    case 'fault':
      return `执行错误: ${stop.message}`
    case 'instruction_limit':
      return '达到指令数上限'
    case 'timeout':
      return '超时'
    default:
      return null
  }
})

const stopType = computed(() => {
  const k = store.state?.stop?.kind
  if (k === 'breakpoint') return 'info'
  if (k === 'program_exit') return 'success'
  return 'error'
})

async function onReset() {
  await store.reset()
}

async function onPatchRegister(name: string) {
  const cur = store.regMap.get(name) ?? '0x0'
  const input = window.prompt(`修改 ${name}(支持 0x 十六进制):`, cur)
  if (input == null) return
  await store.patchRegister(name, input)
}
defineExpose({ onPatchRegister })
</script>

<template>
  <div class="ctrl">
    <n-button-group size="small">
      <n-button type="primary" :disabled="store.busy || store.state?.terminal" @click="store.run()">
        ▶ 运行
      </n-button>
      <n-button :disabled="store.busy || store.state?.terminal" @click="store.step(stepCount)">
        单步 ×{{ stepCount }}
      </n-button>
      <n-input-number v-model:value="stepCount" size="small" :min="1" :max="1000" style="width: 90px" />
    </n-button-group>

    <n-button-group size="small">
      <n-button :disabled="store.busy" @click="onReset">⟲ 重置</n-button>
      <n-tooltip trigger="hover">
        <template #trigger>
          <n-button :disabled="store.busy" @click="store.reassemble()">编译源码</n-button>
        </template>
        重新汇编并重载(保留有效断点)
      </n-tooltip>
    </n-button-group>

    <n-space size="small" align="center">
      <n-tag v-if="store.state?.terminal" :type="stopType" size="small">{{ stopText }}</n-tag>
      <n-tag v-else-if="stopText" type="info" size="small">{{ stopText }}</n-tag>
      <n-tag v-if="store.state" size="small" :bordered="false">
        步数 {{ store.state.step_count }}
      </n-tag>
    </n-space>

    <div style="flex: 1" />

    <span class="hint">点击反汇编行设断点 · 双击寄存器修改</span>
    <n-select
      :value="store.isa"
      size="small"
      :options="isaOptions"
      style="width: 150px"
      @update:value="onIsaChange"
    />
  </div>
</template>

<style scoped>
.ctrl {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 6px 10px;
  background: #202020;
  border-bottom: 1px solid #2a2a2a;
  flex: none;
}
.hint {
  color: #555;
  font-size: 12px;
}
</style>
