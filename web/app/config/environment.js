const ENVIRONMENTS = Object.freeze({
  development: Object.freeze({
    preloginUrl: "https://dev-hub.monas.co.kr/prelogin",
    appUrl: "https://dev-hub.monas.co.kr/app"
  }),
  production: Object.freeze({
    preloginUrl: "https://hub.monas.co.kr/prelogin",
    appUrl: "https://hub.monas.co.kr/app"
  })
});

function selectedEnvironment() {
  const configured = document.documentElement.dataset.environment;
  if (configured === "production" || configured === "development") {
    return configured;
  }

  const isDevelopmentHost = location.hostname === "dev-hub.monas.co.kr" ||
    location.hostname === "localhost" || location.hostname === "127.0.0.1";
  return isDevelopmentHost ? "development" : "production";
}

export const environment = selectedEnvironment();
export const authConfig = ENVIRONMENTS[environment];
