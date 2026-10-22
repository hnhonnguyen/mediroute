import type { PatientStatus, PatientView } from '../types';
import { formatMinutes } from '../lib/format';

interface PatientTableProps {
  patients: PatientView[];
}

const STATUS_META: Record<
  PatientStatus,
  { label: string; className: string; dot: string }
> = {
  arrived: {
    label: 'Arrived',
    className: 'border-slate-500/30 bg-slate-500/10 text-slate-300',
    dot: 'bg-slate-400',
  },
  en_route: {
    label: 'En route',
    className: 'border-sky-500/30 bg-sky-500/10 text-sky-300',
    dot: 'bg-sky-400',
  },
  in_queue: {
    label: 'In queue',
    className: 'border-amber-500/30 bg-amber-500/10 text-amber-300',
    dot: 'bg-amber-400',
  },
  in_service: {
    label: 'In service',
    className: 'border-emerald-500/30 bg-emerald-500/10 text-emerald-300',
    dot: 'bg-emerald-400',
  },
  departed: {
    label: 'Done',
    className: 'border-emerald-500/20 bg-emerald-500/5 text-emerald-300/80',
    dot: 'bg-emerald-500/60',
  },
};

const STATUS_ORDER: Record<PatientStatus, number> = {
  in_service: 0,
  in_queue: 1,
  en_route: 2,
  arrived: 3,
  departed: 4,
};

export default function PatientTable({ patients }: PatientTableProps) {
  const sorted = [...patients].sort((a, b) => {
    const byStatus = STATUS_ORDER[a.status] - STATUS_ORDER[b.status];
    if (byStatus !== 0) return byStatus;
    return (b.elapsed_min ?? -1) - (a.elapsed_min ?? -1);
  });
  const activeCount = patients.filter((patient) => patient.status !== 'departed').length;

  return (
    <section className="rounded-2xl border border-slate-800 bg-slate-900/40 shadow-xl shadow-black/20">
      <div className="flex flex-wrap items-center justify-between gap-2 border-b border-slate-800 px-4 py-4 sm:px-5">
        <div>
          <h2 className="text-sm font-semibold tracking-tight text-white">Patients</h2>
          <p className="mt-1 text-xs text-slate-400">
            Status, journey progress, and actual per-room times.
          </p>
        </div>
        <span className="rounded-full border border-slate-700 bg-slate-800/70 px-2.5 py-1 text-[11px] text-slate-300">
          {activeCount} active · {patients.length - activeCount} completed
        </span>
      </div>

      {patients.length === 0 ? (
        <div className="px-4 py-10 text-center text-sm text-slate-500">
          Patients appear here as soon as the simulation starts.
        </div>
      ) : (
        <div className="max-h-[560px] overflow-auto">
          <table className="w-full min-w-[900px] border-collapse text-left text-sm">
            <thead className="sticky top-0 z-10 bg-slate-900/95 text-[11px] uppercase tracking-[0.12em] text-slate-400 backdrop-blur">
              <tr>
                <th className="px-4 py-3 font-medium sm:px-5">Patient</th>
                <th className="px-3 py-3 font-medium">Status</th>
                <th className="px-3 py-3 font-medium">Progress</th>
                <th className="px-3 py-3 font-medium">Elapsed</th>
                <th className="px-3 py-3 font-medium">Actual room times</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-slate-800/70">
              {sorted.map((patient) => {
                const meta = STATUS_META[patient.status];
                const done = patient.rooms_done.length;
                const progress = patient.rooms_total
                  ? (done / patient.rooms_total) * 100
                  : 0;
                return (
                  <tr
                    key={patient.id}
                    className={
                      patient.status === 'departed'
                        ? 'bg-slate-950/20 text-slate-400'
                        : 'transition-colors hover:bg-slate-800/30'
                    }
                  >
                    <td className="px-4 py-3 align-top sm:px-5">
                      <p className="font-medium text-slate-100">{patient.name}</p>
                      <p className="mt-0.5 text-[11px] text-slate-500">
                        #{String(patient.id).padStart(2, '0')} · {patient.package_name}
                      </p>
                    </td>
                    <td className="px-3 py-3 align-top">
                      <span
                        className={`inline-flex items-center gap-1.5 rounded-full border px-2 py-0.5 text-[11px] font-medium ${meta.className}`}
                      >
                        <span className={`h-1.5 w-1.5 rounded-full ${meta.dot}`} />
                        {meta.label}
                      </span>
                      {patient.current_room_name && (
                        <p className="mt-1 text-[11px] text-slate-500">
                          {patient.current_room_name}
                        </p>
                      )}
                    </td>
                    <td className="px-3 py-3 align-top">
                      <div className="flex items-center gap-2">
                        <div className="h-1.5 w-20 overflow-hidden rounded-full bg-slate-800">
                          <div
                            className="h-full rounded-full bg-emerald-500/80"
                            style={{ width: `${progress}%` }}
                          />
                        </div>
                        <span className="text-[11px] tabular-nums text-slate-400">
                          {done}/{patient.rooms_total}
                        </span>
                      </div>
                      <p className="mt-1.5 max-w-[200px] truncate text-[11px] text-slate-500">
                        {patient.rooms_remaining_names.length > 0
                          ? `Next needs: ${patient.rooms_remaining_names.join(', ')}`
                          : 'All checkups complete'}
                      </p>
                    </td>
                    <td className="whitespace-nowrap px-3 py-3 align-top tabular-nums text-slate-300">
                      {formatMinutes(patient.elapsed_min)}
                    </td>
                    <td className="px-3 py-3 align-top">
                      {patient.visits.length === 0 ? (
                        <span className="text-xs text-slate-600">—</span>
                      ) : (
                        <div className="flex max-w-xl flex-wrap gap-1.5">
                          {patient.visits.map((visit) => (
                            <span
                              key={`${visit.room_id}-${visit.service_end}`}
                              className="inline-flex items-baseline gap-1 rounded-md border border-slate-700/70 bg-slate-800/60 px-1.5 py-0.5 text-[10px]"
                              title={`wait ${visit.wait_min.toFixed(1)} min`}
                            >
                              <span className="text-slate-400">{visit.room_name}</span>
                              <span className="font-semibold text-slate-200">
                                {visit.service_min.toFixed(1)}m
                              </span>
                              <span className="text-slate-500">
                                w{visit.wait_min.toFixed(1)}
                              </span>
                            </span>
                          ))}
                        </div>
                      )}
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      )}
    </section>
  );
}
