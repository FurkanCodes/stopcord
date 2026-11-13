# Contributing

Thanks for looking. Stopcord is early and has one maintainer, so small, focused changes are the easiest to take.

## Before you start

For anything bigger than a small fix, open an issue first and say what you want to change and why. That saves you writing something that doesn't fit.

## Layout

- `crates/` holds the terminal daemon, screen detection, terminal and sandbox crates.
- `app/src-tauri/src` is the engine and the app's backend.
- `app/src` is the React window. `ui/` holds shared components and design tokens.
- `e2e/` holds tests that drive the real app.
- `docs/` holds design notes. [docs/README.md](docs/README.md) lists them.

## Build and run

See [Install](README.md#install) for what you need. Then, from the repo root:

```sh
npm ci
npm run app
```

`npm run app` starts a debug build of the app that reloads as you edit the window.

## Checks

Run these before you open a pull request. CI runs them too.

```sh
cargo test --workspace
cargo clippy --workspace --all-targets
npm run typecheck
```

The tests in `e2e/` need a built app and open windows, so they aren't part of CI. If your change touches the window or how terminals start, say in the pull request whether you ran them (`npm run e2e`).

## Writing

Text in the app is plain and literal: short sentences, things named by what they are, no jargon. The same goes for error messages and docs.

## Sandbox changes

Changes to `crates/sandbox`, `app/src-tauri/src/runner.rs`, `flow.rs`, `hooks.rs` or `worktree.rs` can change what an agent is able to do. Say in the pull request what an agent can or can't do after your change, and add a test that shows it. If you find a way out of the sandbox, report it privately as described in [SECURITY.md](SECURITY.md) instead of opening a public issue.

## Sign your commits off

This project uses the [Developer Certificate of Origin](https://developercertificate.org/). Add a `Signed-off-by` line to each commit to say you wrote the change or have the right to submit it under the project's license:

```sh
git commit -s
```

Contributions are licensed under the Apache License 2.0, the same as the project.
