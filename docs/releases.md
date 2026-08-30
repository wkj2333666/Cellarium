# Release installation

Each Cellarium release provides one executable per operating system and CPU
architecture. There is one program and one mode: it opens a window and runs the
simulation in the same process.

The `v0.4.1` assets are:

```text
cellarium-v0.4.1-linux-x86_64.tar.gz
cellarium-v0.4.1-linux-aarch64.tar.gz
cellarium-v0.4.1-macos-x86_64.tar.gz
cellarium-v0.4.1-macos-aarch64.tar.gz
cellarium-v0.4.1-windows-x86_64.zip
cellarium-v0.4.1-windows-aarch64.zip
SHA256SUMS
```

Verify a downloaded archive against `SHA256SUMS` before installing it. The
supplied installer does this for you:

```sh
./scripts/install-gui-local.sh cellarium-v0.4.1-linux-x86_64.tar.gz SHA256SUMS
```

## What each release contains

Linux releases contain the CUDA, wgpu and CPU backends. macOS and Windows
releases contain wgpu and CPU. Cellarium chooses at startup and falls back on
its own, so one executable works whether or not the machine has a usable GPU.

The Backend panel inside the window lists every device found and the reason any
of them was rejected. If a GPU probe is what hangs on a particular machine,
start with `--safe-mode` and change the setting from inside the window.

## Linux

```sh
tar -xzf cellarium-v0.4.1-linux-x86_64.tar.gz
install -Dm755 cellarium "$HOME/.local/bin/cellarium"
install -Dm644 cellarium.desktop "$HOME/.local/share/applications/cellarium.desktop"
```

Use the `aarch64` archive on ARM64 systems, including Jetson systems with a
compatible CUDA installation.

A Linux machine needs the usual window and rendering runtime libraries. On
Debian and Ubuntu:

```sh
sudo apt-get install libx11-6 libxcursor1 libxrandr2 libxi6 \
  libxkbcommon0 libxkbcommon-x11-0 libwayland-client0 libgl1 mesa-vulkan-drivers
```

## macOS

Choose the x86_64 archive for Intel Macs or the aarch64 archive for Apple
Silicon.

```sh
tar -xzf cellarium-v0.4.1-macos-aarch64.tar.gz
chmod +x cellarium
mkdir -p "$HOME/.local/bin"
mv cellarium "$HOME/.local/bin/cellarium"
```

macOS uses the wgpu backend over Metal, and the CPU backend when no usable
device is present.

## Windows

Choose the x86_64 or aarch64 zip for the machine, extract `cellarium.exe`, and
move it into a directory listed in `PATH`. Windows uses the wgpu backend over
DirectX 12 or Vulkan, and the CPU backend otherwise.

## Where Cellarium keeps its files

Settings and a recovery autosave live under `$XDG_DATA_HOME/cellarium`, or
`~/.local/share/cellarium` when that is not set. Experiments are saved wherever
you choose. Every file is written to a temporary and renamed into place, so an
interrupted save leaves the previous file intact.
