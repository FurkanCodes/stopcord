# Sawtooth

Sawtooth is a desktop app that hands tickets to the coding agents you already use, such as Claude Code. For each repo it runs a line of steps on a separate branch: an agent plans, you approve the plan, an agent builds, the tests run on a clean copy, a second agent reviews, and you decide whether it merges. Every factory, terminal and ticket across your projects is in one window, and the app tells you when something needs you.

> **Early release.** Sawtooth has had one user so far. It runs on macOS. Linux builds from source but hasn't been tested on a real machine. Use it on repos that are pushed to a remote, and read [What it doesn't protect yet](#what-it-doesnt-protect-yet) before you give an agent a repo you care about.

## Words used here

- **Factory**: one project folder in Sawtooth, with its terminals, knowledge base, checks and tickets.
- **Line**: the steps a ticket goes through in a factory. The default line is plan, approve, build, verify, review, handoff.
- **Stage**: one step of a line. An agent, a check, or a person handles it.
- **Gate**: a stage that runs checks, such as your tests, on a clean copy of the ticket's latest commit.
- **Runner**: where a factory's agents run, and how much they are confined. See [Levels](#levels).
- **Harness**: the coding-agent program a stage runs, such as `claude` or `codex`.

## What you need

- macOS. It's tested on macOS 26.
- git, and a repo with at least one commit.
- Rust 1.88 or later (`rustup`), Node.js 22 or later with npm, and Xcode's command line tools (`xcode-select --install`).
- At least one coding-agent CLI, installed and signed in. Built in: Claude Code (`claude`), Codex (`codex`), Gemini CLI (`gemini`), opencode, GitHub Copilot CLI, Cline and Kilo Code. Claude Code is the one tested most. The others are built in but less tested.
- Docker, only if you want the `vm` level.

## Install

Sawtooth isn't signed or notarized yet, so there is no download. Build it from source:

```sh
git clone https://github.com/FurkanCodes/sawtooth.git
cd sawtooth
npm ci
cd app
npx tauri build --bundles app
```

The app is at `target/release/bundle/macos/Sawtooth.app`. Move it to `/Applications` and open it. An app you build yourself isn't blocked by Gatekeeper.

To work on Sawtooth itself, run `npm run app` from the repo root instead. That starts a debug build that reloads as you edit.

## Your first ticket

1. On the home screen, choose **new factory** and pick your repo's folder.
2. Under **start**, choose **line**. Pick the harness that builds and the one that reviews, and keep **runner** at **contained**.
3. Write a small ticket, for example a one-line bug fix with a test.
4. Read the plan the planner writes, then approve it or send it back with a note.
5. Wait. The builder works on its own branch, the gate runs your tests, and the reviewer reads the change. The window shows each step, and asks you only when a stage needs a person.
6. At handoff, read the diff and merge it, or send it back.

## What creating a factory changes in your repo

- **Branches and worktrees.** Each ticket gets a branch named after it, checked out in a worktree under `~/.sawtooth/`. Your own checkout stays as it is until you merge.
- **Git hooks.** The **reviewed** and **line** starts add your tests and a secrets check as checks that run on commit, so Sawtooth installs a `pre-commit` hook that runs them on your own commits too. Checks set to run before a merge get a `pre-push` hook. Every factory also installs a `reference-transaction` hook that stops work in a ticket's worktree from moving the branch your project has checked out. If your repo sets `core.hooksPath` (husky does), or already has a hook with the same name, Sawtooth leaves it alone and says so.
- **The knowledge base.** Sawtooth makes the factory's knowledge file readable as `CLAUDE.md`, `AGENTS.md` and `GEMINI.md`, the names agents read when they start. If your folder doesn't have one of these, it adds a link and lists it in `.git/info/exclude`, so it's never committed. If your folder already has one, it adds a short marked block at the end of that file, which does show up in `git status`. Removing the factory takes both back.
- **Merging.** When you merge, Sawtooth runs `git merge --no-ff` in your project with git hooks off, so hooks a ticket's branch changed don't run. Your own merge hooks don't run for these merges either.

## Levels

A factory's runner sets how much its agents are confined.

| Level | What the agent gets | What's protected |
| --- | --- | --- |
| `worktree` | Its own folder and branch. No sandbox. | Only the git hook above, which stops it moving your checked-out branch. The agent can read and write anything your user can. |
| `contained` (default) | Its own folder and branch, inside the macOS sandbox, with web traffic through a filter. | See below. |
| `vm` | A Linux container in Docker with a copy of the folder. | The container gets no mounts of your folders, no Docker socket, and no network except the web filter. Its work comes back through git. |

At `contained`, an agent:

- can write only to its own worktree, the parts of the repo's `.git` that a commit needs, its harness's own config folders, tool caches, temporary folders, the factory's knowledge file, and folders you add to the sandbox.
- can't read `~/.ssh`, `~/.aws`, `~/.gnupg`, `~/.config/gcloud`, `~/.kube`, `~/.docker/config.json`, `~/.netrc`, `~/.npmrc`, your shell history or Sawtooth's own secrets.
- can't move the branch your project had checked out when its terminal started.
- can't change `.git/hooks` or `.git/config`.
- can reach only the web sites its harness needs, the ones you allow, and ports on your own machine.
- can't reach Sawtooth's own sockets or Docker's.
- can't write `~/.zcompdump` or the `claude` program in `~/.local/share/claude`, which your own shell and Claude Code run later.

Checks run in the same kind of sandbox, with no network unless the check asks for it. In a factory's **setup** tab, **test this sandbox** (under **sandboxes**) tries these from inside the sandbox and shows what was blocked.

Sawtooth's own git commands run with your repo's hooks, fsmonitor and replace refs turned off. They run outside the sandbox, in folders an agent can write to.

## What it doesn't protect yet

These are known gaps at `contained`. They are being worked on, and they're listed so you can decide what to give an agent.

- **Other branches, tags and the stash.** Only the branch your project had checked out is protected. An agent can delete your other branches and drop your stash. Push anything you'd miss.
- **Git history.** An agent can delete objects in `.git/objects` and break the repo's history. A pushed remote is the copy that survives that.
- **Some of Claude Code's config.** An agent can change `~/.claude.json` (including its MCP servers), `~/.claude/plugins`, `~/.claude/agents`, `~/.claude/commands` and `~/.claude/CLAUDE.md`. Your next Claude Code session outside Sawtooth runs or reads them. `settings.json` and `hooks` are protected.
- **Shared caches.** An agent can change `~/.cargo/registry`, `~/.npm` and `~/.cache`. Code in an unpacked crate source there runs at your next `cargo build`.
- **Renaming git's folders.** Whether the sandbox stops an agent renaming `.git/refs/heads` or a worktree's folder in `.git/worktrees` hasn't been tested.
- **Folders that aren't git repos.** Sawtooth allows them, but the agent works in the folder itself with no worktree.
- **Linux.** The Linux sandbox uses bubblewrap. It hasn't been tested on a real machine, and Ubuntu 24.04's default settings may stop bubblewrap from starting.
- **Machines that can't sandbox.** If the sandbox can't start, a contained terminal won't start, but checks run without one.

## Cost

Each ticket runs at least a planner, a builder and a reviewer, each a full session of your harness, billed to your own subscription or API key. The verify gate can send work back to the builder up to 3 times, and the reviewer up to 2 times, so one ticket can take up to about 8 agent sessions. The default line works on 2 tickets at once.

## Status and feedback

This is an early release by one person. If you try it, please open a **First run report** issue saying how far your first ticket got and where it stopped. That's the most useful thing you can send.

Report security problems privately, as described in [SECURITY.md](SECURITY.md).

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md).

## License

Apache License 2.0. See [LICENSE](LICENSE) and [NOTICE](NOTICE).
