import { createRouter, createWebHashHistory, type RouterHistory } from "vue-router";
import AuthGateView from "../views/AuthGateView.vue";
import CardCatalogView from "../views/CardCatalogView.vue";
import DrawView from "../views/DrawView.vue";
import HomeQuestionView from "../views/HomeQuestionView.vue";
import HistoryView from "../views/HistoryView.vue";
import SessionDetailView from "../views/SessionDetailView.vue";
import NotFoundView from "../views/NotFoundView.vue";

export function createAppRouter(history: RouterHistory = createWebHashHistory()) {
  return createRouter({
    history,
    routes: [
      { path: "/", name: "reading-room", component: HomeQuestionView, meta: { navSection: "new" } },
      { path: "/reading/draw/:drawId", name: "draw-view", component: DrawView, meta: { navSection: "new", parentNavigation: { to: "/", label: "返回新占卜" } } },
      { path: "/history", name: "history", component: HistoryView, meta: { navSection: "history" } },
      { path: "/history/:sessionId", name: "history-session", component: SessionDetailView, meta: { navSection: "history", parentNavigation: { to: "/history", label: "返回历史牌阵" } } },
      { path: "/reading/:sessionId", name: "session-detail", component: SessionDetailView, meta: { navSection: "history", parentNavigation: { to: "/history", label: "返回历史牌阵" } } },
      { path: "/settings/connection", name: "connection-settings", component: AuthGateView, meta: { navSection: "settings" } },
      { path: "/catalog", name: "card-catalog", component: CardCatalogView, meta: { navSection: "settings", parentNavigation: { to: "/settings/connection", label: "返回设置" } } },
      { path: "/:pathMatch(.*)*", name: "not-found", component: NotFoundView },
    ],
  });
}

export const router = createAppRouter();
