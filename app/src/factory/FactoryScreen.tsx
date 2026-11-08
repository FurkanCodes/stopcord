import { useEffect, useState } from 'react';
import { confirm } from '@tauri-apps/plugin-dialog';
import { Button, IconButton, SectionTabs, Tooltip, ViewSwitch, factoryViews } from '@sawtooth/ui';
import { api, type Pos, type ProjectInfo, type Terminal } from '../api';
import {
  changeFactory,
  go,
  harnessOf,
  openFactory,
  refreshFactories,
  setState,
  showProblem,
  terminalsIn,
  useStore,
  type Draft,
  type Route,
  type Tab,
} from '../store';
import { EditPanel } from './EditPanel';
import { FlowCanvas } from './FlowCanvas';
import { FlowList } from './FlowList';
import { Knowledge } from './Knowledge';
import { Runs } from './Runs';
import { Terminals } from './Terminals';
import { fromTemplate, type Template } from './parts';
import { tilde } from '../text';

type FactoryRoute = Extract<Route, { screen: 'factory' }>;

const NO_DRAFTS: Draft[] = [];

/** One factory: its flow on a canvas or as lists, its terminals, its knowledge and its runs. */
export function FactoryScreen({ id }: { id: string }) {
  const route = useStore((s) => s.route) as FactoryRoute;
  const factories = useStore((s) => s.factories);
  const terminalsById = useStore((s) => s.terminals);
  const branches = useStore((s) => s.branches);
  const runs = useStore((s) => s.runs[id]);
  const drafts = useStore((s) => s.drafts[id] ?? NO_DRAFTS);
  const selected = useStore((s) => s.selected);
  const view = useStore((s) => s.factoryView);
  const agents = useStore((s) => s.agents);
  const installed = useStore((s) => s.installed);
  const loaded = useStore((s) => s.loaded);
  const [project, setProject] = useState<ProjectInfo | null>(null);
  const [folderCounts, setFolderCounts] = useState<Record<string, number>>({});

  const factory = factories.find((f) => f.id === id);
  const terminals = terminalsIn(id, { factories, terminals: terminalsById });

  useEffect(() => {
    if (!factory) return;
    api.detectProject(factory.path).then(setProject, () => setProject(null));
  }, [factory?.path]);

  const folderKey = factory?.parts
    .filter((p) => p.kind === 'folder')
    .map((p) => p.folder)
    .join('|');
  useEffect(() => {
    if (!factory) return;
    // Count after the debounced save has reached the app, so a just-typed folder is counted.
    const timer = setTimeout(() => {
      api.knowledge(factory.id).then(
        (k) => setFolderCounts(Object.fromEntries(k.folders.map((f) => [f.partId, f.files.length]))),
        () => undefined,
      );
    }, 700);
    return () => clearTimeout(timer);
  }, [factory?.id, folderKey]);

  if (id === '') return <OtherTerminals route={route} terminals={terminals} branches={branches} />;
  if (!factory) {
    return (
      <p className="m-8 text-body text-ink-2">{loaded ? 'This factory is gone. It may have been removed.' : 'Loading…'}</p>
    );
  }

  const meta = (t: Terminal) => {
    const where = branches[t.cwd] ?? (t.cwd === factory.path ? null : t.cwd.split('/').pop());
    return [harnessOf(t), where].filter(Boolean).join(' · ');
  };
  const select = (sel: string | null) => setState({ selected: sel });
  const pickTerminal = (terminal: string) => openFactory(id, 'terminals', terminal);

  function add(template: Template, at?: Pos) {
    if (!factory) return;
    if (template === 'terminal') {
      const taken = new Set([...terminals.map((t) => t.name), ...drafts.map((d) => d.name)]);
      let n = 1;
      while (taken.has(`Terminal ${n}`)) n++;
      const firstAgent = agents.find((a) => !installed || installed.includes(a)) ?? '';
      const draft: Draft = {
        id: `draft-${Math.random().toString(36).slice(2, 8)}`,
        name: `Terminal ${n}`,
        command: firstAgent,
        other: false,
        branch: `terminal-${n}`,
      };
      setState((s) => ({ drafts: { ...s.drafts, [id]: [...(s.drafts[id] ?? []), draft] }, selected: draft.id }));
      if (route.tab !== 'overview') go({ ...route, tab: 'overview' });
      return;
    }
    const part = fromTemplate(template, project?.testCommand);
    if (part.kind === 'knowledge' && factory.parts.some((p) => p.kind === 'knowledge')) return;
    if (part.kind === 'tickets' && factory.parts.some((p) => p.kind === 'tickets')) return;
    const layout = at ? { ...factory.layout, [part.id]: at } : factory.layout;
    changeFactory({ ...factory, parts: [...factory.parts, part], layout });
    select(part.id);
  }

  async function remove() {
    if (!factory) return;
    const ok = await confirm(
      `Remove ${factory.name} from Sawtooth? Its terminals keep running and the project folder stays as it is. Sawtooth takes out its hooks and knowledge base links, and keeps the factory's files in ~/.sawtooth/removed.`,
      { title: 'Remove factory', kind: 'warning', okLabel: 'Remove', cancelLabel: 'Keep it' },
    );
    if (!ok) return;
    try {
      await api.removeFactory(factory.id);
      await refreshFactories();
      go({ screen: 'home' });
    } catch (e) {
      showProblem(e);
    }
  }

  const hasTickets = factory.parts.some((p) => p.kind === 'tickets');
  const shared = {
    factory,
    terminals,
    drafts,
    runs,
    selected,
    folderCounts,
    meta,
    onSelect: (sel: string | null) => select(sel),
    onOpenRuns: () => go({ ...route, tab: 'runs' }),
  };

  return (
    <SectionTabs.Root value={route.tab} onValueChange={(tab) => go({ ...route, tab: tab as Tab })} className="grow">
      <div className="flex shrink-0 items-center gap-3 border-b border-line pr-3 pl-3.5">
        <SectionTabs.List className="shrink-0 flex-nowrap self-stretch border-b-0">
          <SectionTabs.Tab value="overview">Overview</SectionTabs.Tab>
          <SectionTabs.Tab value="terminals">Terminals</SectionTabs.Tab>
          <SectionTabs.Tab value="knowledge">Knowledge</SectionTabs.Tab>
          <SectionTabs.Tab value="runs">Runs</SectionTabs.Tab>
        </SectionTabs.List>
        <span className="min-w-0 grow truncate text-right font-mono text-meta text-ink-2" title={factory.path}>
          {tilde(factory.path)}
        </span>
        {route.tab === 'overview' && (
          <ViewSwitch value={view} onValueChange={(v) => setState({ factoryView: v })} options={factoryViews} />
        )}
        {hasTickets && (
          <Button
            variant="quiet"
            onClick={() => {
              const tickets = factory.parts.find((p) => p.kind === 'tickets');
              if (route.tab !== 'overview') go({ ...route, tab: 'overview' });
              if (tickets) select(tickets.id);
            }}
          >
            Feed tickets
          </Button>
        )}
        <Tooltip label="Remove from Sawtooth">
          <IconButton icon="trash" label={`Remove ${factory.name} from Sawtooth`} onClick={() => void remove()} />
        </Tooltip>
      </div>

      <SectionTabs.Panel value="overview" className="flex min-h-0">
        <div className="flex min-w-0 grow flex-col">
          {view === 'canvas' ? (
            <FlowCanvas
              {...shared}
              onOpenTerminal={pickTerminal}
              onAdd={add}
            />
          ) : (
            <FlowList {...shared} onSelect={(sel) => select(sel)} onAdd={(t) => add(t)} />
          )}
        </div>
        {selected && (
          <EditPanel
            factory={factory}
            selected={selected}
            terminals={terminals}
            drafts={drafts}
            runs={runs}
            git={project?.git ?? true}
            onClose={() => select(null)}
            onOpenTerminal={pickTerminal}
          />
        )}
      </SectionTabs.Panel>
      <SectionTabs.Panel value="terminals" className="flex min-h-0">
        <Terminals
          list={terminals}
          selectedId={route.terminal}
          meta={meta}
          onPick={pickTerminal}
          onAdd={() => add('terminal')}
        />
      </SectionTabs.Panel>
      <SectionTabs.Panel value="knowledge" className="flex min-h-0">
        <Knowledge factory={factory} />
      </SectionTabs.Panel>
      <SectionTabs.Panel value="runs" className="flex min-h-0">
        <Runs runs={runs} terminals={terminals} />
      </SectionTabs.Panel>
    </SectionTabs.Root>
  );
}

/** Terminals started outside every factory, such as from the sawtooth command in a shell. */
function OtherTerminals({
  route,
  terminals,
  branches,
}: {
  route: FactoryRoute;
  terminals: Terminal[];
  branches: Record<string, string | null>;
}) {
  const meta = (t: Terminal) => [harnessOf(t), branches[t.cwd] ?? t.cwd.split('/').pop()].filter(Boolean).join(' · ');
  return (
    <div className="flex min-h-0 grow flex-col">
      <p className="m-0 border-b border-line px-4 py-2.5 text-body text-ink-2">
        These terminals run outside every factory. Make a factory for their folder to see them on a canvas.
      </p>
      <div className="flex min-h-0 grow">
        <Terminals list={terminals} selectedId={route.terminal} meta={meta} onPick={(t) => openFactory('', 'terminals', t)} />
      </div>
    </div>
  );
}

