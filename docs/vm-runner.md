# The VM runner

The `vm` level runs each terminal in a fresh Linux container in Docker. It's for work you don't trust, such as tickets written by strangers. The code is `app/src-tauri/src/vm.rs`, and what runs inside the image is in `app/src-tauri/vm/`.

## What the container gets

| | |
|---|---|
| Files | A copy of its folder in a Docker volume. Nothing from the Mac is mounted: no home folder, no project, no Docker socket |
| Network | Its own `--internal` network, where it can reach only a relay container. The relay forwards port 3128 to the terminal's web filter and port 3129 to its report port |
| Reports | `st`, a Python script, sends reports with the terminal's key. The key travels in the environment, never on a command line, because `ps` shows those to anything on the Mac |
| Privileges | User `agent` (uid 1000), `--cap-drop ALL`, `no-new-privileges`, a process limit |
| Secrets | A sandbox lists environment variable names, and each container gets that sandbox's values for those names, kept in Stopcord's secrets under the factory and sandbox. A bare name would be shared with every sandbox and with Stopcord's own secrets, so an untrusted sandbox could ask for the Slack bot token by naming it. `vm-run` reads them on the Mac and passes `-e NAME` with no value, so no value is ever on a command line, in the spec or in a log. Names Stopcord sets itself, such as the proxy variables or `STOPCORD_*`, are refused, so a secret can't send traffic around the filter |
| Logins | Through a secret: each harness profile suggests the names it can log in with, such as `CLAUDE_CODE_OAUTH_TOKEN` from `claude setup-token`. Without one, a harness asks to log in once in each sandbox's first VM terminal, and that login stays in the sandbox's home volume |
| Home | One volume per sandbox, `stopcord-home-` and a hash of Stopcord's home, the factory and the sandbox. An agent's shell runs what's in its home, so a home shared across sandboxes would let an untrusted agent leave something for a trusted one to run with its secrets |

The probe behind "test this sandbox" checks these from inside the container, with the network and relay an agent gets.

## How work goes in and out

`stopcord-sync` runs on both sides. A folder's state is its HEAD plus the git tree of everything in it that git doesn't ignore. Trees are content addresses, so the two sides compare exactly whatever their git versions. A record under `~/.stopcord/vm/records` keeps the state from the last time they matched:

- Only the folder changed: it's copied into the VM.
- Only the VM changed: its work comes back.
- Both changed: nothing is overwritten, and the person is told.
- No record: the VM's copy can't be shown to come from this folder, so it's replaced.

Work crosses as a git bundle. Commits coming back must follow on from the folder's own, except in a throwaway copy, where only kept files go anywhere. Fetches run with `fsck`, which refuses a tree containing `.git`, so a VM can't plant a hook on the Mac. No git filter runs on either side, because `.gitattributes`, which the VM writes, picks them. Otherwise git-lfs on the Mac would fetch whatever URL the VM named, outside the web filter. If a switch fails partway, the folder goes back to how it was, uncommitted work included, so the next sync can still tell which side changed. A folder syncs one at a time, held by a file lock, because the engine and each terminal's `vm-run` sync from different processes. While it syncs, its agent's container is paused (`docker pause`), because an agent keeps running after it says it's done. A switch only happens if the folder is still what the sync measured, so an edit made on the Mac meanwhile is kept and the sync says so. From then on both sides have changed, which is the truth, and a person settles it. Stopcord's own files, the knowledge-base links, never cross. The VM gets a plain copy of the knowledge base instead. What agents write to that copy comes back on every sync. It's merged with what changed on the Mac meanwhile, keeping every line either side added (`git merge-file --union`), into the factory's knowledge file. The knowledge review then shows it like any agent's edit. A link, a file the project tracks, or anything over 1 MB isn't read back.

The engine syncs when an agent says it's done, when a person treats work as done, before the next round in the same terminal, and when a terminal that was never on the line ends. A line terminal ending doesn't sync, even after its work ended, since that would write into ended work and make its volume again. Volumes and containers go when the work ends.

## Things learned building it

- `git bundle create` leaves out a branch the other side already has. The folder's state is a commit made on top of HEAD that's never an ancestor of anything, so it always crosses, and its parent says where HEAD is.
- The state's temporary index must be copied with `cp -p`. A fresh time on the index makes git trust a file edited in the same second it last looked, and the edit is missed. Without `-p`, the unit tests failed in 12 of 40 runs.
- `git add` refuses a pathspec naming an ignored file, even an exclude pathspec, so ignored names aren't excluded.
- With `-t`, killing `docker run` leaves its container running. Containers are removed by name before their volumes, because a volume in use can't be removed.
- Tokens restart with each daemon, and a test's daemon runs beside the real one. So names include a hash of Stopcord's home, and the e2e test only cleans up containers labelled with its own home.

## Why logins come in as secrets

Copying a harness's login from the Mac doesn't work generally. Each harness keeps it somewhere else (Claude Code in the Keychain, Codex in a file), and most logins renew by cancelling the old key, so a copy shared with the Mac logs one side out. What every harness accepts is an environment variable, so a VM gets its own token or key, which can be cancelled alone. The profile's suggested names are data, so a harness Stopcord doesn't know works by typing its variable's name.

## Docker

Stopcord doesn't install Docker. When it's missing, the sandbox settings say so and offer Docker's install page, or, on a Mac with Homebrew, `brew install --cask docker` in a terminal the person watches. When it's installed but stopped, they offer to start it and keep looking until it answers. `STOPCORD_DOCKER` names the `docker` program outright.

The image is built on first use and named by a hash of its files. After a build, older images of Stopcord's own, named by a hash, are removed, but not forcibly, so one a running container uses stays. `latest` and a person's own tags are never touched.

## Limits

- On Linux, Docker shares the kernel, so the wall is thinner than a real VM. The Linux relay, which mounts the filter's and report's Unix sockets, hasn't been run.
- Any agent in a VM can read the secrets it's given, since its harness needs them. Use a token made for the VM, which can be cancelled alone. Keeping keys out of the VM entirely would mean the filter adding them to requests, which needs it to open TLS.
- A sandbox's agents share its home. An agent can change a harness's settings there for later agents of the same sandbox, which get the same secrets.
- A removed sandbox's home volume stays, with any login in it, until it's removed with `docker volume rm`. So does the `stopcord-home` volume every VM agent shared before homes were per sandbox.
- A knowledge-base edit from the VM is live for every agent before a person reviews it, as any agent's edit is. For untrusted work that's a way to reach trusted agents, and closing it means agents reading only the approved copy.
- The image installs Claude Code, Codex, opencode and Gemini CLI from npm. A real harness has only been started with `--version`. Logging in and working inside the VM have been tested only with a stand-in agent.
- Codex's browser login listens on localhost inside the container, which the Mac's browser can't reach. Use its device login or an API key.
- A switch is refused when the folder changed after the sync looked at it, but the check and the switch are separate steps, so an edit in the moment between them can still be lost. If the app quits mid-sync, the agent's container stays paused until its terminal is closed, which removes it.
- A step of a sync that takes over 10 minutes is stopped: its process group is killed and its container removed. So a VM that stalls one, such as with a named pipe where its knowledge file was, holds its folder up to 10 minutes, not forever. The size of a bundle coming back isn't capped, and it's held in memory, so a VM can make the app use a lot of it.
- The setup marker is in the Mac's folder, so a VM copy made again, such as after its volume was removed, starts without its setup.
