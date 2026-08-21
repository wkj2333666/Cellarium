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
Cellarium transfers each RGBA frame through POSIX shared memory and sends only
a small graphics command through the terminal PTY. This keeps terminal output
from blocking keyboard or mouse input at high resolutions. Cellarium waits for
Kitty to unlink each consumed object; if consumption stalls or shared-memory
allocation fails, it switches to inline Kitty graphics. Other compatible
terminals use inline graphics directly, and half-block rendering remains the
last-resort fallback when no graphics protocol is available.

The `render` rate shown by `connect` is measured by the local viewer. It is not
the server's configured snapshot target, so it reflects graphics work the
client is actually scheduling.

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
