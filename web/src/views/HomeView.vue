<script setup lang="ts">
import { NCard, NButton, NTag, NSpace, NList, NListItem, NThing } from 'naive-ui'
import { onMounted, ref } from 'vue'
import { useRouter } from 'vue-router'
import { api } from '../api/client'

const router = useRouter()
const tools = ref<{ nasm: string | null; unicorn: boolean } | null>(null)

onMounted(async () => {
  try {
    tools.value = (await api.health()).tools
  } catch {
    tools.value = { nasm: null, unicorn: false }
  }
})
</script>

<template>
  <div style="max-width: 860px; margin: 0 auto; padding: 32px 20px">
    <h1 style="font-size: 28px; margin: 0 0 4px">Assembly Lab</h1>
    <p style="color: #888; margin: 0 0 24px">汇编语言交互式学习与实验平台 · Phase 1 MVP(x86-64 模拟器模式)</p>

    <n-space vertical size="large">
      <n-card title="实验工作台" size="small">
        <template #header-extra>
          <n-tag type="info" size="small">核心</n-tag>
        </template>
        逐指令执行 x86-64 汇编,观察寄存器、标志位、内存的每一步变化与解释。
        <template #action>
          <n-button type="primary" @click="router.push('/workbench')">进入工作台</n-button>
        </template>
      </n-card>

      <n-card title="工具链状态" size="small">
        <n-list>
          <n-list-item>
            <n-thing title="NASM 汇编器">
              <n-tag v-if="tools?.nasm" type="success" size="small">就绪</n-tag>
              <n-tag v-else type="error" size="small">未找到</n-tag>
              <span v-if="tools?.nasm" style="margin-left: 8px; color: #888; font-size: 12px">{{ tools.nasm }}</span>
            </n-thing>
          </n-list-item>
          <n-list-item>
            <n-thing title="Unicorn 模拟引擎(vendor/unicorn.dll)">
              <n-tag v-if="tools?.unicorn" type="success" size="small">就绪</n-tag>
              <n-tag v-else type="error" size="small">不可用</n-tag>
            </n-thing>
          </n-list-item>
        </n-list>
      </n-card>

      <n-card title="学习进度" size="small">
        <p style="color: #888; margin: 0">
          全部 7 个阶段已交付:27 门课(10 个阶段)/ 20 道题(8 种题型)/ 知识图谱 / AI 助教 / 离线课程包 / 双架构(x86-64 与 ARM64)实验室。
        </p>
      </n-card>
    </n-space>
  </div>
</template>
