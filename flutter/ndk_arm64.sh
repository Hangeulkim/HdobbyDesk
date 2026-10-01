#!/usr/bin/env bash
set -euo pipefail

: "${ANDROID_NDK_HOME:?ANDROID_NDK_HOME must point to the Android NDK}"

# Cross-compiled archives must use the Android NDK tools. macOS ar/ranlib can
# silently discard ELF members and leave dependencies such as libsodium empty.
ndk_toolchain="$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/darwin-x86_64/bin"
export AR="$ndk_toolchain/llvm-ar"
export RANLIB="$ndk_toolchain/llvm-ranlib"
export STRIP="$ndk_toolchain/llvm-strip"
export AR_aarch64_linux_android="$AR"
export RANLIB_aarch64_linux_android="$RANLIB"
export CARGO_TARGET_AARCH64_LINUX_ANDROID_AR="$AR"

# The Android package embeds the cdylib only. Building unrelated desktop/helper
# binaries can fail on Android after the library has already compiled.
cargo ndk --platform 21 --target aarch64-linux-android build --locked --release --features flutter,hwcodec --lib

native_library="target/aarch64-linux-android/release/liblibhdobbydesk.so"
if "$ndk_toolchain/llvm-nm" -D --undefined-only "$native_library" | grep -q ' sodium_'; then
    echo "Android native library contains unresolved libsodium symbols" >&2
    exit 1
fi

# Flutter loads this exact library name. Cargo's cdylib artifact has an extra
# `lib` prefix; leaving it beside an older copy silently packages both cores.
jni_dir="flutter/android/app/src/main/jniLibs/arm64-v8a"
mkdir -p "$jni_dir"
install -m 0644 "$native_library" "$jni_dir/libhdobbydesk.so"
rm -f "$jni_dir/liblibhdobbydesk.so"
