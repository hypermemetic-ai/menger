#!/usr/bin/env bash
# menger v2 as an APK: a WebView on a menger server's /app/ (android/README.md).
#
#   android/build.sh            → dist/android/menger.apk (signed, release)
#   android/build.sh debug      → dist/android/menger-debug.apk (debuggable: chrome://inspect)
#
# No Gradle and no network: aapt2, javac, d8, zipalign and apksigner in the
# menger-desktop-android image (menger-desktop's docker/android.Dockerfile:
# JDK 17, the Android SDK), the repository mounted, run as your uid.
#
# Signing: $MENGER_ANDROID_KEYSTORE (a .jks) with its password in
# "$MENGER_ANDROID_KEYSTORE.pass"; by default ~/.android-keys/menger-app.jks,
# made on the first build with a random password (0600). Never in the repository.
set -euo pipefail

VERSION_NAME=0.1.0
VERSION_CODE=1
MIN_SDK=29
TARGET_SDK=35

mode="${1:-release}"
case "$mode" in release | debug) ;; *) echo "usage: $0 [release|debug]" >&2; exit 2 ;; esac

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo="$(cd "$here/.." && pwd)"
image="${MENGER_ANDROID_IMAGE:-menger-desktop-android}"
docker image inspect "$image" >/dev/null 2>&1 || {
  echo "no $image image: build it from menger-desktop (docker build --target build -t $image -f docker/android.Dockerfile docker)" >&2
  exit 1
}

keystore="${MENGER_ANDROID_KEYSTORE:-$HOME/.android-keys/menger-app.jks}"
keydir="$(dirname "$keystore")"
mkdir -p "$keydir" && chmod 700 "$keydir"
if [[ ! -f "$keystore" ]]; then
  umask 077
  head -c 24 /dev/urandom | base64 | tr -d '/+=' >"$keystore.pass"
fi
[[ -r "$keystore.pass" ]] || { echo "$keystore has no $keystore.pass" >&2; exit 1; }

out="$repo/dist/android"
mkdir -p "$out"
apk_name=$([[ $mode == debug ]] && echo menger-debug.apk || echo menger.apk)

docker run --rm -i \
  --security-opt label=disable \
  --user "$(id -u):$(id -g)" \
  -e HOME=/tmp \
  -v "$here:/app:ro" \
  -v "$out:/out" \
  -v "$keydir:/keys" \
  -e MODE="$mode" -e APK="$apk_name" -e KS="/keys/$(basename "$keystore")" \
  -e VERSION_NAME="$VERSION_NAME" -e VERSION_CODE="$VERSION_CODE" -e MIN_SDK="$MIN_SDK" -e TARGET_SDK="$TARGET_SDK" \
  "$image" bash -euo pipefail -s <<'EOF'
sdk=/opt/android-sdk
bt=$(ls -d $sdk/build-tools/* | sort -V | tail -1)
jar=$(ls -d $sdk/platforms/*/android.jar | sort -V | tail -1)
w=$(mktemp -d)

if [[ ! -f "$KS" ]]; then
  keytool -genkeypair -keystore "$KS" -storepass:file "$KS.pass" -keypass:file "$KS.pass" \
    -alias menger -keyalg EC -groupname secp256r1 -validity 10000 -dname "CN=menger app" >/dev/null 2>&1
  chmod 600 "$KS"
  echo "made the signing key $KS"
fi

debug=()
[[ $MODE == debug ]] && debug=(--debug-mode)
"$bt/aapt2" compile --dir /app/res -o "$w/res.zip"
"$bt/aapt2" link -o "$w/base.apk" -I "$jar" --manifest /app/AndroidManifest.xml "$w/res.zip" \
  --min-sdk-version "$MIN_SDK" --target-sdk-version "$TARGET_SDK" \
  --version-code "$VERSION_CODE" --version-name "$VERSION_NAME" "${debug[@]}"

mkdir -p "$w/gen/ai/hypermemetic/menger/app" "$w/classes" "$w/dex"
cat >"$w/gen/ai/hypermemetic/menger/app/BuildInfo.java" <<J
package ai.hypermemetic.menger.app;
final class BuildInfo { static final String VERSION = "$VERSION_NAME"; }
J
javac -nowarn -encoding UTF-8 --release 11 -classpath "$jar" -d "$w/classes" \
  $(find /app/src "$w/gen" -name '*.java')
"$bt/d8" --release --min-api "$MIN_SDK" --lib "$jar" --output "$w/dex" $(find "$w/classes" -name '*.class')

cp "$w/base.apk" "$w/unaligned.apk"
(cd "$w/dex" && "$bt/aapt" add "$w/unaligned.apk" classes.dex >/dev/null)
"$bt/zipalign" -f -p 4 "$w/unaligned.apk" "$w/aligned.apk"
"$bt/apksigner" sign --ks "$KS" --ks-pass "file:$KS.pass" --ks-key-alias menger --out "/out/$APK" "$w/aligned.apk"
"$bt/apksigner" verify "/out/$APK"
rm -f "/out/$APK.idsig"
echo "/out/$APK $(stat -c %s "/out/$APK") bytes"
EOF
echo "$out/$apk_name"
