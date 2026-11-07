# menger on Android

An APK of one activity and one WebView, opened on a menger v2 server's
`/app/`: the server's live plexus-ui bundle (the engine, the components,
the theme), its socket and its sign-in, all on the server's own origin.
Nothing of menger is in the APK (about 110 KB), so the server decides the
version and every open page hot-swaps on a push, as in a browser.

```sh
android/build.sh            # dist/android/menger.apk — signed, release
android/build.sh debug      # dist/android/menger-debug.apk — debuggable (chrome://inspect)
```

The build is `aapt2`, `javac`, `d8`, `zipalign` and `apksigner` in the
`menger-desktop-android` image (menger-desktop's `docker/android.Dockerfile`),
with no Gradle and no network. It signs with `~/.android-keys/menger-app.jks`
(or `$MENGER_ANDROID_KEYSTORE`), whose password is in the `.pass` file beside
it; the first build makes both. Neither is ever in the repository. Android 10
(API 29) and up; target SDK 35.

## Using it

- The first start asks which server: **prod** (`https://app.menger.sh`),
  **test** (`https://app-test.menger.sh`), or **Other…** (any address). Both
  run v2 since 2026-11-06. The choice is remembered. A long press on the
  launcher icon gives **menger prod** and **menger test**, which open one
  directly, and **Switch server**, which opens the picker; the "not
  reachable" page offers it too. Sign-ins are kept per server, so switching
  signs you out of neither.
- The server's origin stays in the app, along with a sign-in's hops: a
  redirect, the IdP's pages, `/login`. A link tapped on a menger page to
  another site opens in the browser. File inputs use the system picker.
- From adb: `am start -n ai.hypermemetic.menger.app/.MainActivity -e server URL`
  sets the server; `-e open URL` opens a page on a server, such as a
  ticket's link or a local menger's one-time sign-in link.

## Trying it on this machine

```sh
../menger-desktop/scripts/android-emulator.sh start
docker cp dist/android/menger-debug.apk menger-desktop-emulator:/tmp/m.apk
docker exec menger-desktop-emulator adb install -r /tmp/m.apk
# a local v2 menger (README "Run"; never on prod's ports), then:
docker exec menger-desktop-emulator adb reverse tcp:PORT tcp:PORT      # the socket's and the shell's
docker exec menger-desktop-emulator adb shell am start -n ai.hypermemetic.menger.app/.MainActivity \
  -e open "'http://127.0.0.1:SHELL_PORT/login?once=…'"
```

## Why the engine is not in the APK

The ideal is to put plexus-ui's engine (`host.js`, `engine.js`,
`engine.wasm`) in the APK and have it reach menger. That does not work
today, for four reasons:

- `host.js` names the socket from `location` (`wss://<page host>/ws`), and it
  fetches `/app/<version>/…` and frames `/frame` on the page's origin.
- The session cookie is `SameSite=Lax`, so a page from the APK's origin
  carries none on its socket to the server.
- The socket refuses an `Origin` that `--allowed-origin` does not name.
- The engine is part of each pushed bundle, and the server picks the live
  version, so an engine pinned in the APK would drift from the components it
  draws.

Getting there would take a socket address the host can be handed (plexus-ui),
a credential for the socket other than the cookie (an app key, as synapse
uses), and the app's origin admitted (menger).
