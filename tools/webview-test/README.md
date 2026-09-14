# WebView Test

Tauri WebView/popup 호환성을 확인하는 독립 개발 도구입니다. Production MonaHub에 포함되지 않습니다.

루트에서 의존성 설치(`npm install`) 및 Tauri 개발 환경 준비 후:

```sh
npm run test:webview
```

`tauri dev`가 Node 기본 HTTP 서버도 자동 실행합니다. 종료는 Ctrl+C입니다.
8088 포트가 사용 중이면 기존 서버를 종료하세요. 설치파일은 만들지 않습니다.
기존 HTTP origin/opener/postMessage 및 포트 거부 정책을 보존하기 위해
`127.0.0.1:8088`을 유지하며, 별도 Python 서버나 플러그인은 필요 없습니다.

루트와 child에서 popup/named 재사용/nested/target=_blank, 양방향 메시지 ACK,
same/cross-origin DOM 접근, about:blank, navigation/302 redirect 허용·거부,
print, 파일 선택/drop, 일반·Blob 다운로드, window.close/X를 확인합니다.
example.com은 메시지 ACK를 제공하지 않습니다. 종료는 Rust Destroyed/registry 로그와 대조하세요.
Windows에서는 기존 WebView2 WindowCloseRequested 어댑터를 유지합니다.
화면 PASS는 개별 관찰 결과이며 전체 native 동작 성공을 의미하지 않습니다.
