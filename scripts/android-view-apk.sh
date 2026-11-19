#!/bin/sh
# Builds examples/android-view into a signed APK without Gradle: cargo builds the cdylib,
# the Kotlin compiler (from Gradle's cache, or KOTLIN_COMPILER_CP) compiles the host
# activity and view against android.jar, d8 dexes them with the Kotlin standard library, and
# aapt2/zipalign/apksigner package it.
#
#   scripts/android-view-apk.sh [--debug]
#
# Needs what scripts/android-apk.sh needs, plus a JDK. RUNE_ANDROID_TARGET picks the ABI
# (aarch64-linux-android by default). The APK lands in target/android/android-view.apk.
set -eu

profile=release
for arg in "$@"; do
  case "$arg" in
    --debug) profile=debug ;;
    *) echo "unknown argument $arg" >&2; exit 2 ;;
  esac
done
root=$(cd "$(dirname "$0")/.." && pwd)
dir=$root/examples/android-view

: "${ANDROID_HOME:?set ANDROID_HOME to the Android SDK}"
if [ -z "${NDK_HOME:-}" ]; then
  NDK_HOME=$(ls -d "$ANDROID_HOME"/ndk/* 2>/dev/null | sort -V | tail -1)
fi
build_tools=$(ls -d "$ANDROID_HOME"/build-tools/* | sort -V | tail -1)
platform=$(ls -d "$ANDROID_HOME"/platforms/android-* | sort -V | tail -1)
llvm=$NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/bin
api=30

target=${RUNE_ANDROID_TARGET:-aarch64-linux-android}
case "$target" in
  aarch64-linux-android) abi=arm64-v8a ;;
  x86_64-linux-android) abi=x86_64 ;;
  *) echo "unsupported target $target" >&2; exit 2 ;;
esac
triple_env=$(echo "$target" | tr - _)
triple_upper=$(echo "$triple_env" | tr '[:lower:]' '[:upper:]')
export "CC_$triple_env=$llvm/$target$api-clang"
export "CXX_$triple_env=$llvm/$target$api-clang++"
export "AR_$triple_env=$llvm/llvm-ar"
export "CARGO_TARGET_${triple_upper}_LINKER=$llvm/$target$api-clang"
export "CARGO_TARGET_${triple_upper}_RUSTFLAGS=-C link-arg=-Wl,-z,max-page-size=16384"
export ANDROID_NDK_ROOT="$NDK_HOME"

cd "$root"
flag=--release
[ "$profile" = debug ] && flag=
cargo build -p android-view --lib --target "$target" $flag

target_dir=$(cargo metadata --format-version 1 --no-deps | sed -n 's/.*"target_directory":"\([^"]*\)".*/\1/p')
out=$target_dir/android
mkdir -p "$out"
stage=$(mktemp -d "$out/android-view.XXXXXX")

# Kotlin: the compiler and standard library jars Gradle has cached.
cache=$HOME/.gradle/caches/modules-2/files-2.1
jar() { find "$cache/$1" -name "$2-$3.jar" | head -1; }
kv=${KOTLIN_VERSION:-2.2.21}
stdlib=$(jar org.jetbrains.kotlin/kotlin-stdlib kotlin-stdlib "$kv")
cp_compiler=${KOTLIN_COMPILER_CP:-$(jar org.jetbrains.kotlin/kotlin-compiler-embeddable kotlin-compiler-embeddable "$kv"):$stdlib:$(jar org.jetbrains.kotlin/kotlin-script-runtime kotlin-script-runtime "$kv"):$(jar org.jetbrains.kotlin/kotlin-reflect kotlin-reflect "$kv"):$(jar org.jetbrains.kotlin/kotlin-daemon-embeddable kotlin-daemon-embeddable "$kv"):$(find "$cache/org.jetbrains.kotlinx/kotlinx-coroutines-core-jvm" -name 'kotlinx-coroutines-core-jvm-1.*.jar' | sort -V | tail -1):$(find "$cache/org.jetbrains/annotations" -name 'annotations-*.jar' | sort -V | tail -1)}
mkdir -p "$stage/classes" "$stage/dex" "$stage/lib/$abi"
java -cp "$cp_compiler" org.jetbrains.kotlin.cli.jvm.K2JVMCompiler -no-stdlib -no-reflect \
  -classpath "$platform/android.jar:$stdlib" -jvm-target 11 -d "$stage/classes" \
  $(find "$dir/kotlin" -name '*.kt')
"$build_tools/d8" --release --min-api "$api" --lib "$platform/android.jar" --output "$stage/dex" \
  $(find "$stage/classes" -name '*.class') "$stdlib"

so=$target_dir/$target/$profile/libandroid_view.so
cp "$so" "$stage/lib/$abi/"
"$llvm/llvm-strip" --strip-debug "$stage/lib/$abi/libandroid_view.so"

"$build_tools/aapt2" link -o "$stage/base.apk" --manifest "$dir/AndroidManifest.xml" -I "$platform/android.jar"
(cd "$stage" && zip -q -r base.apk lib && cd dex && zip -q ../base.apk classes.dex)
"$build_tools/zipalign" -f -p 4 "$stage/base.apk" "$stage/aligned.apk"
keystore=$out/debug.jks
[ -f "$keystore" ] || keytool -genkeypair -keystore "$keystore" -storepass android -keypass android \
  -alias debug -keyalg RSA -keysize 2048 -validity 10000 -dname CN=rune-debug >/dev/null
apk=$out/android-view.apk
"$build_tools/apksigner" sign --ks "$keystore" --ks-pass pass:android --out "$apk" "$stage/aligned.apk"
echo "$apk"
