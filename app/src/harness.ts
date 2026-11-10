// A harness command and the model in it. The command stays the truth: a model is only a flag in it,
// written the way that harness's profile says, so any harness Sawtooth doesn't know still works.

import { useEffect, useState } from 'react';
import { api, type Factory, type HarnessProfile } from './api';

let loaded: HarnessProfile[] | null = null;
let loading: Promise<HarnessProfile[]> | null = null;

/** The harness profiles, loaded once for the window. */
export function useHarnessProfiles(): HarnessProfile[] {
  const [all, setAll] = useState<HarnessProfile[]>(loaded ?? []);
  useEffect(() => {
    if (loaded) return;
    loading ??= api.harnesses().then((h) => (loaded = h), () => []);
    void loading.then(setAll);
  }, []);
  return all;
}

const program = (command: string) => command.split(/\s+/).find((w) => w && !w.includes('='))?.split('/').pop() ?? '';

/** Which profile a command runs, going by its program name, as the engine does. */
export function profileFor(all: HarnessProfile[], command: string): HarnessProfile | undefined {
  const p = program(command.trim());
  return p ? all.find((h) => h.commands.includes(p)) : undefined;
}

/** The harness's name for a command: its profile's id, else the program. */
export const harnessName = (all: HarnessProfile[], command: string) => profileFor(all, command)?.id ?? program(command.trim());

const unquote = (s: string) => s.replace(/^['"]|['"]$/g, '');

/** The model a command picks, or '' when it picks none or the harness has no model flag. */
export function modelOf(profile: HarnessProfile | undefined, command: string): string {
  const flag = profile?.modelFlag;
  if (!flag) return '';
  const words = command.trim().split(/\s+/);
  for (let i = 0; i < words.length; i++) {
    if (words[i] === flag) return unquote(words[i + 1] ?? '');
    if (words[i]!.startsWith(`${flag}=`)) return unquote(words[i]!.slice(flag.length + 1));
  }
  return '';
}

/** The command with its model set to `model`, or taken out when it's empty. */
export function withModel(profile: HarnessProfile | undefined, command: string, model: string): string {
  const flag = profile?.modelFlag;
  if (!flag) return command;
  const words = command.trim().split(/\s+/).filter(Boolean);
  const kept: string[] = [];
  for (let i = 0; i < words.length; i++) {
    if (words[i] === flag) {
      i++;
      continue;
    }
    if (words[i]!.startsWith(`${flag}=`)) continue;
    kept.push(words[i]!);
  }
  const m = model.trim();
  if (!m) return kept.join(' ');
  const quoted = /^[\w.:/@+-]+$/.test(m) ? m : `'${m.replace(/'/g, `'\\''`)}'`;
  // Right after the program, so it reads as a choice of model rather than a stray option.
  return [kept[0] ?? '', flag, quoted, ...kept.slice(1)].filter(Boolean).join(' ');
}

/** "claude · haiku", or just "claude" when the command picks no model. */
export function runsOn(all: HarnessProfile[], command: string): string {
  const name = harnessName(all, command);
  const model = modelOf(profileFor(all, command), command);
  return [name, model].filter(Boolean).join(' · ');
}

/** Models to offer for a harness: its profile's, then any a factory on this machine already uses. */
export function modelChoices(all: HarnessProfile[], profile: HarnessProfile | undefined, factories: Factory[]): string[] {
  if (!profile) return [];
  const used = factories.flatMap((f) => [f.defaults.harness ?? '', ...f.agents.map((a) => a.harness), ...f.scorers.map((a) => a.harness)]);
  const seen = used.filter((c) => profileFor(all, c)?.id === profile.id).map((c) => modelOf(profile, c));
  return [...new Set([...(profile.models ?? []), ...seen].filter(Boolean))];
}
