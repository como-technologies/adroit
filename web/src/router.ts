import { createRouter, createWebHistory, type RouteRecordRaw } from 'vue-router'
import DashboardView from './views/DashboardView.vue'
import BrowseView from './views/BrowseView.vue'
import DetailView from './views/DetailView.vue'
import InsightsView from './views/InsightsView.vue'

const routes: RouteRecordRaw[] = [
  { path: '/', name: 'dashboard', component: DashboardView },
  { path: '/browse', name: 'browse', component: BrowseView },
  { path: '/adr/:number', name: 'detail', component: DetailView, props: true },
  { path: '/insights', name: 'insights', component: InsightsView },
  // Redirects from old paths so existing links/bookmarks don't 404.
  { path: '/stats', redirect: '/' },
  { path: '/search', redirect: '/browse' },
  { path: '/graph', redirect: '/insights' },
]

export const router = createRouter({
  history: createWebHistory(),
  routes,
})
