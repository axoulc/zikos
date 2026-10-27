# zikos_app

A new Flutter project.

## Linux desktop prerequisites

Besides `flutter doctor`'s Linux toolchain (clang, cmake, ninja, pkg-config),
the Rust core pulls in system libraries that need their development headers:

```bash
sudo apt install libasound2-dev   # cpal/alsa-sys — audio capture
```

GTK development packages (`libgtk-3-dev`) come with the usual
`flutter create` Linux setup.

## Getting Started

This project is a starting point for a Flutter application.

A few resources to get you started if this is your first Flutter project:

- [Lab: Write your first Flutter app](https://docs.flutter.dev/get-started/codelab)
- [Cookbook: Useful Flutter samples](https://docs.flutter.dev/cookbook)

For help getting started with Flutter development, view the
[online documentation](https://docs.flutter.dev/), which offers tutorials,
samples, guidance on mobile development, and a full API reference.
