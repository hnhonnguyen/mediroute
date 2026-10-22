# MediRoute — Hospital Patient Route Optimizer

## Context

Hospital patients must visit multiple specialty rooms for checkups. Rooms have uneven congestion (some empty, some busy), some rooms require prerequisite rooms to be completed first (e.g., blood test before endocrinology consult), and each patient has their own required room list. MediRoute tells each patient **where to go next** in real time, minimizing their total time in hospital (arrival → all checkups done), and provides a live dashboard + simulation to prove the optimization works vs. naive routing.

Decisions made with user:
- **Backend: Rust** (Axum + tokio)
- **Frontend: React + Vite + TypeScript + Tailwind CSS**, dev server proxies to backend
- **Routing: real-time next-step** (re-decided after each checkup, based on live congestion + dependencies)
- **Primary goal: minimize patient total time**; comparison strategies included to demonstrate value
- **Continuous optimization**: actual per-patient times are recorded and used to keep improving estimates — service time is *not* a fixed constant; the system learns each room's real service time from observed data

## Architecture

```
mediroute/
├── backend/                  # Rust crate (single crate, no workspace needed)
│   ├── Cargo.toml            # axum, tokio, serde, serde_json, rand, thiserror
│   └── src/
│       ├── main.rs           # axum server, CORS, static file serving, sim task spawn
│       ├── domain.rs         # Room, Patient, PatientStatus, DependencyGraph, Scenario
│       ├── scenario.rs       # built-in demo scenario (rooms, deps, checkup packages)
│       ├── api.rs            # handlers + AppState
│       └── engine/
│           ├── mod.rs
│           ├── congestion.rs # per-room wait-time estimation (uses learned stats)
│           ├── router.rs     # routing strategies (enum + next_room())
│           ├── sim.rs        # discrete-event simulation World
│           ├── learn.rs      # VisitRecord log, EWMA room stats, persistence
│           └── stats.rs      # metrics aggregation
└── frontend/                 # Vite + React + TS + Tailwind
    ├── vite.config.ts        # proxy /api → http://localhost:8080
    └── src/
        ├── api.ts            # typed fetch client
        ├── types.ts          # shared API types
        ├── hooks/usePolling.ts
        └── components/       # RoomGrid, PatientTable, MetricsBar, Controls, ReportView, HistoryPanel
```

Ports: backend **8080**, frontend dev **5173**. Learned data persisted under `backend/data/`.

## Domain Model

**Room**: `id, name, initial_avg_service_min (from config, only a seed), capacity (parallel stations)`

**RoomStats (runtime, learned)**: `est_service_min (EWMA), est_wait_observed_avg, visit_count`
- Starts from the config seed; updated after every actual service completion.
- EWMA: `est = est * (1 - α) + actual * α`, α = 0.3. Cold start handled by seeding with the config value.

**VisitRecord** (saved per completed service): `{ patient_id, room_id, arrival_at_room, service_start, service_end, wait_min (actual), service_min (actual) }`

**Dependency**: global DAG between rooms: edge `A → B` means "B requires A first". Applies to a patient only when **both** A and B are in that patient's required list. Validated at startup (cycle → error).

**Patient**: `id, name, arrival_min, required_rooms: Vec<RoomId>, status, visit history`
Status lifecycle: `Arrived → EnRoute(room) → InQueue(room) → InService(room) → (next room …) → Departed`

**Walk time**: global constant `WALK_TIME_MIN = 2.0` (part of routing cost).

## Routing Engine (core algorithm)

When a patient checks in or finishes a checkup, the router picks the next room:

1. **Eligible rooms** = required rooms not yet done, whose prerequisite rooms (per DAG, restricted to the patient's list) are all done.
2. **Cost per eligible room** — uses **learned** estimates, not config constants:
   - `est_wait(r) = people_ahead(r) * est_service(r) / capacity(r)`
   - `people_ahead(r)` = in-queue + in-service + already en-route-to-r (counts assignments so recommendations don't herd everyone into one "empty" room)
   - `cost(r) = est_wait(r) + est_service(r) + WALK_TIME_MIN`
3. **Choose min cost**; tie-break: room that unblocks more dependent rooms (prune the critical path early).

**Strategies** (enum, same interface):
- `Optimized` — algorithm above, driven by learned stats (improves continuously as data accumulates)
- `FixedOrder` — baseline: visit rooms in package-defined order (topologically reordered to satisfy dependencies)
- `ShortestQueue` — myopic "human" baseline: eligible room with smallest current queue

## Continuous Learning

1. **Record actuals**: every queue-join, service-start, and service-end is timestamped; each completed service writes a `VisitRecord` with the patient's actual wait and actual service time.
2. **Update estimates**: after each completion, the room's `est_service_min` EWMA is updated; wait observations feed `est_wait_observed_avg` (diagnostic). All routing costs immediately use the updated values → decisions keep improving as the day goes on.
3. **True ≠ configured**: the simulator samples real durations from per-room distributions whose true means intentionally differ from the configured seed values (e.g., config says 10 min, true mean 13 min). The dashboard shows estimates converging toward reality — demonstrating continuous optimization.
4. **Persistence**: room stats + visit log are saved to `backend/data/` as JSON (snapshot throttled; loaded at startup) so learning survives restarts and accumulates across sessions.
5. **Visibility**: `GET /api/history` exposes recent visit records; RoomCards show learned estimate vs. initial seed and sample count.

## Simulation (discrete-event)

- Virtual clock in sim-minutes; event queue (BinaryHeap): `PatientArrival`, `QueueJoin`, `ServiceStart`, `ServiceEnd`, `Departure`.
- Background tokio task ticks every 100 ms; each tick advances sim clock by `speed * 0.1` sim-minutes (speed = sim-min per real second, user-controllable), processing all events due. Pause = stop advancing clock.
- Service durations sampled `uniform(true_mean*0.8, true_mean*1.2)` where `true_mean` differs from the configured seed. Room serves up to `capacity` patients in parallel; FIFO queue per room.
- Same seeded RNG → reproducible scenarios across strategies.
- Runs in two modes:
  - **Live** (dashboard): one world, chosen strategy, speed controls.
  - **Compare** (`POST /api/sim/compare`): runs all strategies headless on the same seed to completion, returns metrics for charting. (Each compare run starts from the same persisted learned stats so the comparison is fair.)

**Demo scenario** (built-in, seeded): 9 rooms — Blood Test (cap 3), X-Ray (cap 2), Ultrasound (cap 2), ECG (cap 1), Cardiology (cap 1), Orthopedics (cap 1), Endocrinology (cap 1), ENT (cap 1), Eye Exam (cap 2). Dependencies: Blood Test→Endocrinology, Blood Test→Cardiology, X-Ray→Orthopedics, ECG→Cardiology. Checkup packages (General Checkup, Cardiac Screen, Joint Pain, Thyroid Check, Full Premium) generate ~40–60 patients arriving over ~90 sim-minutes.

## API (JSON, all under /api)

| Endpoint | Purpose |
|---|---|
| `GET /config` | rooms, dependencies, packages |
| `POST /sim/start` | `{strategy, speed, seed?, patient_count?}` → starts live sim |
| `POST /sim/pause` / `POST /sim/resume` / `POST /sim/reset` | controls |
| `GET /sim/status` | running/paused, sim clock, live stats |
| `GET /rooms` | rooms + live congestion (queue len, in-service, est wait) + learned `est_service`, initial seed, visit_count |
| `GET /patients` | patients + status, done/remaining rooms, elapsed, per-room actual times |
| `GET /history` | recent VisitRecords (actual wait + actual service per patient per room) |
| `POST /sim/compare` | `{seed, patient_count?}` → headless run of all strategies, returns metrics report |

State shared via `Arc<RwLock<...>>`. Backend also serves `frontend/dist` if present (production convenience); CORS open in dev.

## Frontend (single dashboard page)

- **RoomGrid**: heat-map cards (green→red by est wait), showing queue length, in-service/capacity, est wait, and **learned service estimate vs. initial seed + sample count** (convergence indicator).
- **PatientTable**: status chips (EnRoute/InQueue/InService/Done), progress (rooms done/total), elapsed time, actual times per completed room.
- **MetricsBar**: patients in hospital, completed, avg total time, avg wait time.
- **Controls**: start/pause/reset, speed slider, strategy select, "Run comparison" button.
- **ReportView**: recharts bar charts comparing strategies (avg total time, avg wait, p90, room utilization).
- **HistoryPanel**: recent visit records table (patient, room, actual wait, actual service) — the "saved actual times".
- Live updates via 1 s polling (`usePolling` hook) while sim runs.

## Implementation Steps

1. Scaffold: `cargo init backend` (deps: axum, tokio, serde, serde_json, rand, thiserror); Vite React-TS frontend + Tailwind; vite proxy; verify both dev servers run.
2. `domain.rs` + `scenario.rs`: models (incl. `RoomStats`, `VisitRecord`), demo scenario, DAG cycle validation.
3. `engine/learn.rs`: visit log, EWMA updates, JSON persistence (save/load `backend/data/`).
4. `engine/router.rs` + `congestion.rs`: three strategies using learned estimates + unit tests (dependency blocking, congestion preference, topo order for FixedOrder, cycle detection, router uses learned value not seed).
5. `engine/sim.rs` + `stats.rs`: headless discrete-event world, seeded RNG, true-vs-seed service distributions, visit recording + EWMA hooks, metrics; tests (all patients depart; Optimized ≤ FixedOrder avg total time on a fixed seed; EWMA converges toward true mean).
6. `api.rs` + `main.rs`: endpoints incl. `/history`, shared state, live sim driver task (speed/pause), compare endpoint, CORS, static serving, load persisted stats at startup.
7. Frontend scaffold: types, api client, polling hook.
8. Dashboard components (RoomGrid with learning indicators, PatientTable, MetricsBar, Controls, HistoryPanel).
9. ReportView with recharts + wire up compare.
10. End-to-end manual verification + README (run instructions).

## Verification

- `cargo test` — router logic, DAG validation, sim determinism, EWMA convergence (est moves from seed toward true mean as visits accumulate), Optimized-beats-baseline on a seeded scenario.
- Manual: `cargo run` + `npm run dev` → start sim, watch heat map update, RoomCards' learned estimates converge away from seed values toward actuals, HistoryPanel fills with real per-patient times; run comparison → Optimized shows measurably lower avg total time.
- Verify persistence: restart backend → learned estimates and visit history survive (loaded from `backend/data/`).
- Golden path + edge cases: patient with single room, dependency-heavy package (Cardiac Screen), sim end (all patients depart, stats freeze).

## Notes / Future Extensions (out of scope for v1)

- Per-room walk-time distance matrix, patient priorities (elderly/emergency), time-of-day patterns, real integration with hospital systems (HL7/FHIR), ML-based service-time prediction beyond EWMA.
