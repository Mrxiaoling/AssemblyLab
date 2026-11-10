<script setup lang="ts">
import { onMounted, ref, watch } from 'vue'
import { useRoute } from 'vue-router'
import { NAlert } from 'naive-ui'
import ControlBar from '../components/workbench/ControlBar.vue'
import CodePane from '../components/workbench/CodePane.vue'
import DisasmPane from '../components/workbench/DisasmPane.vue'
import RegisterPanel from '../components/workbench/RegisterPanel.vue'
import FlagsPanel from '../components/workbench/FlagsPanel.vue'
import MemoryViewer from '../components/workbench/MemoryViewer.vue'
import StackViewer from '../components/workbench/StackViewer.vue'
import TracePanel from '../components/workbench/TracePanel.vue'
import ExplainCard from '../components/workbench/ExplainCard.vue'
import { useSessionStore, defaultSourceFor } from '../stores/session'

const store = useSessionStore()
const route = useRoute()

// 三列宽度可拖拽,比例持久化到 localStorage(ROADMAP T1.x 的布局持久化要求)
const COLS_KEY = 'assemblylab.workbench.cols'
const colSizes = ref<number[]>([32, 36, 32])
onMounted(() => {
  try {
    const raw = localStorage.getItem(COLS_KEY)
    if (!raw) return
    const v: unknown = JSON.parse(raw)
    if (
      Array.isArray(v) && v.length === 3 &&
      v.every((n) => typeof n === 'number' && n >= 10) &&
      Math.abs(v.reduce((a: number, b: number) => a + b, 0) - 100) < 2
    ) {
      colSizes.value = v as number[]
    }
  } catch {
    // localStorage 不可用(隐私模式/file://)时用默认比例
  }
})
function onResized(panes: { size: number }[]) {
  if (!Array.isArray(panes) || panes.length !== 3) return
  colSizes.value = panes.map((p) => p.size)
  try {
    localStorage.setItem(COLS_KEY, JSON.stringify(colSizes.value))
  } catch {
    // 忽略持久化失败
  }
}

// 课程实验入口:workbench→workbench 同组件复用时 route.query 变化不触发 onMounted,
// 用 watch(immediate) 监听 exp/isa 变化重建会话(记录上次值防重复)
let lastExp: string | null = null
let lastIsa: string | null = null
watch(
  () => [route.query.exp, route.query.isa],
  async ([exp, isa]) => {
    const e = (exp as string | undefined) ?? null
    const i = (isa as string | undefined) ?? null
    if (e === lastExp && i === lastIsa) return
    lastExp = e
    lastIsa = i
    if (i && i !== store.isa) {
      store.isa = i
      if (!e) store.source = defaultSourceFor(i)
      store.sessionId = null
      store.isaDetail = null
    }
    if (e) store.source = e
    await store.create()
  },
  { immediate: true },
)
</script>

<template>
  <div class="wb-root">
    <ControlBar />
    <n-alert v-if="store.error" type="error" size="small" closable @close="store.error = null">
      {{ store.error }}
    </n-alert>

    <div class="wb-main">
      <splitpanes class="default-theme wb-cols" @resized="onResized">
        <!-- 左:源码 + 解释卡 -->
        <pane :size="colSizes[0]" min-size="12">
          <splitpanes class="default-theme" :vertical="true">
            <pane :size="62"><CodePane /></pane>
            <pane :size="38"><ExplainCard /></pane>
          </splitpanes>
        </pane>

        <!-- 中:反汇编(点击指令关联源码行) -->
        <pane :size="colSizes[1]" min-size="12">
          <DisasmPane />
        </pane>

        <!-- 右:寄存器/标志 + 内存 + 栈帧 -->
        <pane :size="colSizes[2]" min-size="12">
          <splitpanes class="default-theme" :vertical="true">
            <pane :size="34">
              <div class="stacked">
                <RegisterPanel />
                <FlagsPanel />
              </div>
            </pane>
            <pane :size="33"><MemoryViewer /></pane>
            <pane :size="33"><StackViewer /></pane>
          </splitpanes>
        </pane>
      </splitpanes>
    </div>

    <TracePanel />
  </div>
</template>

<script lang="ts">
import { Splitpanes, Pane } from 'splitpanes'
import 'splitpanes/dist/splitpanes.css'
export default {
  components: { Splitpanes, Pane },
}
</script>

<style>
.wb-root {
  height: calc(100vh - 41px);
  display: flex;
  flex-direction: column;
  overflow: hidden;
}
.wb-main {
  flex: 1;
  display: flex;
  min-height: 0;
  gap: 4px;
  padding: 4px;
}
.wb-main > * {
  min-width: 0;
  flex: 1;
}
.stacked {
  height: 100%;
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.stacked > * {
  flex: 1;
  min-height: 0;
}
.panel {
  border: 1px solid #2a2a2a;
  border-radius: 4px;
  display: flex;
  flex-direction: column;
  overflow: hidden;
  background: #181818;
}
.panel-title {
  font-size: 12px;
  color: #999;
  padding: 4px 10px;
  background: #202020;
  border-bottom: 1px solid #2a2a2a;
  flex: none;
  display: flex;
  align-items: center;
  gap: 8px;
}
.panel-body {
  flex: 1;
  overflow: auto;
  min-height: 0;
}
.default-theme .splitpanes__splitter {
  background: #2a2a2a;
  width: 5px !important;
}
.default-theme .splitpanes__pane {
  background: transparent;
}
</style>
