const launcherStatus = document.getElementById("launcherStatus");
const appButtons = document.querySelectorAll(".app-button[data-app-url]");

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
  if (launcherStatus) {
    launcherStatus.textContent = message;
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

appButtons.forEach(button => {
  button.addEventListener("click", () => openApp(button));
});

clearLegacyWebData();
