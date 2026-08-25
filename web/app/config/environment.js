const ENVIRONMENTS = Object.freeze({
  development: Object.freeze({
    preloginUrl: "https://mona-hub.pages.dev/prelogin/",
    appUrl: "https://mona-hub.pages.dev/app/"
  }),
  production: Object.freeze({
    preloginUrl: "https://mona-hub.pages.dev/prelogin/",
    appUrl: "https://mona-hub.pages.dev/app/"
  })
});

function selectedEnvironment() {
  const configured = document.documentElement.dataset.environment;
  if (configured === "production" || configured === "development") {
    return configured;
  }

  const isDevelopmentHost = location.hostname === "mona-hub.pages.dev" ||
    location.hostname === "localhost" || location.hostname === "127.0.0.1";
  return isDevelopmentHost ? "development" : "production";
}

export const environment = selectedEnvironment();
export const authConfig = ENVIRONMENTS[environment];
