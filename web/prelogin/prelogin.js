const profileButton = document.getElementById("profileButton");

let activeE2eEntries = null;

function loginE2eLog(stage, clickStarted, extra = "") {
  const now = performance.now();
  const elapsed = clickStarted === undefined ? 0 : now - clickStarted;
  const message = `[LOGIN E2E] ${stage} t=${now.toFixed(1)}ms elapsed=${elapsed.toFixed(1)}ms${extra}`;
  console.log(message);
  activeE2eEntries?.push(message);
}

async function showLoginWindow() {
  const invoke = window.__TAURI__?.core?.invoke;
  if (!invoke) return;

  const clickStarted = performance.now();
  activeE2eEntries = [];
  loginE2eLog("click", clickStarted);

  try {
    loginE2eLog("invoke start", clickStarted);
    await invoke("show_login_window");
    loginE2eLog("invoke resolved", clickStarted);
    requestAnimationFrame(() => {
      loginE2eLog("RAF after invoke", clickStarted);
      requestAnimationFrame(() => {
        loginE2eLog("RAF + 1", clickStarted);
        const message = activeE2eEntries.join(" | ");
        activeE2eEntries = null;
        void invoke("log_login_diagnostic", { message });
      });
    });
  } catch (error) {
    console.error("로그인 창 표시 실패:", error);
  }
}

profileButton.addEventListener("click", showLoginWindow);
