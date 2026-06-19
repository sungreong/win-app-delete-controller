# Windows App Delete Controller

Windows App Delete Controller는 Windows에 등록된 설치 앱 제거 명령을 한 화면에서 찾고, 검토하고, 실행할 수 있게 만든 데스크톱 도구입니다.

Rust와 egui로 만들었고, HKLM/HKCU의 표준 Windows Uninstall 레지스트리 항목을 읽어 앱 목록을 구성합니다.

[English README](README.en.md)

![Windows App Delete Controller screenshot](docs/screenshots/main.png)

## 만든 배경

Windows에서 앱을 지우다 보면 생각보다 불편한 지점이 많았습니다.

- 앱 제거 화면이 여러 곳에 흩어져 있어 원하는 항목을 빠르게 찾기 어렵습니다.
- 여러 앱을 한 번에 선택해서 순차적으로 제거하기 어렵습니다.
- 게시자, 설치 위치, 크기, 설치일, 제거 명령 같은 정보를 한 화면에서 비교하기 어렵습니다.
- 어떤 앱은 일반 권한으로 실행하면 실패하고, 어떤 앱은 관리자 권한이 필요합니다.
- 어떤 앱은 `NoRemove`처럼 제거 제한 표시가 있지만, 그것이 “절대 제거 불가”인지 “주의가 필요한 항목”인지 판단하기 어렵습니다.
- 제거 프로그램을 실행한 뒤 실제로 Windows 앱 목록에서 사라졌는지 직접 다시 확인해야 합니다.

이 프로젝트는 이런 불편함을 줄이기 위해 만들었습니다. 앱을 무작정 삭제하는 도구가 아니라, Windows에 등록된 제거 명령을 더 잘 찾고, 더 안전하게 검토하고, 실행 후 결과를 확인하기 위한 컨트롤러에 가깝습니다.

## 주요 기능

- Windows Uninstall 레지스트리에서 설치 앱 목록을 스캔합니다.
- 앱 이름, 게시자, 버전, 설치 위치, 레지스트리 경로, 크기, 설치일로 검색할 수 있습니다.
- 게시자, 제거 가능 상태, 숨김 시스템 항목, 제거 제한 항목 기준으로 필터링할 수 있습니다.
- 앱 이름, 게시자, 버전, 예상 크기 기준으로 정렬할 수 있습니다.
- 헤더 경계선을 드래그해 표 열 너비를 조절할 수 있습니다.
- 현재 페이지의 앱을 선택하고, 여러 제거 명령을 순차적으로 실행할 수 있습니다.
- `MsiExec.exe /I{...}` 형태의 MSI 설치 명령을 제거 동작에 맞게 `/X{...}`로 변환합니다.
- 관리자 권한이 필요한 제거 명령을 감지하고, Windows UAC를 통해 다시 실행할 수 있습니다.
- 제거 명령이 종료된 뒤 앱 목록을 다시 스캔해 Windows 목록에서 사라졌는지 확인합니다.
- 필터와 화면 설정을 `%APPDATA%\WinAppDeleteController\settings.ini`에 저장합니다.

## 일반적인 사용 흐름

1. 검색어나 필터로 앱 목록을 좁힙니다.
2. 게시자, 버전, 크기, 설치일, 제거 가능 상태를 확인합니다.
3. 현재 페이지에서 하나 이상의 앱을 선택합니다.
4. 실행될 제거 명령을 확인합니다.
5. Windows 또는 앱 제작사의 제거 프로그램을 진행합니다.
6. 작업 상태에서 앱이 Windows 목록에서 제거됐는지 확인합니다.

## 하지 않는 것

이 도구는 프로그램 폴더나 레지스트리 키를 강제로 삭제하지 않습니다.

각 앱이 Windows에 등록한 제거 명령을 실행하고, 그 뒤 앱이 Windows Uninstall 목록에 아직 남아 있는지 확인합니다. 일부 앱은 자체 제거 UI, 관리자 권한, 조직 정책 변경, 또는 제작사 전용 제거 도구가 필요할 수 있습니다.

`NoRemove` 또는 제거 제한 항목은 Windows나 앱 제작사가 일반적인 제거를 허용하지 않도록 표시한 항목입니다. 항상 제거가 불가능하다는 뜻은 아니지만, 보통 더 신중하게 다뤄야 합니다.

## 소스에서 실행

```powershell
cargo run
```

## 빌드

```powershell
cargo build --release
```

빌드 결과는 다음 위치에 생성됩니다.

```powershell
.\target\release\win_app_delete_controller.exe
```

## 로컬 설치

먼저 release 빌드를 만든 뒤 실행합니다.

```powershell
.\setup\setup.cmd
```

설치 스크립트는 현재 사용자 계정 기준으로 앱을 복사하고 바로가기를 만듭니다.

- 설치 위치: `%LOCALAPPDATA%\Programs\WinAppDeleteController`
- 바탕화면 바로가기
- 시작 메뉴 바로가기
- 시작 메뉴 제거 바로가기

설치하지 않고 바로 실행하려면 다음 파일을 사용할 수 있습니다.

```powershell
.\setup\run.cmd
```

## 릴리스 패키지

저장소에는 생성된 exe와 zip 파일을 커밋하지 않습니다. GitHub Release에는 로컬에서 빌드한 설치 패키지를 release asset으로 업로드합니다.

일반적으로 사용하는 로컬 패키지 파일은 다음과 같습니다.

- `WinAppDeleteControllerSetup.zip`
- `WinAppDeleteControllerSetup-latest.zip`

## 개발

테스트 실행:

```powershell
cargo test
```

포맷 확인:

```powershell
cargo fmt -- --check
```

## 참고

- 이 프로젝트는 Windows를 대상으로 합니다.
- 관리자 권한은 특정 제거 명령이 권한 상승을 요구할 때만 요청합니다.
- 제거 확인은 앱이 Windows Uninstall 레지스트리 목록에 계속 남아 있는지 여부를 기준으로 합니다.
