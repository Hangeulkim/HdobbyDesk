#!/usr/bin/env bash
set -euo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")"
generated_header="macos/Runner/bridge_generated.h"
ios_header="ios/Runner/bridge_generated.h"
if [[ ! -f "${generated_header}" ]]; then
  echo "Generate the Rust/Flutter bridge before building HdobbyDesk for iOS." >&2
  exit 1
fi
if [[ ! -f "${ios_header}" ]] || ! cmp -s "${generated_header}" "${ios_header}"; then
  cp "${generated_header}" "${ios_header}"
fi
if ! xcrun --sdk iphoneos --show-sdk-path >/dev/null 2>&1; then
  echo "Install and select Xcode with the iOS SDK before building HdobbyDesk." >&2
  exit 1
fi
# Use this checkout and the caller's selected toolchain/signing configuration.
# Do not patch a shared Flutter SDK or use another developer's source directory.
flutter build ipa --release "$@"
