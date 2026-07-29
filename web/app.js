const installView = document.getElementById("installView");
const appView = document.getElementById("appView");
const installButton = document.getElementById("installButton");
const installGuide = document.getElementById("installGuide");
const openManualKitButton = document.getElementById("openManualKit");

let deferredInstallPrompt = null;

function isStandaloneMode() {
  return window.matchMedia("(display-mode: standalone)").matches || window.navigator.standalone === true;
}

function applyDisplayMode() {
  const standalone = isStandaloneMode();
  document.body.classList.toggle("app-mode", standalone);
  document.body.classList.toggle("browser-mode", !standalone);
  installView.hidden = standalone;
  appView.hidden = !standalone;

  if (!standalone) {
    installGuide.textContent = "아래 설치 버튼을 눌러 MONA Hub를 설치하세요.";
  }
}

async function registerServiceWorker() {
  if (!("serviceWorker" in navigator)) return;
  try {
    await navigator.serviceWorker.register("service-worker.js");
  } catch (error) {
    console.error("Service Worker 등록 실패:", error);
  }
}

window.addEventListener("beforeinstallprompt", event => {
  event.preventDefault();
  deferredInstallPrompt = event;
  installButton.hidden = false;
  installGuide.textContent = "MONA Hub를 설치하면 독립된 앱 창으로 실행됩니다.";
});

installButton.addEventListener("click", async () => {
  if (!deferredInstallPrompt) {
    installGuide.textContent = "Chrome 또는 Edge 메뉴에서 'MONA Hub 설치'를 선택하세요.";
    return;
  }

  installButton.disabled = true;
  try {
    deferredInstallPrompt.prompt();
    const result = await deferredInstallPrompt.userChoice;
    installGuide.textContent = result.outcome === "accepted" ? "설치가 진행 중입니다." : "설치가 취소되었습니다.";
    if (result.outcome !== "accepted") installButton.disabled = false;
  } catch (error) {
    console.error("PWA 설치 오류:", error);
    installGuide.textContent = "설치 중 오류가 발생했습니다.";
    installButton.disabled = false;
  } finally {
    deferredInstallPrompt = null;
  }
});

window.addEventListener("appinstalled", () => {
  installButton.hidden = true;
  installGuide.textContent = "설치가 완료되었습니다. Windows 시작 메뉴에서 MONA Hub를 실행하세요.";
});

openManualKitButton.addEventListener("click", () => {
  const manualKitUrl = "https://manualkit.pages.dev/editor";
  const manualKitWindow = window.open(
    manualKitUrl,
    "monaManualKit",
    "popup=yes,width=1500,height=920,resizable=yes,scrollbars=yes"
  );

  if (!manualKitWindow) {
    window.location.href = manualKitUrl;
    return;
  }

  manualKitWindow.focus();
});

const standaloneMediaQuery = window.matchMedia("(display-mode: standalone)");
standaloneMediaQuery.addEventListener("change", applyDisplayMode);

applyDisplayMode();
registerServiceWorker();
