import { createApp } from 'vue'
import { createPinia } from 'pinia'
import App from './App.vue'
import { router } from './router'

// Monaco worker(仅编辑器 worker;asm 高亮用本地 Monarch,无需语言 worker)
import editorWorker from 'monaco-editor/esm/vs/editor/editor.worker?worker'
import { registerAsmLanguage } from './monarch/asm'

self.MonacoEnvironment = {
  getWorker() {
    return new editorWorker()
  },
}

const app = createApp(App)
app.use(createPinia())
app.use(router)
app.mount('#app')

// 提前注册汇编语言(不阻塞渲染)
void registerAsmLanguage()
