# Sandbox spike

Run 2026-11-09 on macOS 26.5 (Apple silicon) and in a throwaway Ubuntu 26.04 VM (kernel 7.0, bubblewrap 0.11.1).
This is stage 0 of the factory roadmap. It decides what the contained runner is built on.

## Result

One default-deny policy works on both systems. On Mac it becomes a Seatbelt profile run through
`sandbox-exec`. On Linux it becomes a bubblewrap command. Both send web traffic through a local filter
that allows named domains.

## Harnesses under the Mac sandbox

Each was asked, one-shot, to create a file. Its own sandbox was off, because a sandbox can't run inside another.

| Harness | Result | Domains it used |
|---|---|---|
| Claude Code | Works. Wrote only in its worktree | `api.anthropic.com`, `mcp-proxy.anthropic.com`, `*.datadoghq.com` |
| Codex | Works. Wrote only in its worktree | `chatgpt.com`, `ab.chatgpt.com` |
| opencode | Works once its provider is allowed. Blocked first, which is the correct behaviour | `openrouter.ai`, `models.dev` |
| Gemini CLI | **Not tested.** It isn't logged in on this machine and asks for a browser login even outside the sandbox | `oauth2.googleapis.com` at startup |

opencode's domains depend on the provider the person configured, so a harness profile can't list them all.
Blocked sites have to reach the person as an allow-once, always or no question.

## Real work under the Mac sandbox

| Test | Result |
|---|---|
| `npm install` with a package that runs a postinstall binary | Works |
| `npm test` | Works |
| `cargo build` with a crate from crates.io | Works |
| `git commit` in a worktree | Works |
| A local test server on loopback | Works |

## Escapes

| Attempt | Mac | Linux |
|---|---|---|
| Write to the real project | Denied | Denied |
| Write to the home folder | Denied | Denied |
| Edit `.git/hooks` or `.git/config` | Denied | Denied |
| Read `~/.ssh` | Denied | Denied |
| Edit harness settings | Denied | Not run |
| Launch an app with `open` or `osascript` | Denied | Not applicable |
| Connect to a Unix socket such as Docker's or the daemon's | Denied | Denied **only when masked** |
| Reach the internet without the filter | Denied | Denied |
| Reach a site the filter doesn't allow | Denied | Denied |

On Linux, `npm install` through the filter, an allowed site, a worktree write and a commit all worked.

## What the spike changed

- **Linux must mask sockets by path.** A read-only bind mount does not stop a process connecting to a Unix
  socket on it. With no mask, the test connected to the fake daemon socket. Every socket that matters has to be
  covered, and `/run` replaced with a tmpfs.
- **Linux has no network in the sandbox at all**, so the filter is reached through a Unix socket and a small
  forwarder inside the sandbox.
- **Mac can't scope loopback.** Test servers need it, so loopback stays open and known local ports, such as the
  webhook listener's, are denied by number. Linux doesn't have this gap, because the network namespace is empty.
- **`/tmp` is writable on Mac**, so a project kept under `/tmp` isn't protected. Projects under a home folder are.

## The folder-trust question

Claude Code still asks in a child of a folder it already trusts, so trusting a parent doesn't help. Codex and
opencode don't ask. Sawtooth shows the question in NEEDS, and a per-harness setting can answer it automatically.
The setting is off by default, because the question exists to stop a repo's own hooks from running unseen.

## Limits

- Harnesses were only run one-shot on small folders, not through a long interactive session.
- No harness was run on Linux, because the VM has no logins. Only the sandbox and filter were tested there.
- Apple has deprecated `sandbox-exec`. It still works on macOS 26, and Codex and Cursor rely on it.
