import type { RoomView } from '../types';
import { clamp } from '../lib/format';

interface RoomGridProps {
  rooms: RoomView[];
}

function heatStyle(estWait: number): {
  background: string;
  borderColor: string;
  barColor: string;
} {
  const intensity = clamp(estWait / 30, 0, 1);
  const hue = 158 - 158 * intensity;
  return {
    background: `linear-gradient(150deg, hsla(${hue}, 48%, 23%, 0.72), hsla(${hue}, 42%, 9%, 0.9))`,
    borderColor: `hsla(${hue}, 55%, 50%, ${0.22 + intensity * 0.3})`,
    barColor: `hsl(${hue}, 65%, 55%)`,
  };
}

export default function RoomGrid({ rooms }: RoomGridProps) {
  return (
    <section className="rounded-2xl border border-slate-800 bg-slate-900/40 p-4 shadow-xl shadow-black/20 sm:p-5">
      <div className="flex flex-wrap items-end justify-between gap-3">
        <div>
          <h2 className="text-sm font-semibold tracking-tight text-white">Room congestion</h2>
          <p className="mt-1 text-xs text-slate-400">
            Live heat map with learned service estimates, not just configured seeds.
          </p>
        </div>
        <div className="flex items-center gap-2 text-[11px] text-slate-400">
          <span>Low wait</span>
          <span className="h-2 w-24 rounded-full bg-gradient-to-r from-emerald-500 via-amber-400 to-rose-500" />
          <span>High wait</span>
        </div>
      </div>

      {rooms.length === 0 ? (
        <div className="mt-5 rounded-xl border border-dashed border-slate-700 px-4 py-10 text-center text-sm text-slate-500">
          Room activity will appear when the simulation starts.
        </div>
      ) : (
        <div className="mt-4 grid gap-3 sm:grid-cols-2 xl:grid-cols-3">
          {rooms.map((room) => {
            const heat = heatStyle(room.est_wait_min);
            const deltaPct =
              room.initial_avg_service_min > 0
                ? ((room.est_service_min - room.initial_avg_service_min) /
                    room.initial_avg_service_min) *
                  100
                : 0;
            const deltaPositive = deltaPct >= 0;
            return (
              <article
                key={room.id}
                className="relative overflow-hidden rounded-xl border p-4 transition-colors"
                style={{ background: heat.background, borderColor: heat.borderColor }}
              >
                <div className="flex items-start justify-between gap-2">
                  <div className="min-w-0">
                    <h3 className="truncate text-sm font-semibold text-white">{room.name}</h3>
                    <p className="mt-0.5 text-[11px] text-slate-400">
                      {room.capacity} parallel {room.capacity === 1 ? 'station' : 'stations'}
                    </p>
                  </div>
                  <span className="shrink-0 rounded-full border border-white/10 bg-black/20 px-2 py-0.5 text-[10px] font-medium uppercase tracking-[0.1em] text-slate-300">
                    {room.visit_count} obs
                  </span>
                </div>

                <div className="mt-3 flex items-end gap-2">
                  <p className="text-2xl font-semibold tracking-tight text-white">
                    {room.est_wait_min.toFixed(1)}
                  </p>
                  <span className="pb-0.5 text-xs text-slate-300">min est. wait</span>
                </div>
                <div className="mt-2 h-1.5 overflow-hidden rounded-full bg-black/30">
                  <div
                    className="h-full rounded-full transition-all duration-500"
                    style={{
                      width: `${Math.max(3, clamp(room.est_wait_min / 30, 0, 1) * 100)}%`,
                      backgroundColor: heat.barColor,
                    }}
                  />
                </div>

                <dl className="mt-3 grid grid-cols-3 gap-2 text-center">
                  <div className="rounded-lg bg-black/20 px-2 py-1.5">
                    <dt className="text-[10px] uppercase tracking-wide text-slate-400">Queue</dt>
                    <dd className="text-sm font-semibold text-white">{room.queue_len}</dd>
                  </div>
                  <div className="rounded-lg bg-black/20 px-2 py-1.5">
                    <dt className="text-[10px] uppercase tracking-wide text-slate-400">Service</dt>
                    <dd className="text-sm font-semibold text-white">
                      {room.in_service}/{room.capacity}
                    </dd>
                  </div>
                  <div className="rounded-lg bg-black/20 px-2 py-1.5">
                    <dt className="text-[10px] uppercase tracking-wide text-slate-400">En route</dt>
                    <dd className="text-sm font-semibold text-white">{room.en_route}</dd>
                  </div>
                </dl>

                <div className="mt-3 border-t border-white/10 pt-3">
                  <div className="flex items-center justify-between gap-2 text-xs">
                    <span className="text-slate-300">
                      Learned{' '}
                      <span className="font-semibold text-white">
                        {room.est_service_min.toFixed(1)}m
                      </span>
                    </span>
                    <span
                      className={
                        deltaPositive ? 'text-amber-300' : 'text-emerald-300'
                      }
                    >
                      {deltaPositive ? '+' : ''}
                      {deltaPct.toFixed(0)}% vs seed
                    </span>
                  </div>
                  <div className="mt-1 flex items-center justify-between gap-2 text-[11px] text-slate-400">
                    <span>seed {room.initial_avg_service_min.toFixed(1)}m</span>
                    <span>
                      obs. wait {room.est_wait_observed_avg.toFixed(1)}m ·{' '}
                      {room.visit_count} samples
                    </span>
                  </div>
                  <div className="mt-2 flex items-center gap-2">
                    <div className="h-1 flex-1 overflow-hidden rounded-full bg-black/30">
                      <div
                        className="h-full rounded-full bg-white/50"
                        style={{ width: `${Math.min(100, room.utilization_pct)}%` }}
                      />
                    </div>
                    <span className="text-[10px] text-slate-400">
                      {room.utilization_pct.toFixed(0)}% utilized
                    </span>
                  </div>
                </div>
              </article>
            );
          })}
        </div>
      )}
    </section>
  );
}
