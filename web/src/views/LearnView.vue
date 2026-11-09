<script setup lang="ts">
// 学习中心:课程列表 + 课程内容渲染(15 步模板简化为内容流)+ 实验入口 + 进度
import { computed, onMounted, ref } from 'vue'
import { useRoute } from 'vue-router'
import {
  NCard, NTag, NButton, NList, NListItem, NThing, NSpace, NEmpty, NProgress, NSpin,
} from 'naive-ui'
import { marked } from 'marked'
import DOMPurify from 'dompurify'
import { api, type LessonListItem, type CourseSource } from '../api/client'

// 课程内容经 marked 渲染后必须过 DOMPurify(防课程数据文件中的恶意 HTML)
const md = (t: string | undefined) =>
  t ? DOMPurify.sanitize(marked.parse(t, { async: false }) as string) : ''

const route = useRoute()
const lessons = ref<LessonListItem[]>([])
const current = ref<Record<string, any> | null>(null)
const loading = ref(false)

const lessonId = computed(() => String(route.query.lesson ?? ''))

async function loadList() {
  lessons.value = await api.courses()
  if (lessons.value.length) {
    await openLesson(lessonId.value || lessons.value[0].id)
  }
}

async function openLesson(id: string) {
  loading.value = true
  try {
    current.value = (await api.course(id)) as Record<string, any>
  } finally {
    loading.value = false
  }
}

async function openExperiment() {
  if (!current.value) return
  const src: CourseSource = await api.courseExperiment(current.value.id)
  const payload = encodeURIComponent(src.source)
  // 按 experiment.mode 路由(P3):asm=模式A工作台 / debug=真机调试 / compare=优化对比
  const isa = src.isa && src.isa !== 'x86-64' ? `&isa=${encodeURIComponent(src.isa)}` : ''
  if (src.mode === 'debug') {
    const cmp = src.compiler && src.compiler !== 'g++' ? `&cmp=${encodeURIComponent(src.compiler)}` : ''
    window.location.hash = `/debug?exp=${payload}${cmp}`
  } else if (src.mode === 'compare') {
    window.location.hash = `/compare?exp=${payload}`
  } else {
    window.location.hash = `/workbench?exp=${payload}${isa}`
  }
}

const steps = computed(() => (current.value?.steps ?? []) as any[])
const KIND_LABELS: Record<string, string> = {
  problem: '问题', run: '运行', walkthrough: '逐步', state: '状态', theory: '原理',
  'cpp-link': 'C++ 联系', pitfalls: '误区', 'hands-on': '动手', exercise: '习题',
  summary: '总结', links: '关联', next: '下一步', scenario: '场景', observe: '观察',
  asm: '汇编',
}

function goExercise(id: string) {
  window.location.hash = `/exercises?id=${id}`
}

onMounted(loadList)
</script>

<template>
  <div style="display: flex; height: 100vh; overflow: hidden">
    <!-- 左:课程列表 -->
    <div class="sidebar">
      <div class="side-title">课程</div>
      <n-list hoverable clickable show-divider>
        <n-list-item
          v-for="l in lessons"
          :key="l.id"
          @click="openLesson(l.id)"
        >
          <n-thing :title="`${l.stage}·${l.title}`">
            <template #header-extra>
              <n-tag size="tiny" :type="l.progress === 'done' ? 'success' : l.progress === 'in_progress' ? 'info' : 'default'">
                {{ { not_started: '未学', in_progress: '进行中', done: '已完成' }[l.progress] ?? l.progress }}
              </n-tag>
            </template>
            <span style="color: #666; font-size: 12px">约 {{ l.minutes }} 分钟</span>
          </n-thing>
        </n-list-item>
      </n-list>
    </div>

    <!-- 右:课程内容 -->
    <div class="content">
      <n-spin :show="loading">
        <n-empty v-if="!current" description="选择左侧课程开始学习" style="margin-top: 20vh" />
        <template v-else>
          <div class="lesson-head">
            <n-tag type="info" size="small">阶段 {{ current.stage }}</n-tag>
            <h1>{{ current.title }}</h1>
            <div style="color: #888">{{ current.problem }}</div>
            <n-space style="margin-top: 8px">
              <n-button type="primary" size="small" @click="openExperiment">
                ⚗ 打开本课实验(工作台)
              </n-button>
              <n-button
                size="small"
                @click="api.setProgress(current.id, 'done')"
              >
                标记已完成
              </n-button>
            </n-space>
          </div>

          <n-card
            v-for="(s, i) in steps"
            :key="i"
            size="small"
            style="margin-bottom: 12px"
          >
            <template #header>
              <span class="step-kind">{{ KIND_LABELS[s.kind] ?? s.kind }}</span>
              {{ s.title }}
            </template>
            <div class="step-content" v-html="md(s.content)" />
            <div v-if="s.nodes" class="links">
              <n-tag v-for="n in s.nodes" :key="n" size="tiny" :bordered="false">{{ n }}</n-tag>
            </div>
            <div v-if="s.kind === 'exercise'" class="ex-links">
              <n-button
                v-for="e in current.exercises"
                :key="e.id"
                size="tiny"
                type="info"
                secondary
                @click="goExercise(e.id)"
              >
                前往:{{ e.title }}
              </n-button>
            </div>
          </n-card>

          <div class="done-bar">
            <n-progress
              type="line"
              :percentage="current.progress === 'done' ? 100 : current.progress === 'in_progress' ? 50 : 0"
              :show-indicator="false"
            />
          </div>
        </template>
      </n-spin>
    </div>
  </div>
</template>

<style scoped>
.sidebar {
  width: 260px;
  border-right: 1px solid #2a2a2a;
  overflow: auto;
  flex: none;
  padding: 10px;
}
.side-title {
  color: #888;
  font-size: 13px;
  margin: 4px 6px 10px;
}
.content {
  flex: 1;
  overflow: auto;
  padding: 20px 28px;
}
.lesson-head h1 {
  font-size: 22px;
  margin: 8px 0;
}
.step-kind {
  color: #63e2b7;
  font-size: 12px;
  margin-right: 8px;
  border: 1px solid #63e2b766;
  border-radius: 3px;
  padding: 1px 6px;
}
.step-content {
  white-space: pre-wrap;
  line-height: 1.7;
  color: #cfcfcf;
}
.links {
  margin-top: 8px;
  display: flex;
  gap: 6px;
  flex-wrap: wrap;
}
.ex-links {
  margin-top: 8px;
  display: flex;
  gap: 8px;
}
.done-bar {
  margin: 16px 0 40px;
}
</style>
