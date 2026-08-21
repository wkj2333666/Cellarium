# Cellarium

Cellarium is a GPU-accelerated cellular automata laboratory with an interactive
terminal UI. It supports Conway-style automata, Lenia/Orbium, editable kernels,
custom rule programs, and CPU/CUDA simulation backends.

Cellarium remains one executable with three launch modes:

```sh
cellarium                  # simulate and render in the current terminal
cellarium server           # headless C1 simulation server
cellarium connect tinker   # local renderer connected to a remote server
```

Graphics-capable terminals such as Kitty use high-precision graphics rendering
by default. For SSH sessions, `connect` renders locally so keyboard and mouse
input are not blocked by image frames crossing SSH. See
[Remote viewer](docs/remote-viewer.md).

## Build and install

On a Linux CUDA machine, the default build includes both CUDA and CPU backends
and selects one at runtime:

```sh
cargo build --release
./scripts/install-local.sh
```

To build without CUDA support:

```sh
cargo build --release --no-default-features
```

Prebuilt binaries for Linux, macOS, and Windows on x86_64 and ARM64 are
described in the [release installation guide](docs/releases.md).
