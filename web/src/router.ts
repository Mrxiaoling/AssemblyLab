import { createRouter, createWebHashHistory } from 'vue-router'
import HomeView from './views/HomeView.vue'
// P1 不做代码分割:懒加载路由会在 dev 模式触发依赖发现竞态(vite + naive-ui/pinia),
// 静态导入与 HomeView 同构,行为已被验证稳定。
import WorkbenchView from './views/WorkbenchView.vue'
import LearnView from './views/LearnView.vue'
import ExerciseView from './views/ExerciseView.vue'
import DebugView from './views/DebugView.vue'
import CompareView from './views/CompareView.vue'
import KnowledgeView from './views/KnowledgeView.vue'
import TutorView from './views/TutorView.vue'

export const router = createRouter({
  history: createWebHashHistory(),
  routes: [
    { path: '/', component: HomeView },
    {
      path: '/workbench',
      component: WorkbenchView,
    },
    { path: '/learn', component: LearnView },
    { path: '/exercises', component: ExerciseView },
    { path: '/debug', component: DebugView },
    { path: '/compare', component: CompareView },
    { path: '/knowledge', component: KnowledgeView },
    { path: '/tutor', component: TutorView },
    { path: '/:pathMatch(.*)*', redirect: '/' },
  ],
})
// MARKER-1794137853
