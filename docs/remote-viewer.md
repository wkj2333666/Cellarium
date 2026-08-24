# Remote viewer

Cellarium keeps its original direct mode for local GPU machines. For an SSH
session, use the C1 split mode so the server runs the simulation and the local
terminal renders the scalar field:

```sh
# on tinker (or the target server)
./scripts/install-local.sh

# on the local machine
cellarium connect tinker
```

The server executable is installed at `$HOME/.local/bin/cellarium`. The
connect command starts `$HOME/.local/bin/cellarium server` through SSH. Only
scalar snapshots and input events cross SSH; Kitty graphics escape sequences
are generated locally, so a Kitty-capable terminal keeps high-precision output
without sending image pixels through SSH. On native local Unix Kitty terminals,
Cellarium transfers each opaque RGB frame through POSIX shared memory and sends only
a small graphics command through the terminal PTY. This keeps terminal output
from blocking keyboard or mouse input at high resolutions. Cellarium waits for
Kitty to unlink each consumed object; if consumption stalls or shared-memory
allocation fails, it switches to inline Kitty graphics. Other compatible
terminals use inline graphics directly, and half-block rendering remains the
last-resort fallback when no graphics protocol is available.

Remote status distinguishes `server sim`, `snapshot rx`, `UI draw`, and
`fresh graphics`. The first two are measured from server work and decoded
snapshots; `UI draw` counts completed local terminal draws, while
`fresh graphics` counts only draws that publish a newly encoded viewport.
For C1 graphics, viewport rasterization also runs in a latest-frame worker:
slow clients replace obsolete raster requests instead of blocking input or
building a latency queue. Direct local rendering keeps its synchronous path.
The editor's `server step` timing comes from the server, while `UI draw` timing
is local. The remote backend name, rule, tick, and input acknowledgement are
also authoritative snapshot values rather than placeholders from the local
mirror.

Protocol versions must match, so update the local and remote executable
together. To run the maintained hybrid end-to-end check against an SSH alias:

```sh
./scripts/e2e-tinker.sh tinker
```

The script uses an optimized local client but never benchmarks local
simulation: protocol simulation/step rates come exclusively from the remote
server. The terminal report separately records the actual client-side Kitty
frame-consumption cadence and server-confirmed input-to-frame latency.

The protocol report measures the tinker GPU, tick/snapshot cadence, and
input-to-state latency. The terminal report uses a small local PTY to consume
real Kitty shared-memory frames and verify keyboard/mouse-to-frame behavior;
its frame cadence is diagnostic and is not a benchmark of the server.

Cellarium uses the system `ssh` command by default. This keeps the protocol
stdin/stdout as a transparent byte stream while the local viewer independently
detects Kitty graphics. To use a different connector, set it explicitly:

```sh
CELLARIUM_SSH_COMMAND='ssh -F /path/to/ssh_config' cellarium connect tinker
```

The equivalent CLI option is:

```sh
cellarium connect tinker --ssh-command 'ssh -F /path/to/ssh_config'
```

If the terminal has no graphics protocol, the existing half-block fallback is
selected automatically. Direct mode still uses local GPU/CUDA selection and
defaults to graphics when Kitty, iTerm2, or Sixel support is detected.
