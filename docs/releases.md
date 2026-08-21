# Release installation

Each Cellarium release provides one executable per operating system and CPU
architecture. The program is not split into viewer/server or CPU/CUDA tools:
direct mode, `server`, and `connect` are all available from the same executable.

The `v0.1.3` assets are:

```text
cellarium-v0.1.3-linux-x86_64.tar.gz
cellarium-v0.1.3-linux-aarch64.tar.gz
cellarium-v0.1.3-macos-x86_64.tar.gz
cellarium-v0.1.3-macos-aarch64.tar.gz
cellarium-v0.1.3-windows-x86_64.zip
cellarium-v0.1.3-windows-aarch64.zip
SHA256SUMS
```

Verify a downloaded archive against `SHA256SUMS` before installing it.

## Linux

Linux releases contain both CPU and CUDA backends. At startup Cellarium tries
to load CUDA dynamically; if the NVIDIA driver, NVRTC, or a usable device is
unavailable, that same executable automatically falls back to CPU.

For example, install the x86_64 release into the user executable directory:

```sh
tar -xzf cellarium-v0.1.3-linux-x86_64.tar.gz
install -Dm755 cellarium "$HOME/.local/bin/cellarium"
```

Use `cellarium-v0.1.3-linux-aarch64.tar.gz` on ARM64 systems, including Jetson
systems with a compatible CUDA installation.

## macOS

Choose the x86_64 archive for Intel Macs or the aarch64 archive for Apple
Silicon. Extract it, make the executable runnable, and place it on `PATH`:

```sh
tar -xzf cellarium-v0.1.3-macos-aarch64.tar.gz
chmod +x cellarium
mkdir -p "$HOME/.local/bin"
mv cellarium "$HOME/.local/bin/cellarium"
```

macOS releases use the CPU backend.

## Windows

Choose the x86_64 or aarch64 zip for the machine, extract `cellarium.exe`, and
move it into a directory listed in `PATH`. Windows releases use the CPU
backend.
