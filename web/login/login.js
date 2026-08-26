import { AccessAuthProvider } from "../auth/access-auth-provider.js";
import { authConfig } from "../app/config/environment.js";

const closeButton = document.getElementById("closeButton");
const minimizeButton = document.getElementById("minimizeButton");
const loginButton = document.getElementById("loginButton");
const loginStatus = document.getElementById("loginStatus");
const accessAuth = new AccessAuthProvider({ appUrl: authConfig.appUrl });

function pageLog(event) {
  console.log(
    `[LOGIN PAGE] ${event} t=${performance.now().toFixed(1)}ms visibility=${document.visibilityState}`
  );
  void logViewport(event);
}

async function logViewport(event) {
  const invoke = window.__TAURI__?.core?.invoke;
  if (!invoke) return;

  try {
    await invoke("log_login_viewport", {
      event,
      innerWidth: window.innerWidth,
      innerHeight: window.innerHeight,
      clientWidth: document.documentElement.clientWidth,
      clientHeight: document.documentElement.clientHeight,
      devicePixelRatio: window.devicePixelRatio
    });
  } catch (error) {
    console.error("로그인 viewport 진단 전달 실패:", error);
  }
}

async function notifyPageReady() {
  const invoke = window.__TAURI__?.core?.invoke;
  if (!invoke) return;

  try {
    await invoke("notify_login_page_ready");
  } catch (error) {
    console.error("로그인 페이지 준비 상태 전달 실패:", error);
  }
}

function setLoginStatus(message) {
  loginStatus.textContent = message;
}

async function handleLogin() {
  const invoke = window.__TAURI__?.core?.invoke;
  if (!invoke) {
    setLoginStatus("Tauri 인증 연결을 사용할 수 없습니다.");
    return;
  }

  loginButton.disabled = true;
  setLoginStatus("Cloudflare Access에 연결 중입니다.");

  try {
    // Rust가 이 시점부터 최종 navigation을 관찰한 뒤에만 성공 처리한다.
    await invoke("begin_access_login");
    accessAuth.login();
  } catch (error) {
    loginButton.disabled = false;
    setLoginStatus("인증을 시작하지 못했습니다.");
    console.error("Cloudflare Access 로그인 시작 실패:", error);
  }
}

async function closeLoginWindow() {
  const invoke = window.__TAURI__?.core?.invoke;
  if (!invoke) return;

  try {
    await invoke("close_login_window");
  } catch (error) {
    console.error("로그인 창 닫기 실패:", error);
  }
}

async function minimizeLoginWindow() {
  const invoke = window.__TAURI__?.core?.invoke;
  if (!invoke) return;

  try {
    await invoke("minimize_login_window");
  } catch (error) {
    console.error("로그인 창 최소화 실패:", error);
  }
}

loginButton.addEventListener("click", handleLogin);
closeButton.addEventListener("click", closeLoginWindow);
minimizeButton.addEventListener("click", minimizeLoginWindow);
window.addEventListener("load", () => pageLog("load"));
window.addEventListener("focus", () => pageLog("focus"));
window.addEventListener("blur", () => pageLog("blur"));
window.addEventListener("resize", () => pageLog("resize"));
document.addEventListener("visibilitychange", () => pageLog("visibilitychange"));

pageLog("script evaluated");
notifyPageReady();
