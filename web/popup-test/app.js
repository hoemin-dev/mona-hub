"use strict";

const LOCAL = "http://127.0.0.1:8088";
const CROSS = "https://example.com/";
const CHANNEL = "mona-popup-test-v1";
const isPopup = document.body.dataset.popup === "true";
const children = [];
const pending = new Map();
let sequence = 0;
let signalSequence = 0;
const $ = id => document.getElementById(id);

function safeUrl(value) {
  try {
    const url = new URL(value, location.href);
    if (url.href === "about:blank") return url.href;
    if (!["http:", "https:"].includes(url.protocol)) return `${url.protocol}[redacted]`;
    return `${url.origin}${["/", "/popup.html", "/index.html"].includes(url.pathname) ? url.pathname : "/[path-redacted]"} (query=${!!url.search}, fragment=${!!url.hash})`;
  } catch { return "[invalid URL]"; }
}

function log(kind, event, detail = "") {
  const line = `${new Date().toISOString()} [${kind}] ${event} ${detail}`;
  $("log").textContent += `${line}\n`;
  const lines = $("log").textContent.split("\n");
  if (lines.length > 400) $("log").textContent = lines.slice(-400).join("\n");
  $("log").scrollTop = $("log").scrollHeight;
  $("status").textContent = `[${kind}] ${event} ${detail}`;
  console.info(line);
}

// Fixed diagnostic vocabulary only; Rust observes document title, no Tauri IPC.
// Title events may coalesce: screen log is the detailed source of truth.
function signal(event) { document.title = `popup-test:${event}:${++signalSequence}`; }
function button(container, label, action) {
  const element = document.createElement("button");
  element.type = "button";
  element.textContent = label;
  element.addEventListener("click", () => {
    try { action(); } catch (error) { log("FAIL", label, error.name); }
  });
  $(container).append(element);
}

function openPopup(url, name, features) {
  log("EVENT", "popup request", `url=${safeUrl(url)} target=${name} nested=${isPopup}`);
  if (isPopup) signal("nested-popup");
  const ref = features === undefined ? window.open(url, name) : window.open(url, name, features);
  log(ref ? "PASS" : "FAIL", "popup 반환값", `exists=${!!ref}; 창 생성 완료 여부는 별도 확인`);
  if (!ref) return;
  const id = String(++sequence);
  const entry = { id, ref, origin: url === "about:blank" ? LOCAL : new URL(url).origin, closed: false };
  children.push(entry);
  if (children.length === 1) $("children").replaceChildren();
  const option = document.createElement("option");
  option.value = id;
  option.textContent = `#${id} ${name} ${entry.origin}`;
  $("children").append(option);
  $("children").value = id;
  setTimeout(() => inspect(entry), 1000);
}

function selected() {
  const entry = children.find(item => item.id === $("children").value);
  if (!entry) throw new Error("NoPopupSelected");
  return entry;
}
function inspect(entry) {
  try {
    log("EVENT", "popup.closed 관찰", `#${entry.id} closed=${entry.ref.closed}; native 창 Destroyed와 대조 필요`);
    try {
      log("EVENT", "same-origin DOM 접근", `#${entry.id} accessible=true url=${safeUrl(entry.ref.location.href)}`);
    } catch (error) {
      log(error.name === "SecurityError" && entry.origin !== LOCAL ? "PASS" : "EVENT",
        "cross-origin DOM 접근 차단", `#${entry.id} ${error.name}`);
    }
  } catch (error) { log("FAIL", "popup 참조 관찰", error.name); }
}

function send(ref, origin, type) {
  const nonce = `test-${++sequence}`;
  ref.postMessage({ channel: CHANNEL, type, nonce }, origin);
  log("WAIT", "postMessage 전송", `type=${type}; 수신 ACK 전까지 성공 아님`);
  const timer = setTimeout(() => {
    pending.delete(nonce);
    log("FAIL", "postMessage ACK timeout", "외부 example.com은 ACK를 구현하지 않음");
  }, 3000);
  pending.set(nonce, timer);
}

button("popup-controls", 'window.open(url, "_blank")', () => openPopup(`${LOCAL}/popup.html`, "_blank"));
button("popup-controls", 'named-popup · 500 × 400', () => openPopup(`${LOCAL}/popup.html`, "named-popup", "width=500,height=400"));
button("popup-controls", "Same-origin popup", () => openPopup(`${LOCAL}/popup.html`, "_blank"));
button("popup-controls", "Cross-origin · example.com", () => openPopup(CROSS, "_blank"));
button("popup-controls", "about:blank", () => openPopup("about:blank", "_blank"));
button("popup-controls", "거부 정책: 다른 loopback port", () => openPopup("http://127.0.0.1:8089/", "_blank"));
button("message-controls", "opener 존재 확인", () => log("EVENT", "window.opener", `exists=${!!window.opener}`));
button("message-controls", "opener → 선택 popup postMessage", () => {
  const entry = selected();
  send(entry.ref, entry.origin, "ping");
});
button("message-controls", "popup → opener postMessage", () => {
  if (!window.opener) { log("FAIL", "popup → opener", "opener 없음 (target=_blank 기본 noopener는 정상)"); return; }
  send(window.opener, LOCAL, "ping");
});
button("message-controls", "선택 popup 상태 / DOM 접근 확인", () => inspect(selected()));
button("message-controls", "이 창에서 window.close()", () => {
  log("WAIT", "popup close requested", "window.close 호출 예정; Tauri 최상위 창과 registry 로그를 확인");
  signal("close-requested");
  if (window.opener) window.opener.postMessage({ channel: CHANNEL, type: "close-requested" }, LOCAL);
  // Let the diagnostic title reach the host before exercising the unmodified browser API.
  setTimeout(() => {
    window.close();
    setTimeout(() => log("FAIL", "window.close 이후 JS가 계속 실행됨", "native 창 상태도 확인"), 1000);
  }, 150);
});

window.addEventListener("message", event => {
  const data = event.data;
  const knownSource = (window.opener && event.source === window.opener)
    || children.some(child => child.ref === event.source);
  if (event.origin !== LOCAL || !knownSource || !data || data.channel !== CHANNEL
      || !["ping", "ack", "ready", "close-requested", "pagehide"].includes(data.type)) {
    log("EVENT", "postMessage 무시", "origin/source/schema 불일치; payload 미기록"); return;
  }
  log("PASS", "postMessage 수신", `type=${data.type} origin=${event.origin}`);
  signal("postMessage");
  if (data.type === "ping" && typeof data.nonce === "string" && /^test-\d+$/.test(data.nonce)) {
    event.source.postMessage({ channel: CHANNEL, type: "ack", nonce: data.nonce }, event.origin);
  }
  if (data.type === "ack" && pending.has(data.nonce)) {
    clearTimeout(pending.get(data.nonce)); pending.delete(data.nonce);
    log("PASS", "postMessage 왕복 ACK");
  }
});
window.addEventListener("pagehide", () => {
  signal("pagehide");
  if (window.opener) window.opener.postMessage({ channel: CHANNEL, type: "pagehide" }, LOCAL);
  // pagehide can mean navigation, NOT proof of native WebView/window destruction.
});
setInterval(() => children.forEach(entry => {
  try {
    if (entry.ref.closed && !entry.closed) {
      entry.closed = true;
      log("EVENT", "popup.closed=true", `#${entry.id}; Tauri Destroyed/registry와 대조`);
    }
  } catch { /* Some runtimes sever cross-origin references. Manual inspect is available. */ }
}), 500);

$("blank-link").addEventListener("click", () => log("EVENT", "target=_blank 클릭", "기본 noopener; 반환값을 얻을 JS 호출 없음"));
$("file-input").addEventListener("change", event => {
  log("PASS", "file input change", `count=${event.target.files.length}; 이름/내용 미기록`);
  signal("file-input");
});
$("drag-source").addEventListener("dragstart", event => {
  event.dataTransfer.setData("text/plain", "popup-test-drag-fixture");
  log("EVENT", "HTML5 dragstart");
});
$("drop-zone").addEventListener("dragover", event => { event.preventDefault(); $("drop-zone").classList.add("over"); });
$("drop-zone").addEventListener("dragleave", () => $("drop-zone").classList.remove("over"));
$("drop-zone").addEventListener("drop", event => {
  event.preventDefault(); $("drop-zone").classList.remove("over");
  log("PASS", "HTML5 drop", `files=${event.dataTransfer.files.length}; textFixture=${event.dataTransfer.getData("text/plain") === "popup-test-drag-fixture"}`);
  signal("drag-drop");
});
// Do not navigate away when a file is dropped outside the test zone.
window.addEventListener("dragover", event => event.preventDefault());
window.addEventListener("drop", event => event.preventDefault());
$("download").addEventListener("click", () => {
  log("WAIT", "일반 파일 download 요청", "저장된 파일 / Rust download finished를 확인"); signal("download");
});
$("blob-download").addEventListener("click", () => {
  const url = URL.createObjectURL(new Blob(["MonaHub popup-test blob fixture\n"], { type: "text/plain" }));
  const anchor = document.createElement("a");
  anchor.href = url; anchor.download = "popup-test-blob.txt";
  document.body.append(anchor); anchor.click(); anchor.remove();
  setTimeout(() => URL.revokeObjectURL(url), 30000);
  log("WAIT", "blob download 요청", "저장된 파일 / Rust download finished를 확인"); signal("download");
});
$("clear").addEventListener("click", () => { $("log").textContent = ""; });
$("environment").textContent = `URL: ${safeUrl(location.href)}\norigin: ${location.origin}\nuserAgent: ${navigator.userAgent}\nrole: ${isPopup ? "popup / nested parent" : "root"}\nopener: ${!!window.opener}\nNative IPC: 사용하지 않음`;
log("EVENT", "ready", `opener=${!!window.opener}; native WebView 종료 이벤트는 공개 Tauri API에서 직접 관찰 불가`);
signal("ready");
if (window.opener) window.opener.postMessage({ channel: CHANNEL, type: "ready" }, LOCAL);
