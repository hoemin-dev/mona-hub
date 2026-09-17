# 로그인 준비 UI

## 추적한 기존 흐름

- 수동 로그인: `begin_access_login` → `WAITING_FOR_LOGIN` → login의 보호된 `/app/` 탐색 시작 시 창 숨김 → Finished에서 `RESOLVING_IDENTITY` → AC/DC 쿠키 초기화 및 기존 SSO → 보호된 CSS Finished를 한 번 claim → `resolve`.
- 기존 세션: AppBar native 등록 후 `try_startup` → `RESOLVING_IDENTITY` → main 프로필의 기존 쿠키로 `resolve`. 성공 시 login WebView를 만들지 않는다.
- 공통 `resolve`: 두 Access identity 병렬 조회 → 계정 일치 및 쿠키/generation 확인 → `/api/me` 한 번 → ACTIVE/PER 검증 → 쿠키/generation 재확인 → 메모리에 identity 저장 → `WAITING_FOR_MAIN` → main `/app/` 탐색.
- 웹 `MonaSession.resolve`의 `sync_acdc_identity`는 메모리만 읽으며 HTTP 호출을 추가하지 않는다.
- 기존 main Finished의 인증 완료 전환을 웹 PER 세션 준비 확인(`session_ui_ready`) 뒤로 옮겼다. 기존 로그인창 숨김은 유지하며 창 생성/종료 정책은 바꾸지 않았다.

## 구현

`RESOLVING_IDENTITY | WAITING_FOR_MAIN`을 공통 loading 상태로 사용한다. native는 상태 변경 및 main 문서 Finished 시 snapshot과 이벤트를 전달한다. JS 모듈이 늦게 로드되어도 snapshot을 읽는다. 두 HTML은 처음부터 AppBar 콘텐츠를 숨겨 탐색 중 조기 노출을 방지한다.

공통 `web/auth/session-loading.js`와 CSS는 기존 36px dock 안에 18px 민트 spinner와 “준비 중”을 표시한다. 기존 짙은 회색 배경, Segoe UI/Malgun Gothic, 민트 강조색을 사용한다. 900ms 회전, reduced-motion에서는 정지된 진행 표시, status 접근성 설명, 준비 중 main inert를 적용했다. 새 창/창 크기 변경은 없다.

main만 준비 완료를 알릴 수 있다. Access 리다이렉트 중 숨겨진 login 문서도 `/app/`를 방문하므로 그 문서는 알림/오류 복귀에서 제외한다. 완료 IPC는 HTTP를 호출하지 않는다. 실패 시 기존 `fail`을 사용하며, fast path로 login 창이 없을 때만 기존 창 생성 경로를 사용한다. 창 생성은 async IPC에서 UI 스레드로 전달한다.

인증/API 실패 시 기존 idle/fast-path fallback 전환이 로딩을 해제한다. 쿠키 공유/삭제, Access/Entra 인증, API 재시도 정책, 싱글 인스턴스 코드는 수정하지 않았다.

## 검증 및 한계 (2026-09-17)

- `npm test`: 9개 통과. 실제 JS를 사용하는 VM fixture에서 수동/자동 준비 지연, 캐시 PER 성공/실패, 로그인 WebView 제외, 로드 전 snapshot 및 native 실패 상태 반영을 확인.
- Rust release/custom-protocol lib 테스트: 17개 통과, HKCU Run을 변경하는 테스트 1개는 기존 설정대로 제외. 기존 `/api/me` 테스트는 단일 인증 GET, 302/403, 비활성/잘못된 PER, 잘못된 JSON 거부를 검증한다. 계정 불일치와 stale generation 거부도 통과.
- 브라우저의 실제 36px iframe에서 spinner/문구 배치, 숨겨진 콘텐츠, 접근성 status를 확인.
- JS 문법 검사와 diff 공백 검사를 수행. 기존 appbar unused 경고 2개 유지.
- 실계정 Entra 로그인 및 실제 WebView2 전체 E2E는 수행하지 않았다. 위 성공/실패 UI 테스트는 fixture이며 외부 인증 완료를 증명하지 않는다.

현재 main/login은 원격 `mona-hub.pages.dev` 문서를 읽는다. 따라서 이 변경은 **웹 자산과 native 실행 파일을 함께 적용**해야 한다. 새 `session_ui_ready` IPC에 의존하므로 이전 웹 자산/이전 바이너리와 혼합 배포하지 않는다. 이 작업에서는 운영 배포/설치/커밋을 수행하지 않았다.
