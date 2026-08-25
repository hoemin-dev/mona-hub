const mockApps = Object.freeze([
  Object.freeze({ id: "radar", name: "MonaRadar", shortLabel: "Radar", icon: "./icons/radar.svg", url: "#radar" }),
  Object.freeze({ id: "flex", name: "MonaFlex", shortLabel: "Flex", icon: "./icons/flex.svg", url: "#flex" }),
  Object.freeze({ id: "admin", name: "MonaHub Admin", shortLabel: "Admin", icon: "./icons/admin.svg", url: "#admin" })
]);

const appList = document.getElementById("appList");

function selectApp(app, button) {
  appList.querySelectorAll(".app-button").forEach(item => {
    const isActive = item === button;
    item.classList.toggle("is-active", isActive);
    item.setAttribute("aria-pressed", String(isActive));
  });
  console.info("[MonaHub] Mock app selected:", app);
}

mockApps.forEach(app => {
  const button = document.createElement("button");
  button.className = "app-button";
  button.type = "button";
  button.dataset.appId = app.id;
  button.setAttribute("aria-label", `${app.name} 선택`);
  button.setAttribute("aria-pressed", "false");
  button.title = app.name;

  const icon = document.createElement("img");
  icon.className = "app-icon";
  icon.src = app.icon;
  icon.alt = "";
  icon.setAttribute("aria-hidden", "true");

  const label = document.createElement("span");
  label.className = "app-label";
  label.textContent = app.shortLabel;

  button.append(icon, label);
  button.addEventListener("click", () => selectApp(app, button));
  appList.append(button);
});
