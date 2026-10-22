import type { Metrics } from '../types';
import { formatInt, formatMinutes } from '../lib/format';

interface MetricsBarProps {
  metrics: Metrics;
}

const CARD_STYLES = [
  'from-emerald-500/10 to-transparent border-emerald-500/20',
  'from-sky-500/10 to-transparent border-sky-500/20',
  'from-amber-500/10 to-transparent border-amber-500/20',
  'from-violet-500/10 to-transparent border-violet-500/20',
];

export default function MetricsBar({ metrics }: MetricsBarProps) {
  const cards = [
    {
      label: 'In hospital',
      value: formatInt(metrics.in_hospital),
      hint: `${formatInt(metrics.arrived)} arrived so far`,
    },
    {
      label: 'Completed',
      value: formatInt(metrics.completed),
      hint: `of ${formatInt(metrics.total_patients)} patients`,
    },
    {
      label: 'Avg total time',
      value: formatMinutes(metrics.avg_total_min),
      hint: 'arrival to discharge',
    },
    {
      label: 'Avg wait',
      value: formatMinutes(metrics.avg_wait_min),
      hint: `p90 wait ${formatMinutes(metrics.p90_wait_min)}`,
    },
  ];

  return (
    <section className="grid grid-cols-2 gap-3 lg:grid-cols-4">
      {cards.map((card, index) => (
        <div
          key={card.label}
          className={`rounded-2xl border bg-gradient-to-br p-4 shadow-lg shadow-black/10 ${CARD_STYLES[index]}`}
        >
          <p className="text-[11px] font-medium uppercase tracking-[0.14em] text-slate-400">
            {card.label}
          </p>
          <p className="mt-2 text-2xl font-semibold tracking-tight text-white">
            {card.value}
          </p>
          <p className="mt-1 text-xs text-slate-400">{card.hint}</p>
        </div>
      ))}
    </section>
  );
}
