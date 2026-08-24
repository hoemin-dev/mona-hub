export class AuthController extends EventTarget {
  #provider;
  #preloginUrl;
  #authenticated = false;
  #identity = null;
  #checking = null;

  constructor(provider, { preloginUrl }) {
    super();
    this.#provider = provider;
    this.#preloginUrl = preloginUrl;
  }

  login() {
    this.#provider.login();
  }

  async logout() {
    await this.#provider.logout();
    this.#setState(false, null);
    window.location.replace(this.#preloginUrl);
  }

  isAuthenticated() {
    return this.#authenticated;
  }

  getIdentity() {
    return this.#identity;
  }

  async refresh() {
    if (this.#checking) return this.#checking;

    this.#checking = this.#provider.getIdentity()
      .then(identity => {
        this.#setState(Boolean(identity), identity);
        return this.#authenticated;
      })
      .finally(() => {
        this.#checking = null;
      });

    return this.#checking;
  }

  async requireSession() {
    if (await this.refresh()) return true;

    this.#setState(false, null);
    const prelogin = new URL(this.#preloginUrl);
    if (location.pathname === prelogin.pathname) return false;
    if (location.origin !== prelogin.origin || location.pathname !== prelogin.pathname) {
      window.location.replace(prelogin);
    }
    return false;
  }

  #setState(authenticated, identity) {
    const changed = this.#authenticated !== authenticated || this.#identity !== identity;
    this.#authenticated = authenticated;
    this.#identity = identity;
    if (changed) this.dispatchEvent(new Event("change"));
  }
}
