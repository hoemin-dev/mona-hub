# 기존 세션 자동 로그인 시작 경로 조사

비교 기준: `f68437b` (single-instance 도입 직전) → `57fa060`.

## 코드에서 확인한 사항

- Windows single-instance 2.4.4는 Tauri plugin setup에서 `CreateMutexW`와 메시지 수신 창을 생성한다. 첫 프로세스에 대기 루프는 없다. 기존 인스턴스가 있을 때만 두 번째 프로세스가 `SendMessageW`로 알리고 종료한다.
- `restore_main_window` 호출 지점은 두 번째 인스턴스 콜백과 트레이 동작이다. 첫 실행 setup에서는 호출하지 않는다.
- 새 `main_activation_ready`는 atomic 상태 확인 후 즉시 반환한다. await/sleep/polling을 추가하지 않았다.
- identity의 60초 sleep은 변경 전부터 있던 별도 스레드의 실패 타임아웃이다. 성공 경로가 이를 기다리지 않는다.
- main은 config로 생성되고, setup에서 hidden profile-popup을 생성한 후 AppBar를 등록·표시한다. 이후 로그인 WebView를 생성한다. API가 AppBar 표시를 막지 않는다. 단, popup 생성은 AppBar 표시 전에 동기 실행된다.
- 기존 세션 경로: login의 MonaHub `/app/` Finished → 기존 AC/DC 앱 쿠키 제거 및 보호된 CSS로 SSO 탐색 → MonaHub/ACDC Access identity 확인 → 계정 일치 검증 → AC/DC `/api/me` → main `/app/` → native identity 읽기 및 앱 목록 표시.
- `57fa060`에서 두 Access identity 조회는 직렬에서 병렬로 바뀌었다. `/api/me`는 계정 일치 검증 후 한 번 실행한다. 계정 검증 순서를 성능 목적으로 우회하지 않았다.
- login과 main의 `/app/` 두 번 로드는 서로 다른 WebView에서 기존부터 발생한다. native `/api/me`는 generation claim으로 중복 Finished 이벤트를 방어한다. 프런트엔드 `MonaSession`은 native 결과를 읽으며 HTTP를 추가하지 않는다.
- Cargo.lock diff에 기존 패키지 버전 교체는 없고 single-instance 및 의존성이 추가됐다.

## 기존 로그에서 확인한 측정

2026-09-11 16:05 실행 (로그 시간은 UTC 07:05):

| 구간 | 측정 |
|---|---:|
| AppBar 표시 | 16:05:39 |
| login WebView 생성 | 1,753.4ms |
| login 중앙 배치 | 1.1ms |
| 두 Access identity 병렬 조회 | 429.7ms |
| main 인증 완료 | 16:05:43 |

기존 로그는 초 단위이며 시작 부분과 AC/DC SSO 이벤트가 빠져 있다. 이 자료만으로 전체 지연 원인이나 변경 전 대비 회귀를 확정할 수 없다.

## 추가 계측

`MONAHUB_STARTUP_TRACE`에 절대 파일 경로를 지정하고 release 실행 파일을 실행한다. 계측 파일에는 프로세스 시작부터의 단조 증가 밀리초, PID, 스레드 ID가 기록된다. 기본 실행에서는 파일을 열지 않는다. URL query/fragment, 쿠키, identity 응답 내용은 기록하지 않는다.

배포와 같은 빌드 조건은 `cargo build --manifest-path src-tauri/Cargo.toml --release --features tauri/custom-protocol --offline`이다. `--release`만 지정하면 로컬 WebView가 개발 URL을 사용할 수 있다. 첫 진단 실행 `tmp/startup-current-1.log`는 이 조건이 달라 배포판 전체 시간 비교에서 제외했다.

single-instance 바로 앞/뒤 plugin setup을 경계로 측정한다. plugin 종료부터 app setup 진입까지는 config main 생성과 Tauri 부대 작업을 포함하므로 순수 WebView 생성 시간이라고 해석하면 안 된다.

main 렌더 완료는 실제 페이지의 identity ready 상태와 앱 버튼/profile DOM을 확인하고 두 animation frame 후 native로 알리는 진단값이다. 화면 픽셀의 표시 완료를 직접 측정한 값은 아니다. 원격 웹 배포는 필요 없다.

## 배포 설정 실측 결과

동일 사용자 WebView 프로필의 살아 있는 세션을 사용했다. 로그아웃이나 새 로그인은 하지 않았다. OS 재부팅/캐시 초기화를 한 cold-start 벤치마크는 아니며, 기존 앱 프로세스가 없는 상태에서 실행했다.

| 구간 | 배포 실행 1 (ms) | 배포 실행 2 (ms) |
|---|---:|---:|
| run 진입 → single setup 시작 | 240.4 | 234.7 |
| single-instance setup / 락·메시지 창 등록 | 2.0 | 2.2 |
| single setup 종료 → main 생성 완료 (Tauri 부대 작업 포함) | 1,906.0 | 1,343.9 |
| hidden profile-popup 생성 | 218.9 | 356.1 |
| AppBar 등록·표시 | 57.8 | 120.4 |
| AppBar 표시 시점 (누적) | 2,426.6 | 2,058.8 |
| login WebView 생성 | 1,904.2 | 2,685.5 |
| 기존 MonaHub 세션 확인 시작 → 확인 완료 (login 생성 포함) | 2,073.5 | 2,905.2 |
| AC/DC 쿠키 초기화 | 9.8 | 6.0 |
| AC/DC navigation 메인 스레드 전달 | 0.3 | 0.4 |
| navigation 반환 → AC/DC 보호 리소스 Finished | 3,012.2 | 3,588.2 |
| 두 Access identity 병렬 요청 → body 완료 | 676.5 | 549.8 |
| AC/DC GET /api/me → body 완료 | 189.8 | 460.1 |
| main 인증 페이지 Started → Finished | 109.4 | 118.9 |
| 인증 완료 시점 (누적) | 8,573.6 | 9,770.2 |
| 사용자 identity ready + App DOM + 2 animation frames (누적) | 미계측* | 9,827.9 |

*실행 1의 렌더 진단 명령은 custom-protocol IPC가 Origin만 전달하기 때문에 path를 포함한 ACL에서 거부됐다. 진단 capability는 origin으로 제한하고 실제 main `/app/` 검증은 native 명령에서 수행하도록 바로잡았다. 실행 2에서 실제 신호를 확인했다. 실행 1은 별도 CDP 읽기로 identity ready, 앱 5개, 첫 contentful paint 120ms (문서 기준)를 확인했으나 시작부터의 렌더 완료 시간으로 대체하지 않았다.

실행 2의 WebView 탐색 경계 (앱 시작 기준 ms):

| WebView / 문서 | Started | Finished |
|---|---:|---:|
| main /prelogin/ | 1,723.9 | 1,778.8 |
| profile-popup (번들) | 3,211.4 | 3,300.3 |
| login MonaHub /app/ | 4,788.9 | 4,963.9 |
| login AC/DC 보호 CSS | 8,528.5 | 8,559.6 |
| main /app/ | 9,651.0 | 9,769.8 |

`Started`는 native `navigate` 호출 시각이 아니다. AC/DC의 3초 이상 공백이 바로 그 사이에 있다. 실행 1에서 브라우저 navigation timing도 총 2,991.9ms, 최종 fetchStart 2,936.8ms로 나타났다. 교차 origin SSO 리다이렉트의 상세 시간은 이 API에서 노출되지 않으므로 Cloudflare/MS 각 서버별 원인까지 단정하지 않는다.

두 실행 모두 native `/api/me` 1회, 각 Access identity 1회였다. 실행 1 main의 browser resource timing에도 native identity IPC 1회가 있고 추가 HTTP `/api/me`는 없었다. login에서 main으로 같은 `/app/`를 다시 읽는 기존 구조와 동일 WebView의 중복 탐색을 구분했다.

두 번째 인스턴스 시험: PID 28632는 single setup 시작까지만 기록하고 종료했다. 기존 PID 7592에서 second-instance callback 1회가 기록됐고, 첫 시작 중에는 그 콜백이 없었다. 새 main/login 생성이나 인증 재실행도 없었다.

원본 계측: `tmp/startup-production-1.log`, `tmp/startup-production-2.log`, `tmp/startup-second-instance.log`. 최종 실행은 추가 브라우저 디버깅 플래그 없이 측정했다.

## 판단 및 변경 범위

현재 실측의 큰 지연 구간은 WebView 생성과 AC/DC SSO 탐색이다. single-instance setup과 UI 스레드 전달은 각각 수 ms 이하이며, API 응답 때문에 AppBar 표시가 기다리는 현상은 없다. 기존 코드가 AC/DC 앱 쿠키를 지우고 SSO를 재수행하는 것은 관찰됐지만, 계정 전환 시 잘못된 AC/DC 계정 재사용을 막는 기존 장치이므로 근거 없이 제거하지 않았다.

변경 전 실행 파일을 같은 조건에서 측정하지 않았으므로 **싱글 인스턴스 변경에 의한 성능 회귀의 원인은 확정하지 못했다**. 성능 수정은 하지 않았다. 이 변경은 선택적 계측, 진단 IPC, 조사 기록만 포함하며 single-instance 및 인증 동작을 유지한다. WebView 비용 변동이나 SSO 서버별 지연, 기존 세션 재사용 최적화는 별도 비교·검증이 필요하다.

검증: 배포용 release 빌드 성공, `npm run check` 성공, 기존 `npm test` 3개 통과, `git diff --check` 성공. 기존 appbar의 unused import/variable 경고 2개는 그대로다. 새 로그인 경로는 시험하지 않았다.
