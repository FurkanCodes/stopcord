# Real-app tests

These drive the built app through its test socket, with real worktrees, a real daemon and stand-in agents. They catch what unit tests can't, such as the Work screen saying an answer was sent after it failed.

Run them before merging to `main`:

```sh
cd app && npx tauri build --debug --no-bundle && cd ..
npm run e2e
```

The whole run takes about 10 minutes and opens test windows, so leave the machine alone while it runs. To run some of them, name them: `python3 e2e/run.py isolation logos`. The runner refuses a build older than the source.

No test calls a model unless you ask for `real_answer`, which spends quota. Each test gets a throwaway home at `~/.sawtooth-e2e`, or `$SAWTOOTH_E2E_ROOT`, so your factories aren't touched. Run one at a time. Screenshots go to `target/e2e-shots`.

| Test | What it proves |
|---|---|
| `work_screen` | Decisions, diffs, merges, keys, search, and writing a ticket |
| `work_states` | The send-back box, landing, and a gate that keeps failing |
| `answer_failure` | An answer that can't be typed says so, keeps the words, and clears when one lands |
| `isolation` | A misbehaving planner and reviewer change nothing real, with and without a sandbox |
| `sandbox` | Testing a sandbox from setup reports what really held, for each kind of runner |
| `vm` | Agents in Docker work on a copy, only their work comes back, and the copies go when the work ends. Needs Docker running |
| `source_keys` | A connection is set once in settings. A source's settings and keys are set in setup and reach its pull command. A source says whether it can pull, pulls now and shows its whole error, the same on Work. A report sends a test and keeps its keys when renamed |
| `integrations` | The integrations page takes Sentry from nothing to a factory's source with only a token typed: a wrong token says so, the organization and project come from Sentry to pick, and ticking a factory adds the source. Uses a fake Sentry on this machine |
| `markdown`, `logos`, `terminal` | Rendering |

## Stand-in agents

`standins/*.rs` compile to a binary named `claude`, and `line.py` is the same stand-in for the VM, baked into an image, so the daemon recognizes it through its normal rules. `line.rs` plays any role and runs a test's `turn.sh` each turn, so a test can script good or bad behaviour. An agent inside a sandbox can't write outside its folder, so stand-ins report through `st note`.

Stand-ins prove the app's side. They don't prove a real harness starts or behaves, which is what `real_answer` and real tickets are for.
