const closeButton = document.getElementById("closeButton");
const loginForm = document.getElementById("loginForm");
const userIdInput = document.getElementById("userId");
const passwordInput = document.getElementById("password");
const loginStatus = document.getElementById("loginStatus");

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

async function submitLogin(credentials) {
  // TODO: Replace this mock boundary with POST /auth/login.
  void credentials;
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

loginForm.addEventListener("submit", async event => {
  event.preventDefault();

  if (!loginForm.reportValidity()) return;

  await submitLogin({
    userId: userIdInput.value,
    password: passwordInput.value
  });
});

closeButton.addEventListener("click", closeLoginWindow);

requestAnimationFrame(() => userIdInput.focus());
notifyPageReady();
