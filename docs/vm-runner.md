# The VM runner

The `vm` level runs each terminal in a fresh Linux container in Docker. It's for work you don't trust, such as tickets written by strangers. The code is `app/src-tauri/src/vm.rs`, and what runs inside the image is in `app/src-tauri/vm/`.

## What the container gets

| | |
|---|---|
| Files | A copy of its folder in a Docker volume. Nothing from the Mac is mounted: no home folder, no project, no Docker socket |
| Network | Its own `--internal` network, where it can reach only a relay container. The relay forwards port 3128 to the terminal's web filter and port 3129 to its report port |
| Reports | `st`, a Python script, sends reports with the terminal's key. The key travels in the environment, never on a command line, because `ps` shows those to anything on the Mac |
| Privileges | User `agent` (uid 1000), `--cap-drop ALL`, `no-new-privileges`, a process limit |
| Logins | The `sawtooth-home` volume, shared by every VM agent. A person logs in once, in the first VM terminal |

The probe behind "test this sandbox" checks these from inside the container, with the network and relay an agent gets.

## How work goes in and out

`sawtooth-sync` runs on both sides. A folder's state is its HEAD plus the git tree of everything in it that git doesn't ignore. Trees are content addresses, so the two sides compare exactly whatever their git versions. A record under `~/.sawtooth/vm/records` keeps the state from the last time they matched:

- Only the folder changed: it's copied into the VM.
- Only the VM changed: its work comes back.
- Both changed: nothing is overwritten, and the person is told.
- No record: the VM's copy can't be shown to come from this folder, so it's replaced.

Work crosses as a git bundle. Commits coming back must follow on from the folder's own, except in a throwaway copy, where only kept files go anywhere. Fetches run with `fsck`, which refuses a tree containing `.git`, so a VM can't plant a hook on the Mac. Sawtooth's own files, the knowledge-base links, never cross. The VM gets a plain copy of the knowledge base instead.

The engine syncs when an agent says it's done, when a person treats work as done, before the next round in the same terminal, and when a terminal that isn't on the line ends. Volumes and containers go when the work ends.

## Things learned building it

- `git bundle create` leaves out a branch the other side already has. The folder's state is a commit made on top of HEAD that's never an ancestor of anything, so it always crosses, and its parent says where HEAD is.
- The state's temporary index must be copied with `cp -p`. A fresh time on the index makes git trust a file edited in the same second it last looked, and the edit is missed. Without `-p`, the unit tests failed in 12 of 40 runs.
- `git add` refuses a pathspec naming an ignored file, even an exclude pathspec, so ignored names aren't excluded.
- With `-t`, killing `docker run` leaves its container running. Containers are removed by name before their volumes, because a volume in use can't be removed.
- Tokens restart with each daemon, and a test's daemon runs beside the real one. So names include a hash of Sawtooth's home, and the e2e test only cleans up containers labelled with its own home.

## Limits

- On Linux, Docker shares the kernel, so the wall is thinner than a real VM. The Linux relay, which mounts the filter's and report's Unix sockets, hasn't been run.
- Every VM agent shares `sawtooth-home`. An agent could change a harness's settings there for later agents. They're still inside the VM.
- The knowledge base is read-only in the VM: what an agent writes to it there doesn't come back.
- The image installs Claude Code, Codex, opencode and Gemini CLI from npm. A real harness has only been started with `--version`. Logging in and working inside the VM have been tested only with a stand-in agent.
- Codex's browser login listens on localhost inside the container, which the Mac's browser can't reach. Use its device login or an API key.
