import type { HistoryEntry } from '../types';

interface HistoryPanelProps {
  history: HistoryEntry[];
}

export default function HistoryPanel({ history }: HistoryPanelProps) {
  return (
    <section className="rounded-2xl border border-slate-800 bg-slate-900/40 shadow-xl shadow-black/20">
      <div className="border-b border-slate-800 px-4 py-4">
        <div className="flex items-center justify-between gap-2">
          <h2 className="text-sm font-semibold tracking-tight text-white">Recorded actuals</h2>
          <span className="rounded-full border border-emerald-500/25 bg-emerald-500/10 px-2 py-0.5 text-[10px] font-medium uppercase tracking-[0.1em] text-emerald-300">
            Live
          </span>
        </div>
        <p className="mt-1 text-xs text-slate-400">
          Real per-visit wait and service times feeding the EWMA learner.
        </p>
      </div>

      {history.length === 0 ? (
        <div className="px-4 py-10 text-center text-sm text-slate-500">
          Completed checkups will be logged here with their actual times.
        </div>
      ) : (
        <div className="max-h-[420px] overflow-auto">
          <table className="w-full border-collapse text-left text-xs">
            <thead className="sticky top-0 bg-slate-900/95 text-[10px] uppercase tracking-[0.12em] text-slate-500 backdrop-blur">
              <tr>
                <th className="px-4 py-2.5 font-medium">Patient</th>
                <th className="px-2 py-2.5 font-medium">Room</th>
                <th className="px-2 py-2.5 text-right font-medium">Wait</th>
                <th className="px-4 py-2.5 text-right font-medium">Service</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-slate-800/60">
              {history.slice(0, 20).map((entry, index) => (
                <tr
                  key={`${entry.patient_id}-${entry.room_id}-${entry.service_end}-${index}`}
                  className="transition-colors hover:bg-slate-800/30"
                >
                  <td className="max-w-[110px] truncate px-4 py-2.5 text-slate-300">
                    {entry.patient_name}
                  </td>
                  <td className="px-2 py-2.5 text-slate-400">{entry.room_name}</td>
                  <td className="px-2 py-2.5 text-right tabular-nums text-slate-400">
                    {entry.wait_min.toFixed(1)}m
                  </td>
                  <td className="px-4 py-2.5 text-right font-semibold tabular-nums text-slate-200">
                    {entry.service_min.toFixed(1)}m
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
      <div className="border-t border-slate-800/80 px-4 py-2.5 text-[11px] text-slate-500">
        Showing latest {Math.min(history.length, 20)} of {history.length} recent records.
      </div>
    </section>
  );
}
