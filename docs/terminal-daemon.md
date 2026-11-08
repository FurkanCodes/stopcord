# The terminal daemon

`sawtooth daemon` owns every terminal. Terminals keep running when no window is open. The daemon reads each screen and decides what the agent in it is doing, using the rule files in `crates/detect/rules/`.

It listens on `$SAWTOOTH_SOCKET`, or `~/.sawtooth/sawtooth.sock`. The socket file is mode 0600 and its folder is 0700. Rule files in `~/.sawtooth/rules/*.toml` replace the bundled ones with the same `id`.

## States

| `state` | Meaning |
| --- | --- |
| `working` | Busy with a turn. |
| `idle` | Waiting for the next instruction. |
| `blocked` | Stopped on a question only a person can answer. |
| `unknown` | No agent Sawtooth knows is in front: a plain shell. |
| `exited` | The program ended. `exit_code` says how. |

`needs_you` is true when the terminal is `blocked`, or when it has finished a turn nobody has looked at (`unseen`). A turn counts only after something was typed into the terminal, so startup spinners never count. A working agent must look idle for 1.2 s before its turn counts as finished.

## Protocol

Newline-delimited JSON. Send `{"id":"1","method":"…","params":{…}}`. You get back `{"id":"1","result":…}` or `{"id":"1","error":{"code":"…","message":"…"}}`.

| Method | Params | Result |
| --- | --- | --- |
| `ping` | | `{pong, version}` |
| `daemon.info` | | version, socket, agent ids, `rule_problems`, terminal count |
| `daemon.shutdown` | | ends every terminal and stops the daemon |
| `terminal.start` | `argv` (required), `cwd`, `name`, `agent`, `env` {k: v}, `rows`, `cols` | the terminal |
| `terminal.list` | | `{terminals: [...]}` |
| `terminal.get` | `id` | the terminal |
| `terminal.read` | `id`, `lines` | `{text, title, rows, cols, alternate}` |
| `terminal.redraw` | `id` | `{data, seq, rows, cols}`: base64 escape sequences that repaint the screen, for a viewer joining late. `seq` is the last output chunk they include. |
| `terminal.send` | `id`, `text` and/or `data` (base64), `enter` | `{sent}`. Counts as input, and as looking. |
| `terminal.seen` | `id` | the terminal, with `unseen` cleared |
| `terminal.rename` | `id`, `name` | the terminal, renamed |
| `terminal.resize` | `id`, `rows`, `cols` | |
| `terminal.explain` | `id` | the foreground process, the agent, every rule and whether it matches |
| `terminal.close` | `id` | ends the program and removes the terminal |
| `events.subscribe` | `output`: `"all"` or a list of ids | `{subscribed, terminals}`, then events on that connection |

A terminal looks like `{id, name, argv, cwd, pid, agent, state, needs_you, unseen, rule, title, exit_code, started_at}`.

Events: `terminal.started`, `terminal.changed`, `terminal.exited` (each with `terminal`), `terminal.closed` (with `id`), and `terminal.output` (`id`, `seq`, base64 `data`) for the ids a subscriber asked for.

To show a terminal that's already running: subscribe to its output first, then call `terminal.redraw`, draw that, and then draw only the output events whose `seq` is above the redraw's. `seq` counts a terminal's output chunks from 1. Without it, a viewer can't tell which chunks the redraw already holds, and shows some twice or loses them.

## How an agent is recognised

From the terminal's foreground process: its argv[0], its executable name, and, for `node` and the like, the script it runs and that script's package (`@google/gemini-cli` gives `gemini-cli`). The `agent` given at start counts only while the start command itself is in front.

## Terminal queries

Programs ask their terminal for the cursor position, its attributes and its colours. The daemon answers these itself, because often nobody is watching. A viewer that shows a terminal in another terminal should drop those queries from the output (see `QueryFilter` in `crates/daemon/src/viewer.rs`), or its own terminal will answer a second time.
