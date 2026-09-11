# 기존 Access 세션 fast path

## 구현 전 확인 (2026-09-11)

- main, login 모두 별도 data directory, incognito/profile 옵션을 설정하지 않는다. 설치된 Tauri 2.11.5 `manager/webview.rs:534`는 둘 다 `LocalData/<app identifier>`를 기본값으로 사용한다.
- 실제 실행에서 main과 login의 MonaHub 및 AC/DC `CF_Authorization` 존재 여부를 각각 확인했고 값이 각각 같았다. 쿠키 값은 출력/파일로 저장하지 않았다. 관측된 두 쿠키의 HttpOnly 플래그는 false였지만 구현은 DOM 쿠키 접근에 의존하지 않고 native cookie API를 계속 사용한다.
- main에서 읽은 AC/DC 쿠키를 `Cookie: CF_Authorization=...`로 해당 AC/DC 서버에만 보내면 `/api/me`가 **200 / 271.3ms**, 쿠키를 생략하면 **302 / 43.4ms**였다. reqwest가 WebView 쿠키를 자동 공유하는 것은 아니므로 명시적 Cookie 헤더가 필수다.
- AC/DC 서버 코드의 `/api/me`는 인증된 현재 사용자를 PER로 resolve하는 GET이다. `/api/admin/bootstrap`은 관리자 기능이므로 MonaHub startup API로 바꾸지 않는다.
- WebView2 UDF에 쿠키와 세션이 저장되는 구조는 [Microsoft 문서](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/user-data-folder)와 일치한다. 무세션 테스트는 `WEBVIEW2_USER_DATA_FOLDER`를 별도 빈 디렉터리로 지정하여 기존 프로필을 보존한다.

따라서 login WebView는 이미 저장된 세션을 읽는 데 필수가 아니다. 새 Access 앱 세션을 발급받거나 인터랙티브 로그인/로그아웃이 필요할 때만 필요하다.

## SSO 3초대 지연의 구체적 경로

기존 `identity_session::start`는 계정 전환 시 이전 AC/DC 계정 재사용을 막으려고 AC/DC 앱 쿠키를 삭제하고 보호된 CSS로 이동한다. 메모리에서만 쿠키를 사용하고 실제 프로필을 변경하지 않는 1회 HTTP 추적으로 아래를 확인했다. 리다이렉트 query, 인증 코드, 응답 쿠키는 기록하지 않았다.

| 요청 | 응답 | 시간 |
|---|---:|---:|
| AC/DC `/app/css/style.css` (앱 쿠키 없이) | 302 | 180.9ms |
| Access 팀 도메인 `/cdn-cgi/access/login/mona-acdc.pages.dev` | 302 | **2,740.6ms** |
| AC/DC `/cdn-cgi/access/authorized` | 302 | 107.9ms |
| AC/DC `/app/css/style.css` (재발급 쿠키) | 200 | 31.2ms |

총 3,195.3ms. 살아 있는 Access 글로벌 SSO로 처리되어 이번 추적에는 MS 로그인 페이지 요청이 없었다. Access 서버 내부에서 소요된 이유까지는 클라이언트 타이밍만으로 분해할 수 없다.

## 최소 변경

1. AppBar 등록·표시 이후 `try_startup`이 비동기로 main의 두 앱 쿠키를 읽는다.
2. 기존 두 Access identity 조회를 병렬 실행하고 Entra tenant/object ID 일치를 검증한다.
3. 기존 `/api/me`를 AC/DC 쿠키로 한 번 호출하여 유효한 ACTIVE/PER 응답을 검증한다. 요청 전후 쿠키와 generation도 확인한다.
4. 성공하면 기존 메모리 identity 저장과 main `/app/` 탐색을 재사용한다. login WebView 생성과 AC/DC SSO 탐색은 없다.
5. 쿠키 없음, 만료, HTTP 실패, 계정 불일치, 세션 변경은 새 generation으로 기존 login/SSO 경로에 진입한다. fallback에서의 쿠키 초기화와 계정 보호는 그대로다.

빠른 확인의 네트워크 timeout은 요청별 5초다. 두 병렬 Access 조회 다음 `/api/me`가 있어 네트워크 대기는 최악 약 10초 후 fallback한다 (native cookie/UI dispatch 시간 제외). 기존 로그인 경로는 요청별 10초를 유지한다. 실패를 성공으로 처리하거나 PER를 디스크에서 신뢰하는 캐시는 추가하지 않았다.

login 창이 없는 fast-path 성공 상태에서는 비동기 로그아웃 명령이 UI 스레드에 창 생성을 전달하고 첫 URL을 기존 Cloudflare logout URL로 지정한다. 그 다음 Entra/logout-complete 흐름은 그대로다. 트레이에서도 이 비동기 명령을 실행한다. sync IPC에서는 main-thread 전달만 해도 재진입하여 멈출 수 있었으므로 명령 자체가 async여야 한다. 계정 전환은 기존 로그아웃/새 로그인 경로를 유지한다.

싱글 인스턴스 코드와 기존 startup 계측은 변경하지 않았다. 추가 이벤트: `fast-path.begin`, `fast-path.hit`, `fast-path.miss`, `fast-path.fallback`, `logout.lazy-login.build`.

## 검증

전용 출력 폴더를 사용한다. 사용자 target 정리 작업과 분리하기 위해 공용 `src-tauri/target`에서 시도한 테스트는 중단했다.

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --target-dir tmp/fastpath-target --release --features tauri/custom-protocol --lib --offline
cargo build --manifest-path src-tauri/Cargo.toml --target-dir tmp/fastpath-target --release --features tauri/custom-protocol --offline
```

Rust 테스트에는 서로 다른 tenant/계정 거부, 로그아웃/취소/fallback 후 stale 결과 거부를 추가했다. 기존 HTTP 테스트는 쿠키 헤더 포함, 302/403/잘못된 JSON/비활성 PER 거부를 검증한다.

## 실제 A/B 결과

AppBar/세션 확인은 release + custom-protocol 빌드로 측정했다. A/B 프로필은 분리했고 OS 재부팅/동일 네트워크 지연을 통제한 벤치마크는 아니다. startup 시간은 단조 증가 native trace를 기준으로 한다.

| 항목 | 수정 전 재측정 | A: 기존 세션, 최종 빌드 | B: 빈 프로필 |
|---|---:|---:|---:|
| AppBar 표시 (누적) | 기존 로그 참조 | 3,211.2ms | 2,697.8ms |
| fast path 시작 → 결과 | 없음 | 3,656.1ms / hit | 3,767.4ms / miss |
| Access identity 병렬 요청 | 기존 흐름 | 1,913.0ms | 0회 |
| `/api/me` | 기존 흐름 | 302.1ms / 200 / 1회 | 0회 |
| startup login WebView 생성 | 있음 | **0회** | 1회 / 263.5ms |
| AC/DC SSO 탐색 | 있음 | **0회** | 0회 (로그인 화면으로 fallback) |
| 사용자/App 목록 렌더 (누적) | **8,974.9ms** | **7,149.3ms** | 인증되지 않음 |

이전 조사 기준값은 9,827.9ms였다. 이번 수정 전 재측정은 8,974.9ms였고, 성공한 A 실행들은 5,606.8ms 및 최종 빌드 7,149.3ms였다. 네트워크/초기화 변동이 있어 단일 고정 개선율로 일반화하지 않는다. 생략 대상인 login 생성과 SSO가 0회라는 점은 로그에서 직접 확인됐다.

첫 시도에서는 요청별 2초 제한으로 Access 확인이 실패했고, fallback을 통해 8,900.8ms에 정상 인증됐다. 그래서 최종 제한을 5초로 늘리고 timeout 여부도 고정된 진단 필드로 기록한다. 이 실패 실행을 fast-path 성공 수치로 포함하지 않았다.

B에서는 native 쿠키 조회/초기 UI 대기를 거친 후 6,466.0ms에 세션 부재가 확인됐다. 네트워크 요청 없이 miss → 기존 login 생성 → Access 로그인 필요 감지 → 로컬 로그인 화면이 이어졌다. 빈 프로필의 로그인 문서 Finished는 14,565.5ms였고, 별도 DOM 검사로 로그인 버튼 존재·활성화·화면 visible을 확인했다. `begin_access_login` 후 취소 명령도 성공하여 PRELOGIN으로 돌아왔다. B를 즉시 실패하거나 전체 시작이 빨라졌다고 해석하지 않는다.

### 로그아웃 회귀 검증

최종 A 상태에서는 login 창이 없다. 실제 native `begin_access_logout`를 호출하고 브라우저의 외부 Cloudflare/Entra logout 응답만 fixture로 대체했다. 외부 세션 무효화 서비스는 호출하지 않았다.

- IPC 즉시 성공 반환.
- 요청 시 login WebView 생성: 248.1ms.
- Cloudflare logout → Entra logout → logout-complete → PRELOGIN.
- main `/prelogin/`와 login `/login/` 복귀 및 활성 로그인 버튼 확인.
- Cloudflare, Entra 응답 fixture 각각 1회 사용.

실제 MS 신규 인증 완료 및 서로 다른 실계정으로의 전환은 수행하지 않았다. 해당 외부 동작은 기존 흐름을 유지하며 계정 일치/세션 취소 경계는 Rust 테스트로 검증했다.

계측 파일:

- `tmp/fastpath-baseline.log`: 변경 전 8.975초.
- `tmp/fastpath-A.log`: 초기 2초 timeout으로 fallback한 실행.
- `tmp/fastpath-A2.log`: 5.607초 fast-path 성공; 이후 sync logout 재진입 문제를 재현한 로그.
- `tmp/fastpath-A-final.log`: 최종 7.149초 fast-path 성공 및 async logout fixture 통과. startup 이후 logout 때 생긴 `login.build`와 startup을 구분해야 한다.
- `tmp/fastpath-B.log`: 별도 빈 프로필, 로그인 진입/취소 확인.

최종 검증: Rust 20개 통과, JS 3개 통과, `npm run check` 및 `git diff --check` 통과, 배포용 release 빌드 성공. 기존 appbar unused 경고 2개는 유지했다. 검증 앱과 임시 디버깅 포트는 종료했다. 테스트 실행 파일은 `tmp/fastpath-target/release/app.exe`이며 설치/배포나 Git commit은 수행하지 않았다.
