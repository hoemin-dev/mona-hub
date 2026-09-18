const invoke = window.__TAURI__?.core?.invoke;
const menuView = document.getElementById("menuView");
const confirmView = document.getElementById("confirmView");
const logoutItem = document.getElementById("logoutItem");
const cancelButton = document.getElementById("cancelButton");
const confirmButton = document.getElementById("confirmButton");

console.info("[profile-popup] popup.js loaded", {
  href: window.location.href,
  hasInvoke: Boolean(invoke),
  menuItems: document.querySelectorAll('[role="menuitem"]').length
});

window.addEventListener("error", event => {
  console.error("[profile-popup] page error", event.error ?? event.message);
});
window.addEventListener("unhandledrejection", event => {
  console.error("[profile-popup] unhandled rejection", event.reason);
});

function showMenu() {
  confirmButton.disabled = false;
  confirmView.hidden = true;
  menuView.hidden = false;
  menuView.focus({ preventScroll: true });
}

function showConfirmation() {
  menuView.hidden = true;
  confirmView.hidden = false;
  cancelButton.focus();
}

async function hidePopup() {
  // Change the retained WebView surface before asking native code to hide it.
  // Calling this only after invoke resolves is too late: WebView2 can preserve
  // the confirmation frame while the native window is already hidden.
  showMenu();
  try {
    if (invoke) {
      await invoke("hide_profile_popup");
    }
  } catch (error) {
    console.error("프로필 메뉴 닫기 실패:", error);
  } finally {
    // Keep the state correct even if native hiding fails or another event ran.
    showMenu();
  }
}

document.querySelectorAll("[data-placeholder]").forEach(item => {
  item.addEventListener("click", () => void hidePopup());
});
logoutItem.addEventListener("click", showConfirmation);
cancelButton.addEventListener("click", () => void hidePopup());
confirmButton.addEventListener("click", async () => {
  confirmButton.disabled = true;
  try {
    if (invoke) {
      // Reset synchronously before native code hides the reused window. This
      // prevents its last confirmation frame from flashing on the next show.
      showMenu();
      await invoke("confirm_access_logout");
      return;
    }

    window.location.assign(new URL("/cdn-cgi/access/logout", window.location.origin));
  } catch (error) {
    confirmButton.disabled = false;
    console.error("로그아웃 확인 전달 실패:", error);
  }
});
// Clicking outside the popup makes the native window hide on blur. Reset the
// DOM in the same event turn, before WebView2 retains the surface for reuse.
window.addEventListener("blur", showMenu);
window.addEventListener("focus", showMenu);
