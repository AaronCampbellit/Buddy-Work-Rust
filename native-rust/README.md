# Buddy Work for Windows, native Rust prototype

BuddyRust.exe implements the work edition with Win32 controls, native Windows services, local OCR, and an independent per-user profile. It can run alongside the Electron and WebView2 editions. The camera studio uses the same pinned Buddy Camera component as those editions. The Rust app does not use Electron, a WebView2 browser process, or Microsoft Graph. Calendar and Zen browser integration are excluded.

## Features and access

| Feature | Windows or external access |
| --- | --- |
| Compact island, tray, settings and display placement | Uses a per-session hotkey, display information, and foreground window information for fullscreen and app exclusions. Optional startup writes the current user's Run entry. |
| Notes, reminders, Pomodoro, stopwatch, laps and widgets | Stores notes and settings with Windows current-user data protection. Reminders use the local tray. |
| Quick Access, file shelf and Downloads | Reads the signed-in user's Downloads directory and the files or folders they add. Shelf copies are ordinary files in the selected storage location. Opening saved items uses their associated Windows app. |
| Clipboard history, image markup and OCR | Reads the Windows clipboard while services are active. Protected profile files hold retained text and images. Image annotations, exports and Latin-script OCR run locally. Opening or copying a selection is a user action. |
| Media and notifications | Reads compatible Windows media sessions and sends playback controls. Reading other apps’ notifications requires Windows notification access and user consent. The MSIX declares the listener and media capabilities. |
| Audio, camera controls and virtual camera studio | Enumerates endpoints and attached devices. Preview opens the selected camera. Output, screen sharing and virtual camera installation have explicit actions. Camera component registration can require Windows administrator approval. |
| Optional AI Rewrite | A user-initiated rewrite sends the text through a separately installed Codex CLI. It uses a dedicated Buddy sign-in and working directory. The app can be used without signing in. |
| Diagnostics and updates | Records local process counters. An update check contacts the HTTPS manifest specified by BUDDY_UPDATE_MANIFEST_URL when configured. |

Choose a note in the Widgets picker to edit its title and body directly on the island. Use Ctrl+V, Shift+Insert, or right-click and choose Paste. Both editors retain rich-text and HTML clipboard formatting, including emphasis, font sizes and colors. Pasted clipboard images appear inline at the caret and keep their position after reopening. The widget header’s More menu holds reminder, copy and hide actions. Ctrl+click an HTTP or HTTPS link to open it, or select its URL and press Ctrl+Enter. Changes appear in the same note in the Notes workspace.

The EXE installer uses current-user privileges. The packaged desktop app runs with the signed-in user's file access. The MSIX declares runFullTrust, webcam, globalMediaControl and userNotificationListener. A managed Windows deployment needs a signed package from a trusted publisher. The prototype uses its own profile under LocalAppData, separate from the other editions.

## Build and package on Windows x64

The build uses an installed MSVC Rust toolchain, Visual Studio C++ build tools, and Windows SDK. Packaging also uses Node.js, NSIS and the SDK MakeAppx tool. Build helpers are developer dependencies.

From this directory in a Windows command prompt:

```bat
build.cmd build --release --locked
build.cmd test --release --locked
node package.mjs "C:\staging\buddy-native-release" "C:\staging\verified-camera"
```

Supply a new output directory and a camera component matching the repository and commit in ../camera-component.json. The packager checks every camera payload digest. It stages the pinned OCR models and third-party notices and outputs a per-user installer, portable ZIP, unsigned MSIX and unpacked payload. For offline model staging, set BUDDY_OCR_MODELS to a directory containing the two matching ONNX files. BUDDY_NSIS_PATH and BUDDY_MAKEAPPX can point to existing compilers. BUDDY_MSIX_PUBLISHER sets the intended certificate subject before packaging.

The optional CLI adapter test uses the synthetic FakeRewrite.cs fixture compiled as codex.exe in an empty directory. Set BUDDY_FAKE_CLI_DIR to that directory, then use `build.cmd test --release --locked isolated_rewrite_with_standin -- --ignored --test-threads=1`. This test makes no provider requests.

Windows notification listener requirements are described in [Microsoft's notification listener documentation](https://learn.microsoft.com/en-us/windows/apps/develop/notifications/app-notifications/notification-listener).
