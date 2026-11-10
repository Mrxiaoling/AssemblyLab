import { defineConfig } from 'vitest/config'

// 缓存放到项目内、单 fork 串行执行:避免 CI/沙箱环境下写入系统 Temp 触发 EPERM,
// 也避免两个测试文件并行时抢同一份 ssr 缓存导致其中一个被静默跳过。
export default defineConfig({
  cacheDir: './node_modules/.vitest-cache',
  test: {
    environment: 'node',
    pool: 'forks',
    poolOptions: { forks: { singleFork: true } },
  },
})
