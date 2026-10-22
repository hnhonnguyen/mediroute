import type { SimStatus, Strategy } from '../types';
import { formatMinutes } from '../lib/format';

interface ControlsProps {
  status: SimStatus | null;
  strategy: Strategy;
  speed: number;
  comparing: boolean;
  onStrategyChange: (strategy: Strategy) => void;
  onSpeedChange: (speed: number) => void;
  onStart: () => void;
  onPause: () => void;
  onResume: () => void;
  onReset: () => void;
  onCompare: () => void;
}

const STRATEGIES: { value: Strategy; label: string; description: string }[] = [
  {
    value: 'optimized',
    label: 'Optimized',
    description: 'Congestion-aware routing from learned service times',
  },
  {
    value: 'fixed_order',
    label: 'Fixed Order',
    description: 'Package order baseline, ignoring live queues',
  },
  {
    value: 'shortest_queue',
    label: 'Shortest Queue',
    description: 'Myopic baseline that only sees current queues',
  },
];

function PlayIcon() {
  return (
    <svg viewBox="0 0 20 20" fill="currentColor" className="h-4 w-4">
      <path d="M6.3 3.8a1 1 0 0 1 1.53-.85l8 6.2a1 1 0 0 1 0 1.7l-8 6.2a1 1 0 0 1-1.53-.85V3.8Z" />
    </svg>
  );
}

function PauseIcon() {
  return (
    <svg viewBox="0 0 20 20" fill="currentColor" className="h-4 w-4">
      <rect x="4.5" y="3.5" width="3.6" height="13" rx="1" />
      <rect x="11.9" y="3.5" width="3.6" height="13" rx="1" />
    </svg>
  );
}

function ResetIcon() {
  return (
    <svg
      viewBox="0 0 20 20"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.7"
      className="h-4 w-4"
    >
      <path
        d="M4.2 6.3A6.5 6.5 0 1 1 3.5 12"
        strokeLinecap="round"
      />
      <path d="M3.6 2.9v3.8h3.8" strokeLinecap="round" strokeLinejoin="round" />
    </svg>
  );
}

function ChartIcon() {
  return (
    <svg viewBox="0 0 20 20" fill="none" stroke="currentColor" strokeWidth="1.7" className="h-4 w-4">
      <path d="M3 16.5h14" strokeLinecap="round" />
      <path d="M5.5 13V9.5M10 13V4.5M14.5 13v-5" strokeLinecap="round" />
    </svg>
  );
}

export default function Controls({
  status,
  strategy,
  speed,
  comparing,
  onStrategyChange,
  onSpeedChange,
  onStart,
  onPause,
  onResume,
  onReset,
  onCompare,
}: ControlsProps) {
  const started = status?.started ?? false;
  const running = status?.running ?? false;
  const finished = status?.finished ?? false;
  const active = started && !finished;
  const strategyLocked = active;
  const selected = STRATEGIES.find((option) => option.value === strategy);

  let action: { label: string; onClick: () => void; icon: JSX.Element };
  if (!started || finished) {
    action = {
      label: started ? 'Start new run' : 'Start simulation',
      onClick: onStart,
      icon: <PlayIcon />,
    };
  } else if (running) {
    action = { label: 'Pause', onClick: onPause, icon: <PauseIcon /> };
  } else {
    action = { label: 'Resume', onClick: onResume, icon: <PlayIcon /> };
  }

  return (
    <section className="rounded-2xl border border-slate-800 bg-slate-900/60 p-4 shadow-xl shadow-black/20 backdrop-blur">
      <div className="flex flex-col gap-4 xl:flex-row xl:items-end xl:justify-between">
        <div className="flex flex-wrap items-center gap-2">
          <button
            type="button"
            onClick={action.onClick}
            className="inline-flex items-center gap-2 rounded-xl bg-emerald-500 px-4 py-2.5 text-sm font-semibold text-emerald-950 shadow-lg shadow-emerald-500/20 transition hover:bg-emerald-400 focus:outline-none focus-visible:ring-2 focus-visible:ring-emerald-300"
          >
            {action.icon}
            {action.label}
          </button>
          <button
            type="button"
            onClick={onReset}
            disabled={!started}
            className="inline-flex items-center gap-2 rounded-xl border border-slate-700 bg-slate-800/60 px-3.5 py-2.5 text-sm font-medium text-slate-200 transition hover:border-slate-600 hover:bg-slate-800 disabled:cursor-not-allowed disabled:opacity-40"
          >
            <ResetIcon />
            Reset
          </button>
          <div className="ml-1 h-8 w-px bg-slate-800 max-sm:hidden" />
          <button
            type="button"
            onClick={onCompare}
            disabled={comparing}
            className="inline-flex items-center gap-2 rounded-xl border border-sky-500/30 bg-sky-500/10 px-3.5 py-2.5 text-sm font-medium text-sky-200 transition hover:border-sky-400/50 hover:bg-sky-500/15 disabled:cursor-wait disabled:opacity-60"
          >
            {comparing ? (
              <span className="h-4 w-4 animate-spin rounded-full border-2 border-sky-300/40 border-t-sky-200" />
            ) : (
              <ChartIcon />
            )}
            {comparing ? 'Running comparison…' : 'Run comparison'}
          </button>
        </div>

        <div className="flex flex-col gap-4 sm:flex-row sm:items-end">
          <label className="block sm:w-60">
            <span className="mb-1.5 block text-[11px] font-medium uppercase tracking-[0.14em] text-slate-400">
              Strategy
            </span>
            <select
              value={strategy}
              onChange={(event) => onStrategyChange(event.target.value as Strategy)}
              disabled={strategyLocked}
              className="w-full rounded-xl border border-slate-700 bg-slate-950/80 px-3 py-2.5 text-sm text-slate-100 outline-none transition focus:border-emerald-500/60 disabled:cursor-not-allowed disabled:opacity-60"
            >
              {STRATEGIES.map((option) => (
                <option key={option.value} value={option.value}>
                  {option.label}
                </option>
              ))}
            </select>
          </label>

          <label className="block sm:w-72">
            <div className="mb-1.5 flex items-center justify-between gap-3">
              <span className="text-[11px] font-medium uppercase tracking-[0.14em] text-slate-400">
                Simulation speed
              </span>
              <span className="rounded-full bg-slate-800 px-2 py-0.5 text-[11px] font-medium text-emerald-300">
                {speed}× · {formatMinutes(status?.sim_time_min ?? 0, 0)} clock
              </span>
            </div>
            <input
              type="range"
              min={1}
              max={30}
              step={1}
              value={speed}
              aria-label="Simulation speed in simulated minutes per second"
              onChange={(event) => onSpeedChange(Number(event.target.value))}
              className="h-2 w-full cursor-pointer appearance-none rounded-full bg-slate-700 accent-emerald-500"
            />
            <p className="mt-1.5 text-[11px] text-slate-500">
              {speed} simulated minutes per real second
            </p>
          </label>
        </div>
      </div>

      {selected && (
        <p className="mt-3 border-t border-slate-800/80 pt-3 text-xs text-slate-400">
          <span className="font-medium text-slate-300">{selected.label}:</span>{' '}
          {selected.description}.{' '}
          {strategyLocked
            ? 'Reset the run to switch strategies.'
            : finished
              ? 'Run finished. Start a new run to try another strategy.'
              : 'Start a run to see live routing decisions.'}
        </p>
      )}
    </section>
  );
}
