// Fixed fixture allowlist: no filesystem paths supplied by requests are opened.
const http = require("node:http");
const fs = require("node:fs");
const path = require("node:path");
const files = new Map([
  ["/", ["index.html", "text/html"]],
  ["/index.html", ["index.html", "text/html"]],
  ["/popup.html", ["popup.html", "text/html"]],
  ["/app.js", ["app.js", "text/javascript"]],
  ["/style.css", ["style.css", "text/css"]],
  ["/sample.txt", ["sample.txt", "text/plain"]]
]);
const server = http.createServer((request, response) => {
  const pathname = new URL(request.url, "http://127.0.0.1:8088").pathname;
  if (pathname === "/redirect" || pathname === "/redirect-denied") {
    response.writeHead(302, { Location: pathname === "/redirect" ? "/popup.html?redirected=1" : "http://127.0.0.1:8089/" }).end();
    return;
  }
  const fixture = files.get(pathname);
  if (!fixture) { response.writeHead(404).end(); return; }
  fs.readFile(path.join(__dirname, "web", fixture[0]), (error, data) => {
    if (error) { response.writeHead(500).end(); return; }
    response.writeHead(200, { "Content-Type": `${fixture[1]}; charset=utf-8`, "Cache-Control": "no-store" });
    response.end(data);
  });
});
server.on("error", error => { console.error(`WebView test server: ${error.message}`); process.exit(1); });
server.listen(8088, "127.0.0.1", () => console.log("WebView test fixtures: http://127.0.0.1:8088/"));
