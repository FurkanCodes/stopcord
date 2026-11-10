import type { Factory, LineStage, WorkItem } from '../api';
import { runsOn, useHarnessProfiles } from '../harness';
import { Empty } from '../tui/kit';
import { Logo } from '../tui/Logo';

const who: Record<LineStage['kind'], string> = { agent: 'agent', gate: 'engine', human: 'person' };

/** The work sitting at a stage right now. */
export function atStage(factory: Factory, stage: string): WorkItem[] {
  return factory.work.filter((w) => w.stage === 'running' && w.lineStage === stage);
}

/** What runs an agent stage: its agent's own harness, or the factory's. */
export function stageHarness(factory: Factory, stage: LineStage): string | undefined {
  if (stage.kind !== 'agent') return undefined;
  const agent = [...factory.agents, ...factory.scorers].find((a) => a.id === stage.agent);
  return agent?.harness || factory.defaults.harness || undefined;
}

/** The factory's line, left to right, with what's at each stage now. */
export function LineStrip({ factory, onPick, selected, onSelect }: { factory: Factory; onPick?: (key: string) => void; selected?: number; onSelect?: (index: number) => void }) {
  const profiles = useHarnessProfiles();
  if (factory.line.length === 0) return <Empty>this factory has no line. add one in setup</Empty>;
  return (
    <ol aria-label="The line" style={{ display: 'flex', flexWrap: 'nowrap', overflowX: 'auto', alignItems: 'stretch', gap: '6px 0', listStyle: 'none', margin: 0, padding: '0 12px' }}>
      {factory.line.map((s, i) => {
        const here = atStage(factory, s.id);
        const waiting = here.some((w) => w.waiting !== 'no' && w.waiting !== 'landing');
        const solid = s.kind === 'gate';
        const Box = onSelect ? 'button' : 'div';
        return (
          <li key={s.id} style={{ display: 'flex', alignItems: 'stretch', flexShrink: 0 }}>
            {i > 0 && (
              <span aria-hidden="true" className="t-dim" style={{ padding: '0 6px', alignSelf: 'center' }}>
                ──▶
              </span>
            )}
            <Box
              {...(onSelect ? { type: 'button' as const, 'aria-pressed': selected === i, onClick: () => onSelect(i) } : {})}
              style={{
                minWidth: '13ch',
                padding: '3px 8px',
                border: `1px ${s.kind === 'human' ? 'dashed' : 'solid'} ${waiting ? 'var(--red)' : selected === i ? 'var(--ink)' : 'var(--line)'}`,
                background: solid ? 'var(--bar)' : 'transparent',
                ...(onSelect ? { font: 'inherit', color: 'inherit', textAlign: 'left' as const, cursor: 'pointer' } : {}),
              }}
            >
              <div style={{ fontWeight: 600, color: 'var(--ink)' }}>{s.id}</div>
              <div className="t-muted" style={{ fontSize: 12 }}>
                <Logo name={stageHarness(factory, s)} size={12} style={{ marginRight: 5 }} />
                {/* An agent stage says what it runs on, model and all, so the line shows which model does what. */}
                {s.kind === 'agent' && stageHarness(factory, s) ? runsOn(profiles, stageHarness(factory, s)!) : who[s.kind]}
                {s.kind === 'agent' && [...factory.agents, ...factory.scorers].some((a) => a.id === s.agent && a.once) ? ' · once' : ''}
                {s.race?.length ? ` · races ${s.race.join(', ')}` : ''}
                {s.panel?.length ? ` · talks with ${s.panel.join(', ')}` : ''}
                {s.maxLoops ? ` · up to ${s.maxLoops} loops` : ''}
              </div>
              <div style={{ fontSize: 12, display: 'flex', flexWrap: 'wrap', gap: '0 6px' }}>
                {here.map((w) =>
                  onPick ? (
                    <button key={w.id} type="button" className="t-key" style={{ padding: '0 4px', fontSize: 12 }} onClick={() => onPick(w.ticket.key)}>
                      {w.ticket.key}
                      {w.waiting !== 'no' && w.waiting !== 'landing' && <span className="t-red"> needs you</span>}
                    </button>
                  ) : (
                    <span key={w.id}>
                      {w.ticket.key}
                      {w.waiting !== 'no' && w.waiting !== 'landing' && <span className="t-red"> needs you</span>}
                    </span>
                  ),
                )}
              </div>
            </Box>
          </li>
        );
      })}
    </ol>
  );
}
