import type { ReactNode } from 'react';
import {
  Bar,
  BarChart,
  CartesianGrid,
  Cell,
  Legend,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
} from 'recharts';
import type { CompareResponse, Strategy } from '../types';
import { formatInt, formatMinutes } from '../lib/format';

interface ReportViewProps {
  report: CompareResponse;
  onClose: () => void;
}

const COLORS: Record<Strategy, string> = {
  optimized: '#34d399',
  fixed_order: '#fbbf24',
  shortest_queue: '#fb7185',
};

const AXIS_TICK = { fill: '#94a3b8', fontSize: 11 };
const TOOLTIP_STYLE = {
  backgroundColor: '#0f172a',
  border: '1px solid #334155',
  borderRadius: '12px',
  color: '#e2e8f0',
  fontSize: 12,
};

interface ChartCardProps {
  title: string;
  subtitle: string;
  children: ReactNode;
  className?: string;
}

function ChartCard({ title, subtitle, children, className = '' }: ChartCardProps) {
  return (
    <div className={`rounded-xl border border-slate-800 bg-slate-950/40 p-4 ${className}`}>
      <h3 className="text-sm font-semibold text-white">{title}</h3>
      <p className="mt-0.5 text-[11px] text-slate-500">{subtitle}</p>
      <div className="mt-3 h-52">{children}</div>
    </div>
  );
}

interface StrategyBarChartProps {
  data: { name: string; value: number; color: string }[];
  unit: string;
}

function StrategyBarChart({ data, unit }: StrategyBarChartProps) {
  return (
    <ResponsiveContainer width="100%" height="100%">
      <BarChart data={data} margin={{ top: 6, right: 6, left: 0, bottom: 0 }}>
        <CartesianGrid stroke="#1e293b" strokeDasharray="3 3" vertical={false} />
        <XAxis dataKey="name" tick={AXIS_TICK} axisLine={false} tickLine={false} />
        <YAxis
          tick={AXIS_TICK}
          axisLine={false}
          tickLine={false}
          width={52}
          unit={unit}
        />
        <Tooltip
          contentStyle={TOOLTIP_STYLE}
          labelStyle={{ color: '#e2e8f0' }}
          cursor={{ fill: 'rgba(30, 41, 59, 0.35)' }}
        />
        <Bar dataKey="value" radius={[6, 6, 0, 0]} maxBarSize={56}>
          {data.map((entry) => (
            <Cell key={entry.name} fill={entry.color} />
          ))}
        </Bar>
      </BarChart>
    </ResponsiveContainer>
  );
}

interface UtilizationRow {
  room: string;
  optimized: number;
  fixed_order: number;
  shortest_queue: number;
}

export default function ReportView({ report, onClose }: ReportViewProps) {
  const { results } = report;
  const optimized = results.find((result) => result.strategy === 'optimized');
  const baselines = results.filter((result) => result.strategy !== 'optimized');
  const bestBaseline = baselines.length
    ? Math.min(...baselines.map((result) => result.metrics.avg_total_min))
    : 0;
  const improvementPct =
    optimized && bestBaseline > 0
      ? ((bestBaseline - optimized.metrics.avg_total_min) / bestBaseline) * 100
      : 0;

  const kpis = [
    {
      title: 'Average total time',
      subtitle: 'Arrival to discharge, per patient',
      data: results.map((result) => ({
        name: result.label,
        value: result.metrics.avg_total_min,
        color: COLORS[result.strategy],
      })),
      unit: 'm',
    },
    {
      title: 'Average wait',
      subtitle: 'Time spent waiting before each service',
      data: results.map((result) => ({
        name: result.label,
        value: result.metrics.avg_wait_min,
        color: COLORS[result.strategy],
      })),
      unit: 'm',
    },
    {
      title: 'P90 total time',
      subtitle: 'The slowest 10% of patient journeys',
      data: results.map((result) => ({
        name: result.label,
        value: result.metrics.p90_total_min,
        color: COLORS[result.strategy],
      })),
      unit: 'm',
    },
  ];

  const hasPriorityMetrics = results.some((result) =>
    [
      result.metrics.avg_wait_urgent_min,
      result.metrics.p90_wait_urgent_min,
      result.metrics.avg_wait_normal_min,
      result.metrics.p90_wait_normal_min,
    ].some((value) => typeof value === 'number'),
  );

  const roomRows = optimized?.metrics.room_utilization ?? results[0]?.metrics.room_utilization ?? [];
  const utilizationData: UtilizationRow[] = roomRows.map((room) => {
    const findUtilization = (strategy: Strategy) =>
      results
        .find((result) => result.strategy === strategy)
        ?.metrics.room_utilization.find((entry) => entry.room_id === room.room_id)
        ?.utilization_pct ?? 0;
    return {
      room: room.room_name,
      optimized: findUtilization('optimized'),
      fixed_order: findUtilization('fixed_order'),
      shortest_queue: findUtilization('shortest_queue'),
    };
  });

  return (
    <section className="rounded-2xl border border-sky-500/25 bg-slate-900/50 p-4 shadow-2xl shadow-sky-950/30 sm:p-5">
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div>
          <div className="flex items-center gap-2">
            <span className="rounded-full border border-sky-500/30 bg-sky-500/10 px-2 py-0.5 text-[10px] font-medium uppercase tracking-[0.12em] text-sky-300">
              Headless benchmark
            </span>
            <span className="text-[11px] text-slate-500">
              seed {String(report.seed)} · {formatInt(report.patient_count)} patients
            </span>
          </div>
          <h2 className="mt-2 text-lg font-semibold tracking-tight text-white">
            Strategy comparison
          </h2>
          <p className="mt-1 text-xs text-slate-400">
            All strategies ran on the same arrivals, packages, and sampled service times,
            starting from the same learned estimates.
          </p>
        </div>
        <button
          type="button"
          onClick={onClose}
          className="rounded-lg border border-slate-700 bg-slate-800/60 px-2.5 py-1.5 text-xs font-medium text-slate-300 transition hover:border-slate-600 hover:text-white"
        >
          Close
        </button>
      </div>

      {optimized && (
        <div className="mt-4 flex flex-wrap items-center gap-x-4 gap-y-2 rounded-xl border border-emerald-500/20 bg-emerald-500/[0.07] px-4 py-3">
          <p className="text-3xl font-semibold tracking-tight text-emerald-300">
            {improvementPct > 0.05
              ? `${improvementPct.toFixed(1)}%`
              : improvementPct < -0.05
                ? `${Math.abs(improvementPct).toFixed(1)}%`
                : 'Best'}
          </p>
          <p className="max-w-2xl text-xs text-emerald-100/80">
            {improvementPct > 0.05
              ? 'faster average total time for Optimized versus the best baseline in this run.'
              : improvementPct < -0.05
                ? 'slower average total time for Optimized versus the best baseline in this run; short runs with bursty congestion can erase routing gains.'
                : 'Optimized matched the best baseline in this run. Routing gains vary with congestion.'}{' '}
            <span className="text-emerald-200/60">
              {formatMinutes(optimized.metrics.avg_total_min)} avg total vs{' '}
              {formatMinutes(bestBaseline)} baseline.
            </span>
          </p>
        </div>
      )}

      <div className="mt-4 grid gap-3 lg:grid-cols-3">
        {kpis.map((kpi) => (
          <ChartCard key={kpi.title} title={kpi.title} subtitle={kpi.subtitle}>
            <StrategyBarChart data={kpi.data} unit={kpi.unit} />
          </ChartCard>
        ))}
      </div>

      <div className="mt-3">
        <ChartCard
          title="Room utilization"
          subtitle="Share of available service capacity consumed during each run"
        >
          <ResponsiveContainer width="100%" height="100%">
            <BarChart
              data={utilizationData}
              margin={{ top: 6, right: 8, left: 0, bottom: 18 }}
            >
              <CartesianGrid stroke="#1e293b" strokeDasharray="3 3" vertical={false} />
              <XAxis
                dataKey="room"
                tick={{ ...AXIS_TICK, fontSize: 10 }}
                axisLine={false}
                tickLine={false}
                interval={0}
                angle={-18}
                textAnchor="end"
                height={48}
              />
              <YAxis
                tick={AXIS_TICK}
                axisLine={false}
                tickLine={false}
                width={52}
                unit="%"
                domain={[0, 100]}
              />
              <Tooltip
                contentStyle={TOOLTIP_STYLE}
                labelStyle={{ color: '#e2e8f0' }}
                cursor={{ fill: 'rgba(30, 41, 59, 0.35)' }}
              />
              <Legend
                wrapperStyle={{ color: '#94a3b8', fontSize: 11, paddingTop: 4 }}
              />
              {results.map((result) => (
                <Bar
                  key={result.strategy}
                  dataKey={result.strategy}
                  name={result.label}
                  fill={COLORS[result.strategy]}
                  radius={[4, 4, 0, 0]}
                  maxBarSize={22}
                />
              ))}
            </BarChart>
          </ResponsiveContainer>
        </ChartCard>
      </div>

      <div className="mt-3 overflow-x-auto rounded-xl border border-slate-800">
        <table className="w-full min-w-[820px] border-collapse text-left text-xs">
          <thead className="bg-slate-950/50 text-[10px] uppercase tracking-[0.12em] text-slate-500">
            <tr>
              <th className="px-4 py-2.5 font-medium">Strategy</th>
              <th className="px-3 py-2.5 text-right font-medium">Avg total</th>
              <th className="px-3 py-2.5 text-right font-medium">Avg wait</th>
              <th className="px-3 py-2.5 text-right font-medium">Avg walk</th>
              <th className="px-3 py-2.5 text-right font-medium">P90 total</th>
              <th className="px-3 py-2.5 text-right font-medium">Visits</th>
              <th className="px-3 py-2.5 text-right font-medium">Makespan</th>
              <th className="px-4 py-2.5 text-right font-medium">Completed</th>
            </tr>
          </thead>
          <tbody className="divide-y divide-slate-800/70">
            {results.map((result) => (
              <tr
                key={result.strategy}
                className={result.strategy === 'optimized' ? 'bg-emerald-500/[0.04]' : ''}
              >
                <td className="px-4 py-2.5">
                  <span className="inline-flex items-center gap-2 font-medium text-slate-200">
                    <span
                      className="h-2 w-2 rounded-full"
                      style={{ backgroundColor: COLORS[result.strategy] }}
                    />
                    {result.label}
                  </span>
                </td>
                <td className="px-3 py-2.5 text-right tabular-nums text-slate-300">
                  {formatMinutes(result.metrics.avg_total_min)}
                </td>
                <td className="px-3 py-2.5 text-right tabular-nums text-slate-400">
                  {formatMinutes(result.metrics.avg_wait_min)}
                </td>
                <td className="px-3 py-2.5 text-right tabular-nums text-slate-300">
                  {formatMinutes(result.metrics.avg_walk_min)}
                </td>
                <td className="px-3 py-2.5 text-right tabular-nums text-slate-400">
                  {formatMinutes(result.metrics.p90_total_min)}
                </td>
                <td className="px-3 py-2.5 text-right tabular-nums text-slate-400">
                  {formatInt(result.metrics.total_visits)}
                </td>
                <td className="px-3 py-2.5 text-right tabular-nums text-slate-400">
                  {formatMinutes(result.makespan_min, 0)}
                </td>
                <td className="px-4 py-2.5 text-right tabular-nums text-slate-300">
                  {result.metrics.completed}/{result.metrics.total_patients}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>

      {hasPriorityMetrics && (
        <div className="mt-3 overflow-x-auto rounded-xl border border-slate-800">
          <table className="w-full min-w-[640px] border-collapse text-left text-xs">
            <thead className="bg-slate-950/50 text-[10px] uppercase tracking-[0.12em] text-slate-500">
              <tr>
                <th className="px-4 py-2.5 font-medium">Wait by priority</th>
                <th className="px-3 py-2.5 text-right font-medium">Urgent avg</th>
                <th className="px-3 py-2.5 text-right font-medium">Urgent p90</th>
                <th className="px-3 py-2.5 text-right font-medium">Normal avg</th>
                <th className="px-4 py-2.5 text-right font-medium">Normal p90</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-slate-800/70">
              {results.map((result) => (
                <tr
                  key={result.strategy}
                  className={result.strategy === 'optimized' ? 'bg-emerald-500/[0.04]' : ''}
                >
                  <td className="px-4 py-2.5">
                    <span className="inline-flex items-center gap-2 font-medium text-slate-200">
                      <span
                        className="h-2 w-2 rounded-full"
                        style={{ backgroundColor: COLORS[result.strategy] }}
                      />
                      {result.label}
                    </span>
                  </td>
                  <td className="px-3 py-2.5 text-right tabular-nums text-rose-300">
                    {formatMinutes(result.metrics.avg_wait_urgent_min)}
                  </td>
                  <td className="px-3 py-2.5 text-right tabular-nums text-rose-300/80">
                    {formatMinutes(result.metrics.p90_wait_urgent_min)}
                  </td>
                  <td className="px-3 py-2.5 text-right tabular-nums text-slate-300">
                    {formatMinutes(result.metrics.avg_wait_normal_min)}
                  </td>
                  <td className="px-4 py-2.5 text-right tabular-nums text-slate-400">
                    {formatMinutes(result.metrics.p90_wait_normal_min)}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </section>
  );
}
