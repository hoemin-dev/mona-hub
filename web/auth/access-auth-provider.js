export class AccessAuthProvider {
  #config;

  constructor(config) {
    this.#config = config;
  }

  login() {
    window.location.assign(this.#config.appUrl);
  }

  async logout() {
    const logoutUrl = new URL("/cdn-cgi/access/logout", this.#config.appUrl);
    const response = await fetch(logoutUrl, {
      credentials: "include",
      cache: "no-store",
      redirect: "follow"
    });

    if (!response.ok) {
      throw new Error(`Cloudflare Access logout failed (${response.status})`);
    }
  }

  async getIdentity() {
    const appUrl = new URL(this.#config.appUrl);
    const appPath = appUrl.pathname.endsWith("/") ? appUrl.pathname : `${appUrl.pathname}/`;
    const isProtectedPage = location.origin === appUrl.origin &&
      (location.pathname === appUrl.pathname || location.pathname.startsWith(appPath));

    if (!isProtectedPage) return null;

    try {
      const response = await fetch(new URL("/cdn-cgi/access/get-identity", appUrl), {
        credentials: "include",
        cache: "no-store",
        headers: { Accept: "application/json" },
        redirect: "manual"
      });

      if (!response.ok || response.type === "opaqueredirect") return null;

      const identity = await response.json();
      return identity?.email ? identity : null;
    } catch (error) {
      console.warn("Cloudflare Access identity 확인 실패:", error);
      return null;
    }
  }
}
