const mockApps = Object.freeze([
  Object.freeze({ id: "radar", name: "MonaRadar", shortLabel: "Radar", icon: "./icons/radar.svg", url: "#radar" }),
  Object.freeze({ id: "flex", name: "MonaFlex", shortLabel: "Flex", icon: "./icons/flex.svg", url: "#flex" }),
  Object.freeze({ id: "admin", name: "MonaHub Admin", shortLabel: "Admin", icon: "./icons/admin.svg", url: "#admin" })
]);

const appList = document.getElementById("appList");
const profileButton = document.getElementById("profileButton");
const invoke = window.__TAURI__?.core?.invoke;
const authController = new AuthController(
  new AccessAuthProvider({ appUrl: authConfig.appUrl }),
  {
    preloginUrl: authConfig.preloginUrl,
    logoutNavigator: async () => {
      if (!invoke) throw new Error("Tauri logout bridge is unavailable");
      await invoke("begin_access_logout");
    }
  }
);

async function toggleProfileMenu() {
  console.info("[profile-popup] app profile clicked");
  if (!invoke) {
    console.error("[profile-popup] invoke unavailable", {
      href: window.location.href,
      hasTauriGlobal: Boolean(window.__TAURI__)
    });
    return;
  }
  try {
    console.info("[profile-popup] invoke requested", {
      command: "toggle_profile_popup",
      href: window.location.href
    });
    const open = await invoke("toggle_profile_popup");
    console.info("[profile-popup] invoke completed", { open });
    profileButton.setAttribute("aria-expanded", String(open));
  } catch (error) {
    console.error("[profile-popup] invoke failed", error);
  }
}

window.addEventListener("mona:logout-confirmed", async () => {
  profileButton.setAttribute("aria-expanded", "false");
  try {
    await authController.logout();
  } catch (error) {
    console.error("Cloudflare Access 로그아웃 시작 실패:", error);
  }
});

if (!profileButton) {
  console.error("[profile-popup] #profileButton not found; listener not registered");
} else {
  profileButton.addEventListener("click", toggleProfileMenu);
  console.info("[profile-popup] click listener registered", {
    pointerEvents: getComputedStyle(profileButton).pointerEvents,
    rect: profileButton.getBoundingClientRect().toJSON()
  });
}

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
import { AccessAuthProvider } from "../auth/access-auth-provider.js";
import { AuthController } from "../auth/auth-controller.js";
import { authConfig } from "./config/environment.js";
