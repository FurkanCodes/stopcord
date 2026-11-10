"""Summarize a link-test run: summarize.py <results dir>.

For each harness/case/probe, pulls the agent's final answer and counts tool
calls out of that harness's JSON output, then prints a Markdown report.
"""
import json
import re
import sys
from pathlib import Path

WORDS = {
    "HERON-41": "knowledge base",
    "OTTER-17": "repo's own file",
    "WREN-05": "factory copy of repo file",
}
WORD_RE = re.compile(r"\b(HERON-41|OTTER-17|WREN-05|KESTREL-88)\b")


def jsonl(text):
    for line in text.splitlines():
        line = line.strip()
        if line.startswith("{"):
            try:
                yield json.loads(line)
            except json.JSONDecodeError:
                pass


def walk(obj):
    yield obj
    if isinstance(obj, dict):
        for v in obj.values():
            yield from walk(v)
    elif isinstance(obj, list):
        for v in obj:
            yield from walk(v)


def parse_claude(raw, _):
    answer, tools, denied = "", 0, 0
    for ev in jsonl(raw):
        if ev.get("type") == "result":
            answer = ev.get("result") or ""
            denied = len(ev.get("permission_denials") or [])
        if ev.get("type") == "assistant":
            for part in ev.get("message", {}).get("content", []):
                if part.get("type") == "tool_use":
                    tools += 1
    return answer, tools, denied


def parse_codex(raw, path):
    last = Path(str(path) + ".last")
    answer = last.read_text() if last.exists() else ""
    tools = 0
    for ev in jsonl(raw):
        item = ev.get("item") or {}
        if ev.get("type") == "item.completed" and item.get("type") not in (None, "agent_message", "reasoning", "error"):
            tools += 1
        if not answer and item.get("type") == "agent_message":
            answer = item.get("text", "")
    return answer, tools, 0


def parse_gemini(raw, _):
    start = raw.find("{")
    if start < 0:
        return "", 0, 0
    try:
        data = json.loads(raw[start:])
    except json.JSONDecodeError:
        return "", 0, 0
    tools = ((data.get("stats") or {}).get("tools") or {}).get("totalCalls", 0)
    return data.get("response") or "", tools, 0


def parse_opencode(raw, _):
    texts, tools = [], 0
    for ev in jsonl(raw):
        t = str(ev.get("type", ""))
        part = ev.get("part") or {}
        if "tool" in t or part.get("type") == "tool":
            tools += 1
        elif part.get("type") == "text" and part.get("text"):
            texts.append(part["text"])
    return (texts[-1] if texts else ""), tools, 0


PARSERS = {"claude": parse_claude, "codex": parse_codex, "gemini": parse_gemini, "opencode": parse_opencode}


def probe(dirpath, harness, name):
    path = dirpath / f"{name}.raw"
    if not path.exists():
        return None
    raw = path.read_text(errors="replace")
    code = (Path(str(path) + ".exit").read_text().strip() if Path(str(path) + ".exit").exists() else "?")
    answer, tools, denied = PARSERS[harness](raw, path)
    said = sorted(set(WORD_RE.findall(answer)))
    # Words that appear anywhere in the run's output (tool results included):
    # evidence a file was read even when the agent didn't report it.
    seen = sorted(set(WORD_RE.findall(raw)) - set(said))
    return {"code": code, "answer": answer.strip(), "said": said, "seen": seen, "tools": tools, "denied": denied}


def cell(p):
    if p is None:
        return "not run"
    if p["code"] not in ("0", "?"):
        return f"failed (exit {p['code']})"
    words = ", ".join(w.split("-")[0].lower() for w in p["said"]) or "none"
    bits = [words, f"{p['tools']} tools"]
    if p["denied"]:
        bits.append(f"{p['denied']} denied")
    if p["seen"]:
        bits.append("also read " + ", ".join(w.split("-")[0].lower() for w in p["seen"]))
    return " · ".join(bits)


def write_cell(dirpath, p):
    state = dirpath / "state.txt"
    if p is None or not state.exists():
        return "not run"
    lines = state.read_text().splitlines()
    facts = [l[5:] for l in lines if l.startswith("fact ")]
    links = [l.split()[0] for l in lines if l.endswith(" link")]
    broke = [l.split()[0] for l in lines if l.endswith(" file") and l.split()[0] in ("CLAUDE.md", "AGENTS.md", "GEMINI.md")]
    status = next((l[7:] for l in lines if l.startswith("status ")), "")
    where = "; ".join(facts) if facts else "nowhere"
    out = f"fact in: {where}"
    if links:
        out += f" · links kept: {len(links)}"
    if broke and dirpath.name == "link":
        out += f" · **link replaced by a file: {', '.join(broke)}**"
    if status.strip(";"):
        out += f" · git sees: `{status.strip(';')}`"
    if p["code"] not in ("0", "?"):
        out = f"failed (exit {p['code']}) · " + out
    return out


def main():
    root = Path(sys.argv[1])
    print(f"# Link test · {root.name}\n")
    print("Code words: heron = knowledge base, otter = repo's own file, wren = the factory's copy of the repo file "
          "(only reachable by following \"Before you start, read knowledge/repo/AGENTS.md\").\n")
    print("| harness | case | load (tools off) | follow (read only) | write |")
    print("|---|---|---|---|---|")
    details = []
    for hdir in sorted(p for p in root.iterdir() if p.is_dir()):
        for case in ("control", "link", "second"):
            d = hdir / case
            if not d.is_dir():
                continue
            ps = {n: probe(d, hdir.name, n) for n in ("load", "follow", "write")}
            print(f"| {hdir.name} | {case} | {cell(ps['load'])} | {cell(ps['follow'])} | {write_cell(d, ps['write'])} |")
            for n, p in ps.items():
                if p:
                    details.append(f"- **{hdir.name}/{case}/{n}** (exit {p['code']}): {p['answer'][:300]!r}")
    print("\n## What to look for\n")
    print("- **load:** heron in a link row means the harness follows a symlinked instruction file. "
          "Compare with the control row; if control is missing heron too, the harness didn't load its file in this mode at all.")
    print("- **follow:** wren means the agent opened the factory's copy of the repo's file. "
          "Otter without wren means it got the repo's knowledge some other way (the tracked file, or git).")
    print("- **write:** the fact should land in the factory's knowledge-base.md, and the links should still be links. "
          "\"link replaced by a file\" means the edit tool overwrote the symlink, so the knowledge base and the worktree drifted apart.")
    print("\n## Final answers\n")
    print("\n".join(details))


if __name__ == "__main__":
    main()
