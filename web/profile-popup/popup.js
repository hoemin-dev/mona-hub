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
  confirmView.hidden = true;
  menuView.hidden = false;
  logoutItem.focus();
}

function showConfirmation() {
  menuView.hidden = true;
  confirmView.hidden = false;
  cancelButton.focus();
}

document.querySelectorAll("[data-placeholder]").forEach(item => {
  item.addEventListener("click", () => void invoke?.("hide_profile_popup"));
});
logoutItem.addEventListener("click", showConfirmation);
cancelButton.addEventListener("click", () => void invoke?.("hide_profile_popup"));
confirmButton.addEventListener("click", async () => {
  confirmButton.disabled = true;
  try {
    await invoke?.("confirm_access_logout");
  } catch (error) {
    confirmButton.disabled = false;
    console.error("로그아웃 확인 전달 실패:", error);
  }
});
window.addEventListener("focus", showMenu);
