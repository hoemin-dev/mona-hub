import { authConfig } from "./config/environment.js";
import { AuthController } from "./auth/auth-controller.js";
import { AccessAuthProvider } from "./auth/access-auth-provider.js";

const launcherStatuses = document.querySelectorAll("#launcherStatus, [data-launcher-status]");
const appButtons = document.querySelectorAll(".app-button[data-app-name]");
const largeAppButtons = document.querySelectorAll(".large-app-card[data-app-name]");
const sizeToggles = document.querySelectorAll("#sizeToggle, [data-size-toggle]");
const allAppsButtons = document.querySelectorAll("#allAppsButton, [data-all-apps-button]");
const profileButtons = document.querySelectorAll("#profileButton, [data-profile-button]");
const profileLabels = document.querySelectorAll("#profileLabel, [data-profile-label]");

const EXPANDED_WIDTH = 800;
const COMPACT_WIDTH = 36;

const SIZE_STORAGE_KEY = "mona-hub.appbar-size";
const auth = new AuthController(new AccessAuthProvider(authConfig), authConfig);

let isCompact = true;
let isResizing = false;

async function clearLegacyWebData() {
  try {
    if ("serviceWorker" in navigator) {
      const registrations = await navigator.serviceWorker.getRegistrations();
      await Promise.all(registrations.map(registration => registration.unregister()));
    }

    if ("caches" in window) {
      const cacheNames = await caches.keys();
      await Promise.all(cacheNames.map(cacheName => caches.delete(cacheName)));
    }
  } catch (error) {
    console.warn("기존 웹 캐시 정리 실패:", error);
  }
}

function setStatus(message) {
  launcherStatuses.forEach(status => { status.textContent = message; });
}

function applyCompactMode(compact) {
  isCompact = compact;
  document.documentElement.classList.toggle("mode-small", compact);
  document.documentElement.classList.toggle("mode-large", !compact);
  document.body.classList.toggle("is-compact", compact);
  document.querySelector(".launcher-small")?.setAttribute("aria-hidden", String(!compact));
  document.querySelector(".launcher-large")?.setAttribute("aria-hidden", String(compact));
  sizeToggles.forEach(toggle => {
    toggle.setAttribute("aria-pressed", String(compact));
    toggle.setAttribute("aria-label", compact ? "Large 모드로 전환" : "Small 모드로 전환");
    toggle.title = compact ? "Large 모드로 전환" : "Small 모드로 전환";
  });
}

function savedCompactMode() {
  return localStorage.getItem(SIZE_STORAGE_KEY) !== "large";
}

async function restoreLauncherWidth() {
  const compact = savedCompactMode();
  applyCompactMode(compact);

  const invoke = window.__TAURI__?.core?.invoke;
  if (!invoke) return;

  try {
    await invoke("set_appbar_width", {
      width: compact ? COMPACT_WIDTH : EXPANDED_WIDTH
    });
  } catch (error) {
    console.error("저장된 AppBar 크기 복원 실패:", error);
    setStatus("크기 복원 실패");
  }
}

function renderAuthState() {
  const authenticated = auth.isAuthenticated();
  const identity = auth.getIdentity();
  document.body.classList.toggle("is-authenticated", authenticated);
  document.body.classList.toggle("is-signed-out", !authenticated);
  profileButtons.forEach(button => {
    button.dataset.authState = authenticated ? "signed-in" : "signed-out";
    button.setAttribute("aria-label", authenticated ? "로그아웃" : "로그인");
    button.title = authenticated ? "로그아웃" : "로그인";
  });
  profileLabels.forEach(label => {
    label.textContent = authenticated ? (identity?.name || identity?.email || "로그아웃") : "로그인";
  });
  setStatus(authenticated ? "준비됨" : "로그아웃 상태");
}

async function toggleLauncherWidth() {
  if (isResizing) return;

  const invoke = window.__TAURI__?.core?.invoke;
  if (!invoke) {
    setStatus("크기 변경 불가");
    return;
  }

  isResizing = true;
  sizeToggles.forEach(toggle => { toggle.disabled = true; });

  const nextCompact = !isCompact;
  const nextWidth = nextCompact ? COMPACT_WIDTH : EXPANDED_WIDTH;
  const previousCompact = isCompact;

  try {
    applyCompactMode(nextCompact);
    await invoke("set_appbar_width", { width: nextWidth });
    localStorage.setItem(SIZE_STORAGE_KEY, nextCompact ? "small" : "large");
  } catch (error) {
    applyCompactMode(previousCompact);
    console.error("런처 바 크기 변경 실패:", error);
    setStatus("크기 변경 실패");
  } finally {
    sizeToggles.forEach(toggle => { toggle.disabled = false; });
    isResizing = false;
  }
}

function openApp(button) {
  const appName = button.dataset.appName;
  const appUrl = button.dataset.appUrl;

  if (!appUrl) {
    setStatus("준비 중");
    return;
  }

  setStatus(`${appName} 여는 중`);

  const windowName = `mona${appName.replace(/[^a-z0-9]/gi, "")}`;
  const appWindow = window.open(
    appUrl,
    windowName,
    "popup=yes,width=1500,height=920,resizable=yes,scrollbars=yes"
  );

  if (!appWindow) {
    setStatus("열기 실패");
    return;
  }

  appWindow.focus();
  setStatus("준비됨");
}

async function handleProfileAction() {
  const authState = profileButtons[0]?.dataset.authState;

  if (authState === "signed-in") {
    try {
      setStatus("로그아웃 중");
      await auth.logout();
    } catch (error) {
      console.error("Cloudflare Access 로그아웃 실패:", error);
      setStatus("로그아웃 실패");
    }
    return;
  }

  setStatus("로그인으로 이동 중");
  auth.login();
}

appButtons.forEach(button => {
  button.addEventListener("click", () => openApp(button));
});

largeAppButtons.forEach(button => {
  button.addEventListener("click", () => openApp(button));
});

sizeToggles.forEach(toggle => toggle.addEventListener("click", toggleLauncherWidth));
allAppsButtons.forEach(button => button.addEventListener("click", () => {
  setStatus("전체 앱 준비 중");
}));
profileButtons.forEach(button => {
  button.addEventListener("click", handleProfileAction);
});

auth.addEventListener("change", renderAuthState);
window.addEventListener("focus", () => auth.isAuthenticated() && auth.requireSession());
document.addEventListener("visibilitychange", () => {
  if (document.visibilityState === "visible" && auth.isAuthenticated()) auth.requireSession();
});

restoreLauncherWidth();
renderAuthState();
auth.requireSession();
clearLegacyWebData();
