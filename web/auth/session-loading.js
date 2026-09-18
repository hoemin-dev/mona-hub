// Both login paths use the native auth state; this module never requests identity.
export function setSessionLoading(loading) {
  document.documentElement.classList.toggle("session-loading", loading);
  document.querySelector("main")?.setAttribute("aria-busy", String(loading));
  const content = document.querySelector("main");
  if (content) content.inert = loading;
  const existing = document.getElementById("sessionLoading");
  if (!loading) {
    existing?.remove();
    return;
  }
  if (existing) return;
  const status = document.createElement("div");
  status.id = "sessionLoading";
  status.setAttribute("role", "status");
  status.setAttribute("aria-label", "MONA Hub 준비 중");
  status.innerHTML = '<span class="session-spinner" aria-hidden="true"></span><span aria-hidden="true">준비중</span>';
  document.body.append(status);
}

window.addEventListener("mona:session-loading", () => {
  setSessionLoading(window.__monaSessionLoading === true);
});
setSessionLoading(window.__monaSessionLoading ?? true);
