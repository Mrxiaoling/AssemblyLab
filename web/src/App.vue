<script setup lang="ts">
import { NConfigProvider, NGlobalStyle, darkTheme, zhCN, dateZhCN } from 'naive-ui'
</script>

<template>
  <n-config-provider :theme="darkTheme" :locale="zhCN" :date-locale="dateZhCN">
    <n-global-style />
    <nav class="top-nav">
      <router-link to="/" class="brand">⬢ Assembly Lab</router-link>
      <router-link to="/learn" class="nav-item">学习中心</router-link>
      <router-link to="/exercises" class="nav-item">习题中心</router-link>
      <router-link to="/workbench" class="nav-item">实验工作台</router-link>
      <router-link to="/debug" class="nav-item">真机调试</router-link>
      <router-link to="/compare" class="nav-item">优化对比</router-link>
      <router-link to="/knowledge" class="nav-item">知识图谱</router-link>
      <router-link to="/tutor" class="nav-item">AI 助教</router-link>
    </nav>
    <div class="app-shell">
      <router-view />
    </div>
  </n-config-provider>
</template>

<style scoped>
.top-nav {
  display: flex;
  align-items: center;
  gap: 18px;
  padding: 6px 16px;
  background: #202020;
  border-bottom: 1px solid #2a2a2a;
  height: 34px;
}
.brand {
  color: #63e2b7;
  text-decoration: none;
  font-weight: 600;
  margin-right: 12px;
}
.nav-item {
  color: #999;
  text-decoration: none;
  font-size: 13px;
}
.nav-item.router-link-active {
  color: #63e2b7;
}
</style>

<style>
html,
body,
#app {
  height: 100%;
  margin: 0;
}
.app-shell {
  height: calc(100% - 41px);
}
body {
  font-family: 'Cascadia Code', 'JetBrains Mono', Consolas, 'Segoe UI', monospace;
}
</style>
