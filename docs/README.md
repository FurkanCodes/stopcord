# Docs

Notes on how Sawtooth is built and why. Each says whether it describes code that exists or a plan.

## How it works

- [terminal-daemon.md](terminal-daemon.md): the daemon that owns every terminal and reads what the agent in each one is doing. Built.
- [vm-runner.md](vm-runner.md): the `vm` level, which runs each terminal in a Linux container in Docker. Built.
- [sandbox-spike.md](sandbox-spike.md): the experiment that chose Seatbelt on macOS and bubblewrap on Linux for the `contained` level. Historical: the code has moved on since.

## Designs

- [designs/first-ticket-onboarding.md](designs/first-ticket-onboarding.md): taking a new person from an empty home screen to one ticket through the default line. Approved, mostly not built yet.
- [designs/first-ticket-onboarding-brief.md](designs/first-ticket-onboarding-brief.md): the concept review that design came from.

## The safety model in one paragraph

At the `contained` level, each agent works in its own git worktree inside an OS sandbox: Seatbelt on macOS, bubblewrap on Linux. It can write only to its worktree, the parts of `.git` a commit needs, its harness's config folders, caches and temporary folders, and it can't read the usual credential folders. The branch the project has checked out is read-only to it. Checks run in the same kind of sandbox. Sawtooth's own git commands run outside the sandbox with the repo's hooks, fsmonitor and replace refs off, and merges happen only when a person chooses. The README lists what this doesn't cover yet.
