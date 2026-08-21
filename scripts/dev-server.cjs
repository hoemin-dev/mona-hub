const http = require("node:http");
const fs = require("node:fs");
const path = require("node:path");

const root = path.resolve(__dirname, "..", "web", "app");
const port = Number(process.env.MONA_HUB_DEV_PORT || 1420);
const contentTypes = {
  ".css": "text/css; charset=utf-8",
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".json": "application/json; charset=utf-8",
  ".png": "image/png",
  ".svg": "image/svg+xml"
};
const assetAliases = new Map([
  ["32x32.png", path.resolve(__dirname, "..", "src-tauri", "icons", "32x32.png")],
  ["64x64.png", path.resolve(__dirname, "..", "src-tauri", "icons", "64x64.png")],
  ["manual-kit-transparent.png", path.resolve(__dirname, "..", "temp", "manual-kit-transparent.png")],
  ["pdfy-transparent.png", path.resolve(__dirname, "..", "temp", "pdfy-transparent.png")],
  ["mona-radar.png", path.resolve(__dirname, "..", "temp", "mona-radar.png")]
]);

http.createServer((request, response) => {
  const pathname = decodeURIComponent(new URL(request.url, `http://${request.headers.host}`).pathname);
  const requestedFile = pathname === "/prelogin" || pathname === "/app" || pathname === "/"
    ? "index.html"
    : pathname.replace(/^\/+/, "");
  const filePath = assetAliases.get(requestedFile) || path.resolve(root, requestedFile);

  if (!assetAliases.has(requestedFile) && filePath !== root && !filePath.startsWith(`${root}${path.sep}`)) {
    response.writeHead(403).end("Forbidden");
    return;
  }

  fs.readFile(filePath, (error, data) => {
    if (error) {
      response.writeHead(error.code === "ENOENT" ? 404 : 500).end("Not found");
      return;
    }

    response.writeHead(200, {
      "Content-Type": contentTypes[path.extname(filePath)] || "application/octet-stream",
      "Cache-Control": "no-store"
    });
    response.end(data);
  });
}).listen(port, "127.0.0.1", () => {
  console.log(`MONA-HUB web dev server: http://127.0.0.1:${port}/prelogin`);
});
