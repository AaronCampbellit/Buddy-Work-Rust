# Buddy Work for Windows

Buddy is a local-first desktop island for Windows x64. Pin a note beside the island to edit it while you work. Paste formatted text, links, and images with Ctrl+V or the right-click menu. The widget and main Notes editor open the same encrypted note.

## Download

[Download the Windows installer](https://github.com/AaronCampbellit/buddy-work-windows/releases/download/v0.1.1-beta.44/Buddy-Work-Rust-Setup-x64.exe) or [browse the beta.44 release](https://github.com/AaronCampbellit/buddy-work-windows/releases/tag/v0.1.1-beta.44) for the portable ZIP, unsigned MSIX, and SHA-256 checksums.

The installer runs per user and is unsigned. Beta.44 was tested on Windows 11 x64, including an upgrade from beta.43. The installed app folder occupied 44.94 MB on the test machine.

## Build

The Windows app is written in Rust and uses native Win32 controls. Build from the `native-rust` directory. To run the packager, extract the matching portable ZIP from the release and pass its `Buddy\camera` directory as the pinned camera payload. [Build instructions and feature permissions](native-rust/README.md)
