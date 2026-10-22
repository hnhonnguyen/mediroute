import { useCallback, useEffect, useState } from 'react';
import { api } from './api';
import Controls from './components/Controls';
import HistoryPanel from './components/HistoryPanel';
import MetricsBar from './components/MetricsBar';
import PatientTable from './components/PatientTable';
import ReportView from './components/ReportView';
import RoomGrid from './components/RoomGrid';
import { usePolling } from './hooks/usePolling';
import { formatMinutes } from './lib/format';
import type {
  CompareResponse,
  HistoryEntry,
  Metrics,
  PatientView,
  RoomView,
  ScenarioConfig,
  SimStatus,
  Strategy,
} from './types';

interface DashboardData {
  status: SimStatus;
  rooms: RoomView[];
  patients: PatientView[];
  history: HistoryEntry[];
}

const EMPTY_METRICS: Metrics = {
  total_patients: 0,
  arrived: 0,
  completed: 0,
  in_hospital: 0,
  avg_total_min: 0,
  avg_wait_min: 0,
  p90_total_min: 0,
  p90_wait_min: 0,
  total_visits: 0,
  room_utilization: [],
};

function PulseLogo() {
  return (
    <svg viewBox="0 0 24 24" fill="none" className="h-5 w-5 text-emerald-400">
      <path
        d="M3 12h4l2-6 4 12 2-6h6"
        stroke="currentColor"
        strokeWidth="1.8"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  );
}

function StatusPill({ status }: { status: SimStatus | null }) {
  let label = 'Connecting';
  let classes = 'border-slate-700 bg-slate-800/60 text-slate-400';
  let dot = 'bg-slate-500';
  let pulse = false;

  if (status) {
    if (status.running) {
      label = 'Live';
      classes = 'border-emerald-500/30 bg-emerald-500/10 text-emerald-300';
      dot = 'bg-emerald-400';
      pulse = true;
    } else if (status.finished) {
      label = 'Finished';
      classes = 'border-sky-500/30 bg-sky-500/10 text-sky-300';
      dot = 'bg-sky-400';
    } else if (status.paused) {
      label = 'Paused';
      classes = 'border-amber-500/30 bg-amber-500/10 text-amber-300';
      dot = 'bg-amber-400';
    } else {
      label = 'Standby';
      classes = 'border-slate-700 bg-slate-800/60 text-slate-400';
      dot = 'bg-slate-500';
    }
  }

  return (
    <span
      className={`inline-flex items-center gap-2 rounded-full border px-3 py-1.5 text-xs font-medium ${classes}`}
    >
      <span className={`h-2 w-2 rounded-full ${dot} ${pulse ? 'animate-soft-pulse' : ''}`} />
      {label}
    </span>
  );
}

function ScenarioCard({
  config,
  status,
}: {
  config: ScenarioConfig | null;
  status: SimStatus | null;
}) {
  if (!config) {
    return null;
  }
  const facts = [
    { label: 'Rooms', value: config.rooms.length },
    { label: 'Dependency rules', value: config.dependencies.length },
    { label: 'Packages', value: config.packages.length },
    { label: 'Walk time', value: `${config.walk_time_min.toFixed(0)} min` },
  ];
  return (
    <section className="rounded-2xl border border-slate-800 bg-slate-900/40 p-4 shadow-xl shadow-black/20">
      <h2 className="text-sm font-semibold tracking-tight text-white">Demo scenario</h2>
      <p className="mt-1 text-xs text-slate-400">
        A full day of mixed checkup packages, routed visit by visit.
      </p>
      <dl className="mt-3 grid grid-cols-2 gap-2">
        {facts.map((fact) => (
          <div
            key={fact.label}
            className="rounded-lg border border-slate-800 bg-slate-950/40 px-3 py-2"
          >
            <dt className="text-[10px] uppercase tracking-wide text-slate-500">
              {fact.label}
            </dt>
            <dd className="mt-0.5 text-sm font-semibold text-slate-200">{fact.value}</dd>
          </div>
        ))}
      </dl>
      <p className="mt-3 border-t border-slate-800/80 pt-3 text-[11px] leading-relaxed text-slate-500">
        Seed {status?.seed !== null && status?.seed !== undefined ? String(status.seed) : 'generated at start'}.
        Learned room times persist in <span className="text-slate-400">backend/data/</span> and
        carry into the next run.
      </p>
    </section>
  );
}

export default function App() {
  const [config, setConfig] = useState<ScenarioConfig | null>(null);
  const [configError, setConfigError] = useState<string | null>(null);
  const [strategy, setStrategy] = useState<Strategy>('optimized');
  const [speed, setSpeed] = useState(5);
  const [actionError, setActionError] = useState<string | null>(null);
  const [comparing, setComparing] = useState(false);
  const [report, setReport] = useState<CompareResponse | null>(null);

  useEffect(() => {
    let active = true;
    api
      .getConfig()
      .then((result) => {
        if (active) setConfig(result);
      })
      .catch((caught: unknown) => {
        if (active) {
          setConfigError(caught instanceof Error ? caught.message : 'Could not load config');
        }
      });
    return () => {
      active = false;
    };
  }, []);

  const fetchDashboard = useCallback(async (): Promise<DashboardData> => {
    const [status, rooms, patients, history] = await Promise.all([
      api.getStatus(),
      api.getRooms(),
      api.getPatients(),
      api.getHistory(),
    ]);
    return { status, rooms, patients, history };
  }, []);

  const { data, error: pollingError, loading, refresh } = usePolling(fetchDashboard, 1000);
  const status = data?.status ?? null;
  const rooms = data?.rooms ?? [];
  const patients = data?.patients ?? [];
  const history = data?.history ?? [];
  const metrics = status?.metrics ?? EMPTY_METRICS;
  const started = status?.started ?? false;

  useEffect(() => {
    if (!started) {
      return;
    }
    const timer = window.setTimeout(() => {
      api.setSpeed(speed).catch((caught: unknown) => {
        setActionError(caught instanceof Error ? caught.message : 'Could not change speed');
      });
    }, 250);
    return () => window.clearTimeout(timer);
  }, [speed, started]);

  const runAction = useCallback(
    async (action: () => Promise<unknown>) => {
      setActionError(null);
      try {
        await action();
        await refresh();
      } catch (caught) {
        setActionError(caught instanceof Error ? caught.message : 'Action failed');
      }
    },
    [refresh],
  );

  const handleStart = () => {
    void runAction(() => api.startSim({ strategy, speed }));
  };
  const handlePause = () => {
    void runAction(() => api.pauseSim());
  };
  const handleResume = () => {
    void runAction(() => api.resumeSim());
  };
  const handleReset = () => {
    void runAction(() => api.resetSim());
  };
  const handleCompare = async () => {
    setComparing(true);
    setActionError(null);
    try {
      setReport(await api.compare());
    } catch (caught) {
      setActionError(caught instanceof Error ? caught.message : 'Comparison failed');
    } finally {
      setComparing(false);
    }
  };

  const error = configError ?? pollingError ?? actionError;
  const retry = () => {
    setActionError(null);
    setConfigError(null);
    void refresh();
    api.getConfig().then(setConfig).catch(() => undefined);
  };

  return (
    <div className="min-h-screen text-slate-100">
      <header className="sticky top-0 z-30 border-b border-slate-800/80 bg-slate-950/80 backdrop-blur">
        <div className="mx-auto flex max-w-[1600px] flex-wrap items-center justify-between gap-3 px-4 py-3.5 lg:px-8">
          <div className="flex items-center gap-3">
            <div className="flex h-10 w-10 items-center justify-center rounded-xl border border-emerald-500/30 bg-emerald-500/10">
              <PulseLogo />
            </div>
            <div>
              <h1 className="text-lg font-semibold tracking-tight text-white">MediRoute</h1>
              <p className="text-[11px] text-slate-400">
                Real-time hospital patient route optimizer
              </p>
            </div>
          </div>
          <div className="flex items-center gap-3">
            <div className="hidden rounded-xl border border-slate-800 bg-slate-900/70 px-3 py-1.5 text-right sm:block">
              <p className="text-[9px] font-medium uppercase tracking-[0.14em] text-slate-500">
                Sim clock
              </p>
              <p className="text-sm font-semibold tabular-nums text-slate-200">
                {formatMinutes(status?.sim_time_min ?? 0, 0)}
              </p>
            </div>
            <StatusPill status={status} />
          </div>
        </div>
      </header>

      <main className="mx-auto max-w-[1600px] space-y-4 px-4 py-5 lg:px-8">
        {error && (
          <div className="flex flex-wrap items-center justify-between gap-3 rounded-xl border border-rose-500/30 bg-rose-500/10 px-4 py-3 text-sm text-rose-200">
            <span>{error}</span>
            <button
              type="button"
              onClick={retry}
              className="rounded-lg border border-rose-400/30 bg-rose-500/10 px-2.5 py-1 text-xs font-medium text-rose-100 transition hover:bg-rose-500/20"
            >
              Retry
            </button>
          </div>
        )}

        <Controls
          status={status}
          strategy={strategy}
          speed={speed}
          comparing={comparing}
          onStrategyChange={setStrategy}
          onSpeedChange={setSpeed}
          onStart={handleStart}
          onPause={handlePause}
          onResume={handleResume}
          onReset={handleReset}
          onCompare={handleCompare}
        />

        {loading && !data ? (
          <div className="flex items-center justify-center gap-3 rounded-2xl border border-slate-800 bg-slate-900/40 py-24 text-sm text-slate-400">
            <span className="h-5 w-5 animate-spin rounded-full border-2 border-emerald-500/30 border-t-emerald-400" />
            Connecting to the simulation service…
          </div>
        ) : (
          <>
            <MetricsBar metrics={metrics} />
            <div className="grid gap-4 xl:grid-cols-[minmax(0,1fr)_390px]">
              <RoomGrid rooms={rooms} />
              <div className="space-y-4">
                <HistoryPanel history={history} />
                <ScenarioCard config={config} status={status} />
              </div>
            </div>
            <PatientTable patients={patients} />
            {report && <ReportView report={report} onClose={() => setReport(null)} />}
          </>
        )}

        <footer className="pb-4 pt-1 text-center text-[11px] text-slate-600">
          MediRoute · discrete-event routing with EWMA service-time learning · data persists in
          backend/data
        </footer>
      </main>
    </div>
  );
}
