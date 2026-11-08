# @sawtooth/ui

Sawtooth's components and tokens: React, [Base UI](https://base-ui.com) 1.9 for behaviour, Tailwind v4 for styling. One dark theme.

## Use it

```css
/* app.css */
@import "tailwindcss";
@import "@sawtooth/ui/theme.css";
```

```html
<link rel="stylesheet" href="https://fonts.googleapis.com/css2?family=IBM+Plex+Mono:wght@400;500&family=IBM+Plex+Sans:wght@400;500;600&display=swap">
```

```tsx
import { TooltipProvider, FlowNode, Lamp } from '@sawtooth/ui';

<TooltipProvider>
  <FlowNode icon="shield" name="No secrets" meta="every commit" flag="failed" x={800} y={120} />
</TooltipProvider>
```

`theme.css` holds the tokens as a Tailwind `@theme`, so they're utilities: `bg-raised`, `text-ink-2`, `border-line`, `rounded-node`, `shadow-pop`, `text-meta`, `animate-breathe`. It removes Tailwind's default palette, so only Sawtooth colours exist. It also `@source`s this package, so the app doesn't need to.

## Rules the components keep

- Red means "needs you" and nothing else. Green shows only on working lamps.
- Status is never colour alone. A filled lamp means working, a ring means idle, and a red lamp always has a word beside it.
- One `solid` button per screen. `quiet` buttons are bordered and text-only (the type forbids an icon). `ghost` and `IconButton` have no border.
- At most two boxes deep: the sandbox holds terminal cards and nothing nests further.
- One thing moves at a time: one packet on one line, one breathing lamp. Nothing glows or blinks. Under reduced motion everything stops.
- `ink-3` is for icons only. At about 4:1 it's too low for text.

## What's here

| | |
| --- | --- |
| Actions | `Button` (solid, quiet, ghost; sm, md), `IconButton` |
| Status | `Lamp`, `NeedsCount`, `NeedsBar`, `HarnessChip`, `NoTerminalsChip` |
| Canvas | `Canvas`, `ZoneBox`, `FlowStatus`, `FlowNode`, `Sandbox`, `TerminalCard`, `EdgeLayer`, `Edge`, `Packet`, `EdgeLabel` |
| Routing | `connect`, `pathH`, `pathV`, `arrowAt` (from `route.ts`, ported from the prototype) |
| Home and list | `FactoryCard`, `ListSection`, `ListRow` |
| Navigation | `SectionTabs` (Base UI Tabs), `ViewSwitch` (ToggleGroup), `Menu` |
| Forms | `Field`, `TextInput`, `TextArea`, `Select` |
| Overlays | `Tooltip`, `TooltipProvider`, `Panel` |
| Icons | `Icon` (24px grid, 1.6 or 1.8 stroke), `LogoMark`, `Wordmark` |

### Drawing a flow

```tsx
const sb = { x: 260, y: 40, w: 460, h: 320 };
const intake = { x: 12, y: 80, w: 200, h: 64 };
const r = connect(intake, sb, { ey: sb.y + 72, bend: 'src', prefer: 'h' });

<Canvas width={1000} height={520} label="How work moves through invoice-app">
  <ZoneBox x={0} y={40} width={224} height={116} title="Intake" count={1}
    action={<IconButton icon="plus" label="Add to Intake" />} />
  <EdgeLayer width={1000} height={520}><Edge route={r} live /></EdgeLayer>
  <Packet key={step} path={r.d} />
  <FlowNode icon="ticket" name="Tickets" meta="3 waiting" x={intake.x} y={intake.y} width={intake.w} />
  <Sandbox x={sb.x} y={sb.y} width={sb.w} height={sb.h} status="1 working">
    <TerminalCard name="Ada" state="working" live meta="claude · feature/gst" line="Editing src/billing/tax.ts" />
  </Sandbox>
</Canvas>
```

Change the `Packet`'s `key` every step (2.8s in the prototype) to send the next one. Drag handling stays in the app: `FlowNode` and `Sandbox` take pointer handlers and a `dragging` flag.

## Check it

```sh
npm install
npm run typecheck   # tsc against the real Base UI types
npm run css:check   # builds check/check.css with Tailwind to prove theme.css compiles
```

## Not done yet

- A light theme.
- Tests and a running demo page.
- Building to `dist/`. The package exports `src/` directly, so the app's bundler compiles it.
