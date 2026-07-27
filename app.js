const frame = document.getElementById("manualKitFrame");
const loadingScreen = document.getElementById("loadingScreen");
const installButton = document.getElementById("installButton");

let deferredPrompt = null;

frame.addEventListener("load", () => {
  loadingScreen.classList.add("is-hidden");
});

window.addEventListener("beforeinstallprompt", (event) => {
  console.log("beforeinstallprompt 발생");

  event.preventDefault();
  deferredPrompt = event;
  installButton.hidden = false;
});

installButton.addEventListener("click", async () => {
  if (!deferredPrompt) {
    console.warn("설치 프롬프트가 아직 준비되지 않았습니다.");
    return;
  }

  deferredPrompt.prompt();

  const choiceResult = await deferredPrompt.userChoice;

  console.log("설치 선택:", choiceResult.outcome);

  deferredPrompt = null;
  installButton.hidden = true;
});

window.addEventListener("appinstalled", () => {
  console.log("MONA Hub 설치 완료");
  deferredPrompt = null;
  installButton.hidden = true;
});

async function registerServiceWorker() {
  if (!("serviceWorker" in navigator)) {
    console.error("이 브라우저는 Service Worker를 지원하지 않습니다.");
    return;
  }

  try {
    const registration = await navigator.serviceWorker.register(
      "/service-worker.js"
    );

    console.log("Service Worker 등록 성공:", registration.scope);
  } catch (error) {
    console.error("Service Worker 등록 실패:", error);
  }
}

registerServiceWorker();