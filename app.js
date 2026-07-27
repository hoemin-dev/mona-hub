const frame = document.getElementById("manualKitFrame");
const loadingScreen = document.getElementById("loadingScreen");
const installButton = document.getElementById("installButton");

let installPrompt = null;

/*
 * Manual Kit 로딩 완료
 */
frame.addEventListener("load", () => {
  loadingScreen.classList.add("is-hidden");
});

/*
 * PWA 설치 가능 상태가 되면
 * 브라우저의 설치 이벤트를 저장해 둔다.
 */
window.addEventListener("beforeinstallprompt", (event) => {
  event.preventDefault();

  installPrompt = event;
  installButton.hidden = false;
});

/*
 * 사용자 설치 버튼 클릭
 */
installButton.addEventListener("click", async () => {
  if (!installPrompt) {
    return;
  }

  installButton.hidden = true;

  await installPrompt.prompt();

  const result = await installPrompt.userChoice;

  console.log("PWA install result:", result.outcome);

  installPrompt = null;
});

/*
 * 설치가 완료되면 버튼 제거
 */
window.addEventListener("appinstalled", () => {
  installPrompt = null;
  installButton.hidden = true;

  console.log("MONA Hub installed");
});

/*
 * Service Worker 등록
 */
if ("serviceWorker" in navigator) {
  window.addEventListener("load", async () => {
    try {
      const registration = await navigator.serviceWorker.register(
        "/service-worker.js"
      );

      console.log(
        "Service Worker registered:",
        registration.scope
      );
    } catch (error) {
      console.error(
        "Service Worker registration failed:",
        error
      );
    }
  });
}