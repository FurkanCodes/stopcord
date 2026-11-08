import { useEffect, useState } from 'react';
import { confirm } from '@tauri-apps/plugin-dialog';
import { api, type Pos, type ProjectInfo } from '../api';
import { changeFactory, go, openFactory, refreshFactories, setState, showProblem, terminalsIn, useStore, type Draft, type Route, type Tab } from '../store';
import { Brand, Screen } from '../tui/kit';
import { plain, useKeys } from '../tui/keys';
import { FactoryMap } from './FactoryMap';
import { Fleet } from './Fleet';
import { Knowledge } from './Knowledge';
import { Runs } from './Runs';
import { fromTemplate, type Template } from './parts';

type FactoryRoute = Extract<Route, { screen: 'factory' }>;

const NO_DRAFTS: Draft[] = [];
const tabKeys: Record<string, Tab> = { '1': 'overview', '2': 'terminals', '3': 'knowledge', '4': 'runs' };

/** One factory: its map, its terminals, its knowledge and its runs. */
export function FactoryScreen({ id, onSearch }: { id: string; onSearch: () => void }) {
  const route = useStore((s) => s.route) as FactoryRoute;
  const factories = useStore((s) => s.factories);
  const terminalsById = useStore((s) => s.terminals);
  const runs = useStore((s) => s.runs[id]);
  const drafts = useStore((s) => s.drafts[id] ?? NO_DRAFTS);
  const selected = useStore((s) => s.selected);
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

  useKeys((e) => {
    if (!plain(e)) return;
    const tab = tabKeys[e.key];
    if (tab && factory) {
      if (tab !== route.tab) openFactory(id, tab);
      return true;
    }
    if (e.key === 't' && factory) {
      go({ screen: 'tickets', id });
      return true;
    }
    if (e.key === 'Escape') {
      go({ screen: 'home' });
      return true;
    }
  }, { priority: -1 });

  // Terminals outside every factory only have the terminals screen.
  if (id === '' || route.tab === 'terminals') return <Fleet route={route} onSearch={onSearch} />;

  if (!factory) {
    return (
      <Screen header={<Brand />} hints={[['esc', 'back to every factory']]} status={{ mode: 'FACTORY' }}>
        <p className="t-dim" style={{ padding: 20 }}>
          {loaded ? '-- this factory is gone. it may have been removed --' : 'loading…'}
        </p>
      </Screen>
    );
  }

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
    setState({ selected: part.id });
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

  if (route.tab === 'knowledge') return <Knowledge factory={factory} file={route.file} />;
  if (route.tab === 'runs') return <Runs factory={factory} runs={runs} terminals={terminals} />;
  return (
    <FactoryMap
      factory={factory}
      terminals={terminals}
      drafts={drafts}
      runs={runs}
      selected={selected}
      folderCounts={folderCounts}
      git={project?.git ?? true}
      onSelect={(sel) => setState({ selected: sel })}
      onAdd={add}
      onOpenTerminal={(t) => openFactory(id, 'terminals', t)}
      onOpenRuns={() => openFactory(id, 'runs')}
      onRemove={() => void remove()}
    />
  );
}

