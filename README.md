# HdobbyDesk

Windows·macOS·Android에서 사용하는 직접 연결 중심의 원격 제어 앱입니다. RustDesk 1.4.9를 수정한 프로젝트이며 **AGPL-3.0** 라이선스를 따릅니다.

현재는 **개발 프리뷰 소스**입니다. Windows 호스트에 Mac·Android로 연결하는 경로는 실제 기기로 시험했지만, 모든 방향·기능·장시간 안정성을 검증한 안정판은 아닙니다. iPhone·iPad 빌드와 실기기 동작은 미검증이며 iOS 호스트 기능은 제공하지 않습니다.

## 연결하기

AI나 개발 도구 없이 앱에서 연결을 준비할 수 있도록 만든 흐름입니다. 새 사용자의 설치부터 연결까지 전체 절차에 대한 최종 사용성 검증은 남아 있습니다.

1. **호스트:** 앱을 설치·실행하고 **직접 연결 준비**를 누릅니다. 앱이 기기용 인증서와 개인키를 생성해 로컬에 저장하며, 기존 인증서가 있으면 재사용합니다. 원격 조작에 필요한 OS 권한을 허용합니다.
2. 호스트 보안 설정에서 **고정 비밀번호**를 지정합니다. 일회용 비밀번호와 고정 비밀번호는 별개입니다.
3. 접속자가 도달할 수 있는 호스트 주소와 포트를 지정하고 **연결 코드 전체**를 복사해 상대에게 전달합니다. 연결 코드에는 주소와 공개 인증서가 포함되며, 개인키와 비밀번호는 포함되지 않습니다.
4. **접속 기기:** 직접 연결 준비에서 상대 연결 코드를 붙여 넣고, 호스트에서 확인한 인증서 지문과 일치하는지 확인한 뒤 저장합니다.
5. 연결을 누르고 호스트의 고정 비밀번호를 입력합니다. 인증 정보 저장을 선택했다면 이후 최근 연결에서 다시 접속할 수 있습니다. 앱 데이터나 호스트 인증서를 초기화하면 다시 등록해야 합니다.

공개 ID·중계·API·STUN 서버를 기본 사용하지 않습니다. 직접 연결이 실패했을 때 공개 중계로 자동 우회하지 않습니다. 사용자가 별도로 지정한 내부 서버는 선택적으로 사용할 수 있습니다.

### 네트워크

같은 내부망이라도 기기 간 통신이 허용되어야 합니다. Wi-Fi 또는 이동통신이라는 구분보다 **접속 기기에서 호스트 주소·포트에 도달할 수 있는지**가 중요합니다. 외부에서 내부 호스트로 접속하려면 조직에서 허용한 VPN·라우팅·포트 전달 등의 경로가 필요하며, 앱이 네트워크 차단을 자동 우회하지 않습니다. 기본 직접 연결 포트는 TCP `21118`입니다. 접속이 되는 환경에서 방화벽 설정을 추가로 변경할 필요는 없습니다.

### Windows Console과 RDP

- **Console:** 물리 모니터와 같은 Windows 세션입니다.
- **RDP:** Windows App 등으로 접속한 별도 Windows 바탕 화면입니다. 물리 모니터와 같은 화면으로 취급하지 않습니다.
- RDP 세션 제어에는 호스트의 RDP 공유 설정이 필요합니다. 설치된 서비스가 사용하는 레지스트리 위치의 `share_rdp` 문자열 값이 `true`여야 합니다. 해당 설정 누락을 복구한 뒤 사용자가 전환 동작을 확인했습니다.
- 현재 세션 선택은 다른 HdobbyDesk 접속자가 동시에 연결되어 있으면 제한될 수 있습니다. 세션 전환과 다중 접속을 모두 검증한 것으로 해석하지 마세요.
- RDP 서비스나 Windows 로그인 세션을 강제 종료해 해결하지 마세요.

## 기능과 확인 범위

| 항목 | 현재 범위 |
|---|---|
| Mac·Android → Windows | 직접 연결·화면·기본 입력을 실기기로 시험함 |
| 저장된 인증 정보로 재접속 | 시험 이력 있음; 모든 최신 조합의 장시간 안정성 보증은 아님 |
| 한/영 입력·클립보드·드래그·자동 키보드 | 구현 및 시험 범위가 기능별로 다름; 아래 상세 문서 참조 |
| Windows → Mac, Android 호스트 | 전체 최신 조합 검증 미완료 |
| 다중 접속 | 공유 바탕 화면 제어; 독립된 사용자별 Windows 마우스·키보드 세션을 제공하는 것은 아님 |
| 관리 창 가림 | 캡처 제외와 입력 차단의 제한을 상세 문서에서 확인 |
| iPhone·iPad | 컨트롤러 관련 코드가 있으나 전체 빌드·실기기 검증 미완료 |

- [기능·기존 검증 기록](docs/HDOBBY.md)
- [최근 설치본·재접속 확인 범위](docs/VERIFICATION.md)
- [직접 연결과 내부 서버](docs/PRIVATE_NETWORK.md)
- [테스트와 비밀정보 보호](docs/PRIVATE_TESTING.md)
- [다중 접속과 키보드 게임패드](docs/COLLABORATIVE_INPUT.md)
- [관리 화면 가림의 범위](docs/ADMIN_PRIVACY.md)

## 소스 준비와 개발

원본 공용 라이브러리는 고정된 Git 서브모듈로 받고, 이 프로젝트의 수정은 버전 관리되는 패치로 적용합니다. 원본 서브모듈만 받아서는 HdobbyDesk의 TLS·입력 기능을 빌드할 수 없습니다.

```sh
git submodule update --init --recursive
python3 scripts/apply_private_network.py --apply
```

네이티브 코어는 Rust/Cargo와 대상 OS의 C/C++ 도구, Flutter UI는 Flutter SDK와 대상 OS SDK가 필요합니다. Windows는 Windows 빌드 도구, macOS/iOS 전체 앱 빌드는 Xcode, Android는 Android SDK/NDK가 필요합니다. 현재 저장소의 전체 빌드 워크플로는 원본에서 가져온 것으로, HdobbyDesk 이름·패치 적용·서명·패키징까지 검증한 자동 릴리스 절차가 아닙니다. 기존 시험 앱을 복사하는 방식도 깨끗한 환경에서의 전체 빌드 검증을 대신하지 않습니다.

Rust–Dart 연결 코드는 생성 파일이므로 소스를 받은 뒤 생성해야 합니다. macOS의 새 체크아웃에서 Flutter 3.24.5 / Dart 3.5.4, Rust 1.81.0, `flutter_rust_bridge_codegen` 1.80.1을 사용해 아래 생성 절차를 확인했습니다. `cargo-expand`와 libclang도 필요합니다. 도구 버전 확인과 연결 코드 생성 성공은 전체 앱 패키지 빌드 성공을 뜻하지 않습니다.

```sh
cargo install cargo-expand --version 1.0.95 --locked
cargo install flutter_rust_bridge_codegen --version 1.80.1 --locked
cd flutter
flutter pub get
cd ..
RUST_LOG=info flutter_rust_bridge_codegen \
  --rust-input src/flutter_ffi.rs \
  --dart-output flutter/lib/generated_bridge.dart \
  --c-output flutter/macos/Runner/bridge_generated.h \
  --llvm-path /Library/Developer/CommandLineTools/usr
cp flutter/macos/Runner/bridge_generated.h flutter/ios/Runner/bridge_generated.h
```

위 libclang 경로는 macOS Command Line Tools 설치 기준입니다. 다른 환경에서는 libclang이 설치된 접두 경로로 바꾸세요. 생성 중 오류가 나면 전체 앱 빌드로 넘어가지 말고 먼저 해결해야 합니다.

기본 테스트는 실제 서버에 접속하지 않습니다.

```sh
python3 -m unittest discover -s scripts/tests
python3 scripts/test_private_network.py
cd flutter/packages/hdobby_input
flutter pub get
flutter test
```

실제 서버 시험은 실행자가 주소와 인증 정보를 명시적으로 제공해야 합니다. 저장소에 실제 접속 주소를 기본값으로 넣지 않습니다.

## 보안·배포

직접 연결은 등록한 공개 인증서를 검증하는 TLS 경로를 사용합니다. 연결 코드의 Base32 표기는 복사·전달용 인코딩이며 암호화 자체가 아닙니다. 개인키는 호스트에 남습니다. 인증서 지문은 신뢰할 수 있는 경로로 확인하세요. 전체 앱에 대한 독립 보안 감사나 미래의 모든 공격에 대한 안전을 보증하지 않습니다.

실제 IP·서버 주소·비밀번호·개인키·인증서·운영 설정·로그·화면 캡처는 소스에 포함하지 않습니다. 커밋 전 다음 검사를 실행하고 변경 내용을 검토합니다.

```sh
python3 scripts/check_private_config.py
git diff --cached
```

이 소스 공개와 설치 파일의 안정판 배포는 별개입니다. 현재 Mac 시험본은 로컬 서명이며 Apple 공증을 완료하지 않았고, Windows 실행 파일도 공용 코드 서명을 제공하지 않습니다. Android 배포용 서명키는 저장소에 넣지 않습니다. 출처가 검증되지 않은 기존 시험 패키지를 정식 릴리스에 올리지 않습니다.

## 라이선스와 출처

[AGPL-3.0 라이선스](LICENCE)와 원작 저작권 고지를 유지합니다. 원작은 [RustDesk](https://github.com/rustdesk/rustdesk)이며 MIT로 재라이선스한 프로젝트가 아닙니다. 앱의 오픈소스 고지에서도 원작 출처와 라이선스를 확인할 수 있습니다. 포함된 제삼자 코드에는 각 디렉터리의 라이선스와 출처 기록이 적용됩니다.
