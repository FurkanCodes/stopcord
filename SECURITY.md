# Security

Stopcord runs coding agents on your machine, so a way for an agent to get past its sandbox is the most serious kind of bug here.

## Reporting a problem

Please report security problems privately, through GitHub: on this repo's **Security** tab, choose **Report a vulnerability**. Don't open a public issue for them.

Say what an agent can do that it shouldn't, at which level (`worktree`, `contained` or `vm`), on which OS, and the steps to reproduce it.

You'll get a reply within 7 days. Stopcord has one maintainer, so a fix may take longer than that. You'll be told what's planned, and credited in the fix unless you'd rather not be.

## What counts

Anything that breaks a claim in the README's [Levels](README.md#levels) section counts. Some examples:

- An agent at `contained` writing outside the places listed there, or reading a blocked path.
- An agent at `contained` moving the project's checked-out branch.
- Code an agent wrote running outside the sandbox before you merge.
- An agent reaching Stopcord's own sockets or Docker's.

The gaps listed under [What it doesn't protect yet](README.md#what-it-doesnt-protect-yet) are already known. A report that shows one of them is worse than described is still welcome.
