#!/bin/sh
# Builds a Rune example into a signed APK with cargo and the Android SDK's build tools: no
# Gradle, no Java code (NativeActivity loads the example's cdylib).
#
#   scripts/android-apk.sh [example] [--install] [--debug]
#
# example defaults to android-hello; it needs an AndroidManifest.xml beside its Cargo.toml and
# a cdylib with `android_main` (see examples/android-hello). The APK lands in
# target/android/<example>.apk. --install also installs and starts it with adb (an emulator or
# a phone; set ANDROID_SERIAL to pick one). It builds release: on an emulator's software GPU a
# debug build hadn't drawn its first frame after a minute.
#
# Needs: rustup target aarch64-linux-android; ANDROID_HOME (platforms;android-35,
# build-tools;35.0.0, an NDK) and a JDK for keytool/apksigner. NDK_HOME defaults to the newest
# NDK under ANDROID_HOME. The debug keystore is made once in target/android/debug.jks.
set -eu

example=android-hello
install=0
profile=release
for arg in "$@"; do
  case "$arg" in
    --install) install=1 ;;
    --debug) profile=debug ;;
    -*) echo "unknown option $arg" >&2; exit 2 ;;
    *) example=$arg ;;
  esac
done

root=$(cd "$(dirname "$0")/.." && pwd)
dir=$root/examples/$example
[ -f "$dir/AndroidManifest.xml" ] || { echo "no AndroidManifest.xml in examples/$example" >&2; exit 1; }

: "${ANDROID_HOME:?set ANDROID_HOME to the Android SDK}"
if [ -z "${NDK_HOME:-}" ]; then
  NDK_HOME=$(ls -d "$ANDROID_HOME"/ndk/* 2>/dev/null | sort -V | tail -1)
fi
[ -d "${NDK_HOME:-}" ] || { echo "no NDK: set NDK_HOME or install one under \$ANDROID_HOME/ndk" >&2; exit 1; }
build_tools=$(ls -d "$ANDROID_HOME"/build-tools/* | sort -V | tail -1)
platform=$(ls -d "$ANDROID_HOME"/platforms/android-* | sort -V | tail -1)
case "$(uname -s)" in
  Darwin) host=darwin-x86_64 ;;
  Linux) host=linux-x86_64 ;;
  *) host=windows-x86_64 ;;
esac
llvm=$NDK_HOME/toolchains/llvm/prebuilt/$host/bin
api=26 # minSdkVersion: AAudio and the NDK's current floor for 64-bit

target=aarch64-linux-android
export CC_aarch64_linux_android="$llvm/aarch64-linux-android$api-clang"
export CXX_aarch64_linux_android="$llvm/aarch64-linux-android$api-clang++"
export AR_aarch64_linux_android="$llvm/llvm-ar"
export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$llvm/aarch64-linux-android$api-clang"
export ANDROID_NDK_ROOT="$NDK_HOME"

cd "$root"
flag=--release
[ "$profile" = debug ] && flag=
cargo build -p "$example" --lib --target "$target" $flag

lib=$(echo "$example" | tr - _)
target_dir=$(cargo metadata --format-version 1 --no-deps | sed -n 's/.*"target_directory":"\([^"]*\)".*/\1/p')
so=$target_dir/$target/$profile/lib$lib.so
out=$target_dir/android
stage=$out/$example
rm -rf "$stage" && mkdir -p "$stage/lib/arm64-v8a"
cp "$so" "$stage/lib/arm64-v8a/"
# Strip debug info: a debug build's library is hundreds of MB otherwise.
"$llvm/llvm-strip" --strip-debug "$stage/lib/arm64-v8a/lib$lib.so"

"$build_tools/aapt2" link -o "$stage/base.apk" --manifest "$dir/AndroidManifest.xml" -I "$platform/android.jar"
(cd "$stage" && zip -q -r base.apk lib)
"$build_tools/zipalign" -f -p 4 "$stage/base.apk" "$stage/aligned.apk"

keystore=$out/debug.jks
[ -f "$keystore" ] || keytool -genkeypair -keystore "$keystore" -storepass android -keypass android \
  -alias debug -keyalg RSA -keysize 2048 -validity 10000 -dname CN=rune-debug >/dev/null
apk=$out/$example.apk
"$build_tools/apksigner" sign --ks "$keystore" --ks-pass pass:android --out "$apk" "$stage/aligned.apk"
echo "$apk"

if [ "$install" = 1 ]; then
  adb=$ANDROID_HOME/platform-tools/adb
  package=$(sed -n 's/.*package="\([^"]*\)".*/\1/p' "$dir/AndroidManifest.xml")
  "$adb" install -r "$apk"
  "$adb" shell am start -n "$package/android.app.NativeActivity"
fi
