(() => {
  // Apply only to Microsoft's top-level authentication document in this window.
  if (window.top !== window.self || location.protocol !== "https:") return;
  const host = location.hostname;
  if (host !== "login.microsoftonline.com" && !host.endsWith(".microsoftonline.com")) return;

  const apply = () => {
    const style = document.createElement("style");
    style.textContent = `
      html, body { overflow-x: hidden !important; }
    `;
    (document.head || document.documentElement).appendChild(style);
  };

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", apply, { once: true });
  } else {
    apply();
  }
})();
