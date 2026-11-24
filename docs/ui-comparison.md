# Windows UI comparison

The left column shows the original work-edition web interface rendered by Microsoft Edge on Windows 11. The right column shows the native Rust interface on the same VM. The original layout captures were taken with beta.45. The Audio and camera screenshots were refreshed in beta.46. Open either image at full size to inspect the text, spacing and controls.

The captures use the same synthetic notes, links, image, timer values and pinned websites. The expanded island is 840 × 376 at 100% scale. The pinned note sits beside the collapsed island in its own 400-pixel-wide window. Both editions use the custom center label “Buddy.”

| Web reference | Native Rust |
| --- | --- |
| **Notes** | **Notes** |
| [![Web Notes with a selected note and an inline image](comparison/web-notes.png)](comparison/web-notes.png) | [![Native Notes with the same note and inline image](screenshots/notes.png)](screenshots/notes.png) |
| **Pinned note** | **Pinned note** |
| [![Web pinned note](comparison/web-pinned-note.png)](comparison/web-pinned-note.png) | [![Editable native pinned note](screenshots/pinned-note.png)](screenshots/pinned-note.png) |
| **Clipboard** | **Clipboard** |
| [![Web Clipboard in dark mode](comparison/web-clipboard.png)](comparison/web-clipboard.png) | [![Native Clipboard in dark mode](screenshots/clipboard.png)](screenshots/clipboard.png) |
| **Quick Access** | **Quick Access** |
| [![Web Quick Access with three pinned sites](comparison/web-quick.png)](comparison/web-quick.png) | [![Native Quick Access with the same three sites](screenshots/quick.png)](screenshots/quick.png) |
| **Timers and widgets** | **Timers and widgets** |
| [![Web Pomodoro and stopwatch](comparison/web-timers.png)](comparison/web-timers.png) | [![Native Pomodoro and stopwatch](screenshots/timers.png)](screenshots/timers.png) |
| **Home** | **Home** |
| [![Web Home view](comparison/web-home.png)](comparison/web-home.png) | [![Native Home view](screenshots/home.png)](screenshots/home.png) |
| **Pinned clipboard** | **Pinned clipboard** |
| [![Web Pinned clipboard view](comparison/web-pinned-clipboard.png)](comparison/web-pinned-clipboard.png) | [![Native Pinned clipboard view](screenshots/pinned-clipboard.png)](screenshots/pinned-clipboard.png) |
| **Audio input** | **Audio input** |
| [![Web Audio input view](comparison/web-devices.png)](comparison/web-devices.png) | [![Native Audio input view](screenshots/devices.png)](screenshots/devices.png) |
| **Physical camera** | **Physical camera** |
| [![Web Physical camera view](comparison/web-devices-camera.png)](comparison/web-devices-camera.png) | [![Native Physical camera view](screenshots/camera.png)](screenshots/camera.png) |
| **Virtual camera** | **Virtual camera** |
| [![Web Virtual camera view](comparison/web-devices-virtual-camera.png)](comparison/web-devices-virtual-camera.png) | [![Native Virtual camera view](screenshots/studio.png)](screenshots/studio.png) |
| **General settings** | **General settings** |
| [![Web General settings view](comparison/web-settings.png)](comparison/web-settings.png) | [![Native General settings view](screenshots/settings.png)](screenshots/settings.png) |
| **Appearance** | **Appearance** |
| [![Web Appearance view](comparison/web-settings-appearance.png)](comparison/web-settings-appearance.png) | [![Native Appearance view](screenshots/appearance.png)](screenshots/appearance.png) |

The previous Rust release put clipboard history in a list and showed one selected preview. The new layout exposes the thumbnail, type, time and actions on each card. [Clipboard in beta.44](comparison/clipboard-beta44.png)

## Visible differences

Windows RichEdit supplies the native text caret, link styling, selection and formatted paste. The Windows scrollbars, date picker, font rasterization and corner edges can also differ between the two renderers. The Clipboard image occupies the same 185 × 105 pixels in both captures. In the main Notes editor the inline thumbnail starts two pixels farther left and three pixels higher in Rust. The audio and camera screens show the device state each app reported on the test VM. Live camera video and active-call controls were outside this comparison.

## Windows editions compared

On the same Windows 11 x64 VM, we timed each app from launch until we could create a note, edit its title and read it back. The timings and private committed memory figures are medians of seven launches with an existing profile. We measured memory across the whole process tree 800 ms after the edit.

| Edition | Installer | Installed app | Launch to verified edit | Private memory |
| --- | ---: | ---: | ---: | ---: |
| Electron beta.41 | 155.14 MB | 504.15 MB | 1.78 s | 332.29 MiB |
| WebView2 beta.41 | 24.05 MB | 49.54 MB | 3.45 s | 134.09 MiB |
| Native Rust beta.49 | 24.08 MB | 45.43 MB | 1.13 s | 3.24 MiB |

The native installation uses 91.0% less space than Electron. App sizes exclude user data, the optional shared camera component and the system WebView2 runtime. Electron used its usual services. The WebView2 and Rust runs used isolated profiles with global listeners disabled. The timings apply to this Notes workflow on this VM.

We also profiled the web editions with **fast-anything v0.8.6**. The busiest Notes code relied on the browser’s current text selection. Neither profiled edition yielded a function that met the tool’s criteria for a Rust port. Separate JavaScript changes produced mixed results across paired runs, so both web builds kept their original code. [Tested tool release](https://github.com/EL4CTEO/fast-anything/tree/ba571858d222836843fcd0bccbd94a4290c36ac2)

## Earlier beta.45 measurements

We launched each edition on a signed-in Windows 11 VM, opened Notes, created a draft, and read the edited title back. Each edition used its own profile. Global listeners were disabled and Downloads used test data. The table shows the median of seven alternating launches with an existing profile. Memory is private committed memory across the process tree, measured 800 ms after the edit. [Raw measurements](comparison/notes-performance.csv)

| Notes benchmark | Native Rust beta.45 | WebView2 beta.41 |
| --- | ---: | ---: |
| Launch to verified edit | 1.14 s | 2.97 s |
| Private committed memory | 3.20 MiB | 141.43 MiB |
| Processes | 1 | 8 |

The first launch with a fresh profile took 1.59 s in Rust and 3.31 s in WebView2.

Across six paired launches, the native island began changing width 47 ms after the open command, down from 65 ms in beta.44. The first width change fell from 384 to 197 pixels. The median interval between visible opening steps was 16 ms and the 95th percentile was 28 ms. The island returned to its intended position through 72 interrupted transitions. Keyboard focus, hover re-entry and reduced motion also passed on Windows.
