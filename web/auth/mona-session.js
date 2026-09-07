// This state is scoped to this authenticated document, never persisted across users.
export class MonaSession extends EventTarget {
  #invoke;
  #generation = 0;
  #pending = null;
  #state = Object.freeze({ status: "idle", identity: null, error: null });
  constructor(invoke) { super(); this.#invoke = invoke; }
  get state() { return this.#state; }
  #set(status, identity = null, error = null) {
    this.#state = Object.freeze({ status, identity, error });
    this.dispatchEvent(new Event("change"));
  }
  clear() { this.#generation++; this.#pending = null; this.#set("idle"); }
  resolve() {
    if (this.#pending) return this.#pending;
    if (this.#state.status === "ready") return Promise.resolve(this.#state.identity);
    const generation = this.#generation;
    this.#set("resolving");
    const pending = Promise.resolve().then(async () => {
      if (!this.#invoke) throw new Error("bridge-unavailable");
      const result = await this.#invoke("sync_acdc_identity");
      if (generation !== this.#generation) return null;
      if (!/^PER-[23456789ABCDEFGHJKMNPQRSTVWXYZ]{8}$/.test(result?.person_id)
          || typeof result.tenant_id !== "string" || !result.tenant_id.trim() || result.status !== "ACTIVE") {
        throw new Error("bridge-response-invalid");
      }
      const identity = Object.freeze({ tenant_id: result.tenant_id, person_id: result.person_id });
      this.#set("ready", identity);
      return identity;
    }).catch(error => {
      if (generation !== this.#generation) return null;
      const code = typeof error === "string" ? error : error?.message;
      // Native command returns fixed codes; never forward arbitrary error content.
      const safe = /^(identity-claims-missing|identity-invalid|tenant-mismatch|access-session-(unavailable|changed)|access-identity-unavailable|bridge-(unavailable|offline|timeout|configuration|response-invalid|http-[0-9]{3}))$/.test(code)
        ? code : "identity-resolve-failed";
      this.#set("unavailable", null, safe);
      return null;
    }).finally(() => { if (this.#pending === pending) this.#pending = null; });
    this.#pending = pending;
    return pending;
  }
}
