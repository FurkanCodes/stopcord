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
| `isolation` | A misbehaving planner and reviewer change nothing real, with and without a sandbox |
| `sandbox` | Testing a sandbox from setup reports what really held, for each kind of runner |
| `vm` | Agents in Docker work on a copy, only their work comes back, and the copies go when the work ends. Needs Docker running |
| `source_keys` | A connection is set once in settings. A source's settings and keys are set in setup and reach its pull command. A source says whether it can pull, pulls now and shows its whole error, the same on Work. A report sends a test and keeps its keys when renamed. Setup opens on what a factory needs, with the rest folded under tuning, and a renamed stage keeps what points at it |
| `integrations` | The integrations page takes Sentry from nothing to a factory's source with only a token typed: a wrong token says so, the organization and project come from Sentry to pick, and ticking a factory adds the source. Uses a fake Sentry on this machine |
| `oauth` | Signing in to Sentry (device flow) and Linear (PKCE) in the browser, with nothing pasted, against a fake of both: the code waits for approval, a callback with the wrong state is refused, an expired token refreshes by itself, and signing out forgets the tokens |
| `templates` | A factory is saved as a template from its setup, without its project. A new factory made from it has the same line and roles on the same models. A template goes out as a file and comes back |
| `roles_talk` | Each stage's note reaches every stage after it, the builder hands work back to the planner with `st send` and the planner gets the message, and handoff shows the notes from each stage |
| `answers_once` | A reviewer and a scorer that answer once get the diff and the notes with no terminal, the reviewer's clear finding sends the work back to the builder, the scorer's last line becomes its score, and setup shows how each role runs |
| `race` | Three builders race at build, each in its own copy and terminal; the one that fails the tests is out, a judge that answers once picks between the two that pass, only the winner's commits reach the branch, and the racers' terminals and copies are cleaned up |
| `panel` | The planner and a critic take turns at plan with `st say`, each hearing the other in its own terminal; a judge that answers once scores each round, one ticket settles in round two with its plan kept, and one that never settles asks a person, who answers the last speaker |
| `toolbox` | Picking a trigger, a check or a terminal from the map's parts drawer closes the drawer and opens what was added, with the cursor in its first field |
| `inbox` | A new factory with a line has an inbox on its map. Work written there becomes a ticket at the start of the line, tagged as from the inbox and listed in it, and the drawer offers no second one |
| `needs_terminal` | A terminal whose agent asks a question turns red itself, and the screen adds no second box to answer in |
| `asks` | An agent on the line that asks through `st ask`: Work shows the question and sends you to its terminal with enter, the terminal is red, and an answer typed there reaches the agent, which carries on |
| `remove_factory` | Removing a factory asks what to take with it: its terminals, worktrees, branches (merged ones ticked, unmerged ones not), keys and files. What's ticked goes; unmerged work, shared sign-ins and the project stay |
| `new_factory` | New factory picks harness, model and effort from dropdowns for the builder and the reviewer, and the factory's agents run what was picked |
| `markdown`, `logos`, `terminal` | Rendering |

## Stand-in agents

`standins/*.rs` compile to a binary named `claude`, and `line.py` is the same stand-in for the VM, baked into an image, so the daemon recognizes it through its normal rules. `line.rs` plays any role and runs a test's `turn.sh` each turn, so a test can script good or bad behaviour. An agent inside a sandbox can't write outside its folder, so stand-ins report through `st note`.

Stand-ins prove the app's side. They don't prove a real harness starts or behaves, which is what `real_answer` and real tickets are for.
