const installView = document.getElementById("installView");
const appView = document.getElementById("appView");

const installButton = document.getElementById("installButton");
const installGuide = document.getElementById("installGuide");

const manualKitFrame = document.getElementById("manualKitFrame");
const loadingScreen = document.getElementById("loadingScreen");

let deferredInstallPrompt = null;


/**
 * 현재 창이 설치된 PWA 상태인지 확인
 */
function isStandaloneMode() {
  return (
    window.matchMedia("(display-mode: standalone)").matches ||
    window.navigator.standalone === true
  );
}


/**
 * 브라우저 모드와 앱 모드를 구분
 */
function applyDisplayMode() {
  const standalone = isStandaloneMode();

  document.body.classList.toggle("app-mode", standalone);
  document.body.classList.toggle("browser-mode", !standalone);

  installView.hidden = standalone;
  appView.hidden = !standalone;

  if (!standalone) {
    showBrowserGuide();
  }
}


/**
 * 브라우저 접속 상태 안내
 */
function showBrowserGuide() {
  installGuide.textContent =
    "아래 설치 버튼을 눌러 MONA Hub를 설치하세요.";
}


/**
 * Service Worker 등록
 */
async function registerServiceWorker() {
  if (!("serviceWorker" in navigator)) {
    console.warn("이 브라우저는 Service Worker를 지원하지 않습니다.");
    return;
  }

  try {
    const registration = await navigator.serviceWorker.register(
      "/service-worker.js"
    );

    console.log(
      "Service Worker 등록 완료:",
      registration.scope
    );
  } catch (error) {
    console.error(
      "Service Worker 등록 실패:",
      error
    );
  }
}


/**
 * Chrome 설치 이벤트
 */
window.addEventListener("beforeinstallprompt", event => {
  event.preventDefault();

  deferredInstallPrompt = event;

  installButton.hidden = false;
  installGuide.textContent =
    "MONA Hub를 설치하면 독립된 앱 창으로 실행됩니다.";
});


/**
 * 설치 버튼 클릭
 */
installButton.addEventListener("click", async () => {
  if (!deferredInstallPrompt) {
    installGuide.textContent =
      "Chrome 메뉴에서 'MONA Hub 설치'를 선택하세요.";
    return;
  }

  installButton.disabled = true;

  try {
    deferredInstallPrompt.prompt();

    const choiceResult =
      await deferredInstallPrompt.userChoice;

    if (choiceResult.outcome === "accepted") {
      installGuide.textContent =
        "설치가 진행 중입니다.";
    } else {
      installGuide.textContent =
        "설치가 취소되었습니다.";

      installButton.disabled = false;
    }
  } catch (error) {
    console.error("PWA 설치 오류:", error);

    installGuide.textContent =
      "설치 중 오류가 발생했습니다.";

    installButton.disabled = false;
  } finally {
    deferredInstallPrompt = null;
  }
});


/**
 * 설치 완료
 */
window.addEventListener("appinstalled", () => {
  installButton.hidden = true;

  installGuide.textContent =
    "설치가 완료되었습니다. Windows 시작 메뉴에서 MONA Hub를 실행하세요.";
});


/**
 * iframe 로딩 완료
 */
manualKitFrame.addEventListener("load", () => {
  loadingScreen.classList.add("is-hidden");
});


/**
 * display-mode가 변경되는 경우 대응
 */
const standaloneMediaQuery =
  window.matchMedia("(display-mode: standalone)");

standaloneMediaQuery.addEventListener(
  "change",
  applyDisplayMode
);


/**
 * 초기 실행
 */
applyDisplayMode();
registerServiceWorker();