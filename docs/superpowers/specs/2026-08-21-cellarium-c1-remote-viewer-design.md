# Cellarium C1 Remote Viewer Design

## Goal

Keep the existing direct terminal mode for machines that can run the simulation and render locally, while adding a C1 split mode for SSH sessions:

- cellarium server runs the simulation headlessly on the server (CUDA when available, CPU fallback).
- cellarium connect <host> starts that server through SSH and runs the terminal viewer locally.
- The local viewer owns terminal raw mode, keyboard/mouse capture, local rasterization, and Kitty graphics/half-block output.
- The server sends scalar-field snapshots and accepts control/edit events over a framed stream. Kitty graphics bytes never cross the SSH connection.

This removes the current failure mode where a remote Kitty graphics frame blocks the input loop, while retaining high precision and the existing direct renderer on capable local machines.

## Decisions

1. The C1 stream is a binary, length-prefixed protocol over the SSH process stdin/stdout. It is independent of the terminal escape stream.
2. Snapshots are latest-wins and bounded by a maximum payload size. The server may skip intermediate frames when transport is slower than simulation.
3. The snapshot contains scalar cells plus authoritative tick/rate/status metadata. The viewer rasterizes the received scalar field at its local viewport resolution.
4. Input is sent immediately and is not coupled to the render cadence. Keyboard commands, expression-edit text, mouse actions, and resize/camera updates are separate protocol messages.
5. connect defaults to the system ssh executable and supports CELLARIUM_SSH_COMMAND for installations that require kitten ssh; the command is split as an executable plus arguments, then the host and remote command are appended.
6. server is terminal-free and never enables raw mode, alternate screen, mouse capture, or graphics protocols.
7. Direct mode remains the default when no subcommand is supplied. Its existing backend selection and local graphics behavior stay intact: Kitty/iTerm2/Sixel-capable terminals default to graphics, with CELLARIUM_REMOTE_GRAPHICS=0/1 overrides preserved.
8. The supported server install path is $HOME/.local/bin/cellarium, with cargo install --path . --root "$HOME/.local" as the canonical install command.

## Compatibility and limits

- Existing --kernel, --experiment, and --save-experiment direct-mode flags remain valid.
- The first protocol version supports one scalar channel and the existing App commands/mouse actions. Unknown versions/messages fail with a concise error and do not corrupt the terminal.
- Snapshot payloads are intentionally raw f32 cells in C1 for correctness and simple local rendering. A later C2 can add compression/delta encoding without changing the viewer API.
- If the local terminal has no graphics protocol, the viewer automatically uses the existing half-block fallback. If it supports Kitty graphics, graphics is selected by default.

## Acceptance criteria

- cellarium server can be started by SSH without terminal escape sequences.
- cellarium connect tinker captures local keyboard and mouse events while the remote simulation continues.
- A space key pauses/resumes, N steps, reset/randomize/clear/rule/kernel/editor commands are forwarded, and viewport mouse actions are forwarded.
- Direct mode still starts and keeps its current graphics detection behavior.
- cargo test, protocol tests, and CLI tests pass.
- The release binary can be installed at /home/wkj/.local/bin/cellarium on tinker and is discoverable by command -v cellarium.
