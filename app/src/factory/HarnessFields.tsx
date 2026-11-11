// What a role runs on: a harness, its model and its effort, each picked from a list. Used in a
// factory's setup and in New factory.

import { useState } from 'react';
import type { HarnessProfile } from '../api';
import { effortOf, modelChoices, modelOf, profileFor, runsOn, useHarnessProfiles, useListedModels, withEffort, withModel } from '../harness';
import { useStore } from '../store';
import { FieldRow } from '../tui/kit';

/**
 * What a role runs on: a harness, its model and its effort, each picked from a list. The command stays
 * the truth: a model or an effort is only a flag in it, written the way that harness's profile says,
 * and "other" lets anything be typed, so a harness Sawtooth has no profile for still works.
 */
export function HarnessFields({ id, command, fallback, placeholder, onChange, label = 'harness', width = '14ch' }: { id: string; command: string; fallback?: string; placeholder: string; onChange: (command: string) => void; label?: string; width?: string }) {
  const profiles = useHarnessProfiles();
  const installed = useStore((s) => s.installed) ?? [];
  const factories = useStore((s) => s.factories);
  const [typing, setTyping] = useState<'' | 'command' | 'model' | 'effort'>('');
  const effective = command.trim() || fallback?.trim() || '';
  const profile = profileFor(profiles, effective);
  const { models: listed, asking } = useListedModels(profile);
  const model = modelOf(profile, effective);
  const effort = effortOf(profile, effective);
  const isIn = (p: HarnessProfile) => p.commands.some((c) => installed.includes(c));
  const program = effective.split(/\s+/)[0] ?? '';
  const known = profileFor(profiles, command.trim());
  const harnessValue = !command.trim() ? '' : known ? known.id : 'custom';
  const sorted = [...profiles].sort((a, b) => Number(isIn(b)) - Number(isIn(a)) || a.id.localeCompare(b.id));
  const models = [...new Set([...modelChoices(profiles, profile, factories), ...listed])];
  const base = command.trim() || effective;

  return (
    <>
      <FieldRow id={`${id}-h`} label={label} width={width}>
        <select
          id={`${id}-h`}
          className="t-field"
          value={harnessValue}
          onChange={(e) => {
            const v = e.currentTarget.value;
            setTyping(v === 'other' ? 'command' : '');
            if (v === 'other' || v === 'custom') return;
            // A new harness starts clean: another's model and effort flags mean nothing to it.
            onChange(v ? profiles.find((p) => p.id === v)!.commands[0]! : '');
          }}
        >
          <option value="">{fallback?.trim() ? `default · ${runsOn(profiles, fallback)}` : placeholder}</option>
          {sorted.map((p) => (
            <option key={p.id} value={p.id}>
              {p.id}
              {isIn(p) ? '' : ' · not installed'}
            </option>
          ))}
          {harnessValue === 'custom' && <option value="custom">{program} · your own command</option>}
          <option value="other">other command…</option>
        </select>
        {(typing === 'command' || harnessValue === 'custom') && (
          <input id={`${id}-cmd`} className="t-field" spellCheck={false} autoFocus={typing === 'command'} value={command} placeholder="any command that starts an agent" onChange={(e) => onChange(e.currentTarget.value)} />
        )}
      </FieldRow>
      <FieldRow id={`${id}-m`} label="model" width={width}>
        {!profile?.modelFlag ? (
          <select id={`${id}-m`} className="t-field" disabled value="">
            <option value="">{!effective ? 'pick a harness first' : `set in ${program}'s own command or settings`}</option>
          </select>
        ) : (
          <>
            <select
              id={`${id}-m`}
              className="t-field"
              value={typing === 'model' ? 'other' : model}
              onChange={(e) => {
                const v = e.currentTarget.value;
                if (v === 'other') return setTyping('model');
                setTyping('');
                onChange(withModel(profile, base, v));
              }}
            >
              <option value="">{profile.id}&apos;s default</option>
              {model && !models.includes(model) && <option value={model}>{model}</option>}
              {models.map((m) => (
                <option key={m} value={m}>{m}</option>
              ))}
              <option value="other">other…</option>
            </select>
            {typing === 'model' && <input id={`${id}-m-other`} className="t-field" spellCheck={false} autoFocus defaultValue={model} placeholder="the model's name, as the harness calls it" onBlur={(e) => { onChange(withModel(profile, base, e.currentTarget.value)); setTyping(''); }} onKeyDown={(e) => e.key === 'Enter' && e.currentTarget.blur()} />}
            {asking && <span className="t-dim" style={{ fontSize: 12 }}>asking {profile.id} which models it has…</span>}
          </>
        )}
      </FieldRow>
      <FieldRow id={`${id}-e`} label="effort" width={width}>
        {!profile?.effortArg ? (
          <select id={`${id}-e`} className="t-field" disabled value="">
            <option value="">{!effective ? 'pick a harness first' : `${profile?.id ?? program} has no effort setting`}</option>
          </select>
        ) : (
          <>
            <select
              id={`${id}-e`}
              className="t-field"
              value={typing === 'effort' ? 'other' : effort}
              onChange={(e) => {
                const v = e.currentTarget.value;
                if (v === 'other') return setTyping('effort');
                setTyping('');
                onChange(withEffort(profile, base, v));
              }}
            >
              <option value="">{profile.id}&apos;s default</option>
              {effort && !(profile.efforts ?? []).includes(effort) && <option value={effort}>{effort}</option>}
              {(profile.efforts ?? []).map((m) => (
                <option key={m} value={m}>{m}</option>
              ))}
              <option value="other">other…</option>
            </select>
            {typing === 'effort' && <input id={`${id}-e-other`} className="t-field" spellCheck={false} autoFocus defaultValue={effort} placeholder="the level, as the harness calls it" onBlur={(e) => { onChange(withEffort(profile, base, e.currentTarget.value)); setTyping(''); }} onKeyDown={(e) => e.key === 'Enter' && e.currentTarget.blur()} />}
          </>
        )}
        {effective && <span className="t-dim" style={{ fontSize: 12 }}>{command.trim() ? 'runs' : 'uses the default'}: {effective}</span>}
      </FieldRow>
    </>
  );
}
