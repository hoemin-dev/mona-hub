const closeButton = document.getElementById("closeButton");
const minimizeButton = document.getElementById("minimizeButton");
const loginButton = document.getElementById("loginButton");
const loginStatus = document.getElementById("loginStatus");

function pageLog(event) {
  console.log(
    `[LOGIN PAGE] ${event} t=${performance.now().toFixed(1)}ms visibility=${document.visibilityState}`
  );
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

function handleLogin() {
  setLoginStatus("인증 서비스 연결 준비 중입니다.");
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
