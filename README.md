# MediRoute — Hospital Patient Route Optimizer

MediRoute tells each hospital patient where to go next. A discrete-event simulation drives nine
specialty rooms with uneven congestion, prerequisite visits, and parallel service stations, while
three routing strategies compete on the same workload. The dashboard shows every queue live and —
unlike static routing demos — the optimizer learns each room's real service time from recorded
visits, so its estimates keep improving as the day goes on.

## What it does

- **Real-time routing**: after each checkup the patient's next room is re-decided from live queue
  lengths, in-service patients, patients already walking there, learned service times, and the
  dependency DAG.
- **Continuous learning**: every completed service writes a `VisitRecord` with actual wait and
  actual service time. A room's estimate updates with EWMA (alpha 0.3); configured values are only
  cold-start seeds. The simulator's true service means intentionally differ from those seeds so
  convergence is visible.
- **Fair strategy comparison**: Optimized, Fixed Order, and Shortest Queue run headless on the
  same seed, arrivals, packages, and sampled service durations. All start from the same learned
  stats. Results include avg total time, avg wait, p90, room utilization, and makespan.
- **Persistence**: learned stats and up to 1,000 recent visit records survive backend restarts
  (`backend/data/learning.json`, written atomically and throttled).

## Quick start

Requirements: Rust (stable) and Node.js 18+ with npm.

Run the backend (port 8080):

```bash
cd backend
cargo run
```

Run the frontend dev server (port 5173, proxies `/api` to the backend):

```bash
cd frontend
npm install
npm run dev
```

Open <http://localhost:5173>.

For a single-server production-style run:

```bash
cd frontend
npm install
npm run build
cd ../backend
cargo run
```

The backend serves `frontend/dist` at <http://localhost:8080> when it exists.

## Using the dashboard

1. Pick a strategy and press **Start simulation**.
2. Adjust **Simulation speed** (1–30 simulated minutes per real second). Pause and resume at any
   time; **Reset** returns to a fresh world at the current learned stats.
3. Watch room cards change color with expected wait, queues drain and refill, and the
   **learned service estimate** on each card move away from the initial seed as observations
   accumulate.
4. **Run comparison** executes a headless benchmark of all three strategies on a fixed seed and
   opens the report with charts and per-strategy metrics.

The strategy selector is locked while a run is active — reset or start a new run to switch.

## How routing works

When a patient arrives or finishes a checkup:

1. **Eligibility**: required rooms that are not done and whose prerequisites (restricted to rooms
   in that patient's package) are complete.
2. **Cost**: `est_wait + est_service + walk_time`, where
   `est_wait = people_ahead × est_service / capacity` and `people_ahead` counts in-queue,
   in-service, pending starts, and en-route patients. The walk time is 2 minutes.
3. **Choice**: lowest cost. Ties prefer the room that unblocks more dependent rooms, then fewer
   en-route patients, shorter queue, and earlier package position.

Strategies:

| Strategy | Behavior |
| --- | --- |
| `optimized` | Cost model above using learned service estimates |
| `fixed_order` | Package order (topologically reordered), ignores congestion |
| `shortest_queue` | Myopic baseline: eligible room with the smallest current queue |

## Demo scenario

Nine rooms: Blood Test (capacity 3), X-Ray (2), Ultrasound (2), ECG (1), Cardiology (1),
Orthopedics (1), Endocrinology (1), ENT (1), Eye Exam (2).

Dependencies: Blood Test → Endocrinology, Blood Test → Cardiology, X-Ray → Orthopedics,
ECG → Cardiology. A dependency applies only when both rooms are in the patient's package.

Five packages: General Checkup, Cardiac Screen, Joint Pain, Thyroid Check, Full Premium. A seeded
run creates 52 patients arriving over roughly 88 simulated minutes.

## API

All endpoints are under `/api` and return JSON.

| Method | Endpoint | Purpose |
| --- | --- | --- |
| GET | `/config` | Rooms, dependencies, packages |
| POST | `/sim/start` | `{ "strategy": "optimized", "speed": 5, "seed"?: 42, "patient_count"?: 52 }` |
| POST | `/sim/pause` / `/sim/resume` / `/sim/reset` | Live simulation controls |
| POST | `/sim/speed` | `{ "speed": 10 }` — update speed while running |
| GET | `/sim/status` | Clock, run state, metrics, strategy, seed |
| POST | `/sim/compare` | `{ "seed": 42, "patient_count"?: 52 }` — headless run of all strategies |
| GET | `/rooms` | Live congestion and learned stats per room |
| GET | `/patients` | Status, progress, elapsed time, per-room actuals |
| GET | `/history` | Recent visit records (actual wait and service time) |

## Tests and build

```bash
cd backend && cargo test
cd frontend && npm run build
```

The backend suite (24 tests) covers DAG validation and cycle detection, dependency-aware
eligibility, congestion and en-route-aware routing, the learned-vs-seed estimate, EWMA convergence,
persistence round-trips, seeded determinism, all-patients-depart completion, single-room packages,
and Optimized beating Fixed Order on a fixed seed.

## Project layout

```
backend/src/
  main.rs        server, static serving, simulation driver
  domain.rs      rooms, patients, packages, dependency graph validation
  scenario.rs    built-in demo hospital
  api.rs         REST handlers and shared live state
  engine/
    router.rs    three strategies
    congestion.rs wait/cost model
    sim.rs       discrete-event world
    learn.rs     visit log, EWMA, JSON persistence
    stats.rs     metrics aggregation
frontend/src/
  App.tsx, api.ts, types.ts
  hooks/usePolling.ts
  components/    RoomGrid, PatientTable, MetricsBar, Controls, HistoryPanel, ReportView
```

## Resetting learned data

Stop the backend and remove `backend/data/learning.json`, then start it again. The room estimates
will begin at their configured seed values and converge during the next run. The file is
gitignored, so a fresh clone also starts cold.
