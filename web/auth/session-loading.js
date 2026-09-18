// Both login paths use the native auth state; this module never requests identity.
export function setSessionLoading(loading, label = "준비중") {
  document.documentElement.classList.toggle("session-loading", loading);
  document.querySelector("main")?.setAttribute("aria-busy", String(loading));
  const content = document.querySelector("main");
  if (content) content.inert = loading;
  const existing = document.getElementById("sessionLoading");
  if (!loading) {
    existing?.remove();
    return;
  }
  const status = existing ?? document.createElement("div");
  status.id = "sessionLoading";
  status.setAttribute("role", "status");
  status.setAttribute("aria-label", `MONA Hub ${label}`);
  status.innerHTML = `<span class="session-spinner" aria-hidden="true"></span><span aria-hidden="true">${label}</span>`;
  if (!existing) document.body.append(status);
}

function renderNativeSessionState() {
  const logoutPending = window.__monaAuthState === "logout-pending";
  setSessionLoading(
    logoutPending || (window.__monaSessionLoading ?? true) === true,
    logoutPending ? "로그아웃 진행 중" : "준비중"
  );
}

window.addEventListener("mona:session-loading", renderNativeSessionState);
renderNativeSessionState();
