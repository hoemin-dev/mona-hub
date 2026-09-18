const blockedControlKeys = new Set(["p", "r", "s"]);

function blockContextMenu(event) {
  event.preventDefault();
  event.stopImmediatePropagation();
}

function blockBrowserShortcut(event) {
  const key = event.key.toLowerCase();
  const isRefreshKey = key === "f5";
  const isBlockedControlShortcut = event.ctrlKey && blockedControlKeys.has(key);

  if (!isRefreshKey && !isBlockedControlShortcut) return;

  event.preventDefault();
  event.stopImmediatePropagation();
}

// The login WebView can briefly visit /app/ during an Access redirect, so the
// URL alone cannot identify the AppBar. Guard on the native WebView label.
const currentWindowLabel = window.__TAURI__?.window?.getCurrentWindow?.().label;
if (currentWindowLabel === "main") {
  document.addEventListener("contextmenu", blockContextMenu, { capture: true });
  document.addEventListener("keydown", blockBrowserShortcut, { capture: true });
}
