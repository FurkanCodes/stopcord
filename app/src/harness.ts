// A harness command and the model in it. The command stays the truth: a model is only a flag in it,
// written the way that harness's profile says, so any harness Stopcord doesn't know still works.

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

/** Where a command's effort words are, by the profile's effortArg: their index, how many, and the level. */
function effortAt(profile: HarnessProfile | undefined, words: string[]): { at: number; n: number; level: string } | null {
  const shape = (profile?.effortArg ?? '').split(/\s+/).filter(Boolean);
  const k = shape.findIndex((w) => w.includes('{effort}'));
  if (k < 0) return null;
  const [before, after] = shape[k]!.split('{effort}') as [string, string];
  for (let i = 0; i + shape.length <= words.length; i++) {
    const fits = shape.every((s, j) => (j === k ? words[i + j]!.startsWith(before) && words[i + j]!.endsWith(after) && words[i + j]!.length > before.length + after.length : words[i + j] === s));
    if (fits) {
      const w = words[i + k]!;
      return { at: i, n: shape.length, level: unquote(w.slice(before.length, w.length - after.length)) };
    }
  }
  return null;
}

/** The effort a command asks for, or '' when it asks for none or the harness has no effort setting. */
export function effortOf(profile: HarnessProfile | undefined, command: string): string {
  return effortAt(profile, command.trim().split(/\s+/).filter(Boolean))?.level ?? '';
}

/** The command with its effort set to `effort`, or taken out when it's empty. */
export function withEffort(profile: HarnessProfile | undefined, command: string, effort: string): string {
  if (!profile?.effortArg) return command;
  const words = command.trim().split(/\s+/).filter(Boolean);
  const found = effortAt(profile, words);
  const kept = found ? [...words.slice(0, found.at), ...words.slice(found.at + found.n)] : words;
  const e = effort.trim();
  if (!e || !/^[\w.-]+$/.test(e)) return kept.join(' ');
  const added = profile.effortArg.replace('{effort}', e).split(/\s+/);
  // After the program and any model, so it reads as part of the choice of model.
  const model = profile.modelFlag ? kept.indexOf(profile.modelFlag) : -1;
  const at = model > 0 && model + 1 < kept.length ? model + 2 : 1;
  return [...kept.slice(0, at), ...added, ...kept.slice(at)].join(' ');
}

/** "claude · haiku · high", or just "claude" when the command picks no model or effort. */
export function runsOn(all: HarnessProfile[], command: string): string {
  const name = harnessName(all, command);
  const profile = profileFor(all, command);
  return [name, modelOf(profile, command), effortOf(profile, command)].filter(Boolean).join(' · ');
}

/** Models to offer for a harness: its profile's, then any a factory on this machine already uses. */
export function modelChoices(all: HarnessProfile[], profile: HarnessProfile | undefined, factories: Factory[]): string[] {
  if (!profile) return [];
  const used = factories.flatMap((f) => [f.defaults.harness ?? '', ...f.agents.map((a) => a.harness), ...f.scorers.map((a) => a.harness)]);
  const seen = used.filter((c) => profileFor(all, c)?.id === profile.id).map((c) => modelOf(profile, c));
  return [...new Set([...(profile.models ?? []), ...seen].filter(Boolean))];
}

const listed = new Map<string, Promise<string[]>>();

/** The models a harness lists itself, asked once per window. Empty while asking, or when it can't list them. */
export function useListedModels(profile: HarnessProfile | undefined): { models: string[]; asking: boolean } {
  const id = profile?.modelsCommand ? profile.id : '';
  const [got, setGot] = useState<{ id: string; models: string[] }>({ id: '', models: [] });
  useEffect(() => {
    if (!id) return;
    if (!listed.has(id)) listed.set(id, api.harnessModels(id).catch(() => []));
    let live = true;
    void listed.get(id)!.then((models) => live && setGot({ id, models }));
    return () => {
      live = false;
    };
  }, [id]);
  return { models: got.id === id ? got.models : [], asking: !!id && got.id !== id };
}
