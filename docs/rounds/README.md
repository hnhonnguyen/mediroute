# MediRoute — Improvement Rounds Log

Each round: propose an aspect → headless DeepSeek-Flash implements → verify (cargo test + compare seed 42) → commit if better, discard otherwise.

**Baseline (commit `9ecc85b`, seed 42, comparison report):**

| Strategy | avg_total | avg_wait | p90_total |
|---|---|---|---|
| Optimized | 101.1 | 13.4 | 146.9 |
| ShortestQueue | 104.5 | 14.3 | 159.3 |
| FixedOrder | 108.0 | 15.3 | 175.7 |

Live run (self-generated seed): 52/52 patients, avg_total 109.7, avg_wait 14.0, p90 199.1.

---

## Round 1 — Predictive routing + complex scenarios

**Status:** ✅ COMMITTED (verified independently)

**Proposed aspect:**
1. Smarter routing algorithm (Optimized only, baselines unchanged):
   - Predictive idle-time: predict when each capacity slot frees up (remaining service of in-service patients via `ends_at - now`), adding queue/pending/en-route load, downtime, and weighted anti-herding arrivals; equals 0 when the slot is actually idle
   - Anti-herding lookahead: estimate competitors (other eligible patients) that will arrive at the same room
   - Critical-path tie-break: within the 5% cost band, prefer the room that unblocks the most dependents, then the longest dependency chain
2. Complex scenarios:
   - Rush-hour arrivals: 65% of patients in the main peak 0–120 min, the rest in a secondary peak 150–210 min, with a lull in between
   - Room downtime: each room has a 15% chance of going offline for 10–20 min (seeded); in-service visits finish normally, the queue is held
   - Patient priorities: ~10% urgent; priority queue (urgent before normal, FIFO within a class)
   - Added metrics: avg/p90 wait for urgent and normal (additive, existing fields kept)
   - UI: RoomGrid card with gray stripes + countdown during downtime, PatientTable URGENT badge, ReportView "Wait by priority" table

**Files changed:** backend: domain.rs, scenario.rs, engine/{congestion,router,sim,stats}.rs, main.rs (adds PORT + MEDIROUTE_DATA_DIR env); frontend: types.ts, RoomGrid.tsx, PatientTable.tsx, ReportView.tsx; README.md

**Results (seed 42, independent verification via /api/sim/compare):**

| Strategy | avg_total | avg_wait | p90_total |
|---|---|---|---|
| **Optimized (after R1)** | **57.97** | **2.39** | **70.90** |
| ShortestQueue | 62.96 | 3.75 | 89.49 |
| FixedOrder | 67.52 | 4.99 | 114.44 |
| Optimized (initial baseline) | 101.1* | 13.4* | 146.9* |

*Note: the scenario also changed (rush bursts over 210 min instead of evenly spread arrivals), so an absolute comparison against the initial baseline includes both of the 2 factors; the internal comparison of the 3 strategies on the same new arrivals is the fair signal — Optimized is best on both avg and p90.

**Verify:** cargo test 41/41 pass (new tests: predictive-wait, anti-herding, critical-path, rush-hour, priority queue, downtime, priority metrics); npm run build OK; the agent live-tested all endpoints on port 8081 (isolated).

**Decision:** COMMIT — clearly better.

---

## Round 2 — Global assignment coordination (batch assignment)

**Status:** ❌ DISCARDED (normalized verification showed avg_total regression)

**Proposed aspect:**
- Currently the router decides sequentially per patient (each patient independently picks the lowest-cost room). When many patients need routing at the same moment (burst arrivals, simultaneous service ends), sequential decisions cause local herding — a later patient sees the queue of an earlier patient just increased.
- Batch assignment: group all patients needing routing in the same processing step and sync-optimize the allocation (for example greedy by ascending cost, updating a virtual room load for later assignments in the batch; or small brute force when batch ≤ 6 patients across all permutations of eligible rooms). Goal: further reduce avg_wait and avg_total under high contention (rush hours).

**Principle:** improve Optimized only; leave FixedOrder/ShortestQueue unchanged. Additive; do not break existing API/UI/tests.

**Design the agent implemented:** batch ≤ 6 patients and ≤ 4096 combinations → exhaustive minimization of total cost with a spread tie-break; larger batches → greedy + virtual room-load overlay; because event timestamps are continuous (batch-1 almost always), use a 0.5-minute coordination window: the current patient commits immediately, patients about to be routed within 0.5 minutes are planned/reserved ahead, and revalidated at commit.

**Normalized verification (process: fresh data dir → live run seed 99 → compare seed 42, repeated on both of the 2 code versions):**

| Metric | Round 1 | Round 2 | Δ |
|---|---|---|---|
| avg_total | 58.42 | 59.04 | +0.62 worse |
| avg_wait | 2.51 | 2.68 | +0.17 worse |
| p90_total | 75.95 | 71.96 | −3.99 better |
| p90_wait | 9.23 | 10.44 | +1.21 worse |

Results are consistent across 2 levels of learned stats (1 run, 2 runs). Only p90_total improved; avg_total/avg_wait/p90_wait were all slightly worse. Baselines unchanged (67.52/4.99/114.44 and 62.96/3.75/89.49).

cargo test 48/48 pass (new code), 41/41 after revert. Loop rule: must be better overall → **DISCARD**. Code reverted (`git checkout engine/{router,congestion,sim}.rs README.md`); this record is kept as documentation.

**Lessons learned:** the 0.5-minute coordination window with reservations helps the tail (p90_total −4 min) but plan/recheck adds routing cost that scatters patients away from the locally cost-optimal choice, slightly raising avg. If this direction is retried later, coordination should be limited to genuinely high contention (queue ≥ 2 at ≥ 2 eligible rooms).

---

## Round 3 — Distance-aware routing (walk-time matrix)

**Status:** ✅ COMMITTED (independently verified)

**Proposed aspect:**
1. Walk-time matrix based on physical distance:
   - `walk_time(from, to) = 1.0 + 0.25 × euclidean(from, to)`, clamp `[1.0, 5.0]` min
   - Each room has a `position` + `wing`; the entrance has its own position `(0, 0)`
   - 3 actual wings: Lab (Blood Test, ECG), Imaging (X-Ray, Ultrasound), Consultation (Cardiology, Orthopedics, Endocrinology, ENT, Eye Exam)
2. Patients have a `current_location` (entrance at arrival, updated to the just-completed room) and `total_walk_min`
3. SHARED PHYSICAL SIM for all 3 strategies: `EnRoute` duration = `walk_time(current_location, room)` instead of the constant 2.0; predictive en-route load also uses the real walk time
4. Only Optimized changes routing: `cost(r) = est_wait(r) + est_service(r) + walk_time(current_location, r)`; FixedOrder keeps package order, ShortestQueue still picks the shortest queue — neither reads distance/wing
5. Additive metrics: add `avg_walk_min` (mean total walk time per completed patient), keeping existing fields
6. Additive UI: RoomGrid shows a wing badge; the ReportView comparison table gains an Avg walk column; `types.ts` gains fields

**Design & key decisions:**
- Layout: entrance `(0,0)` placed next to the Imaging wing; Lab and Consultation are farther. Specifically: Blood Test `(14, 9)`, ECG `(15.5, 9)`; X-Ray `(2.5, 2.5)`, Ultrasound `(4, 2.5)`; Cardiology `(6.5, 16.5)`, Orthopedics `(8, 16)`, Endocrinology `(9.5, 16.5)`, ENT `(7, 18)`, Eye Exam `(9, 18)`.
  - Within the same wing: ~1.375–1.75 min (`blood_test → ecg` = 1.375); across wings: ~3.5–5.0 min (`blood_test → cardiology` ≈ 4.5).
- Optimized keeps the congestion-prediction cost but adds walk time and widens the tie band to **25%** (`COST_TIE_BAND`, previously 5%), with tie-break **nearest walk first**, then unblocks/critical-path. Rationale: when wait/service differences are small within the band, choosing a nearby room keeps the tour within a wing instead of jumping wings for a tiny congestion benefit.
  - Tried bands 5% → 20% → 30% → 25%. The 30% band made seed 123 lose to ShortestQueue on p90_total (136.79 vs 126.75); 25% keeps Optimized best on avg_total + p90 across all 3 seeds while also lowering walk.
- Baselines: routing logic stays absolutely unchanged (does not read position/wing). Walk physics applies to every strategy, so baseline avg_walk reflects package/queue order rather than distance optimization.

**Files changed:**
- Backend: `domain.rs`, `scenario.rs`, `engine/{router,congestion,sim,stats}.rs`, `engine/learn.rs` (test fixtures only)
- Frontend: `types.ts`, `components/RoomGrid.tsx`, `components/ReportView.tsx`, `App.tsx`
- Docs: `README.md` (describes cost/wing/tests), `docs/rounds/README.md`

**Results — comparison (headless, same arrivals/packages/service samples):**

Seed 42 (standard verification):

| Strategy | avg_total | avg_wait | p90_total | avg_walk |
|---|---|---|---|---|
| **Optimized** | **64.10** | **2.64** | **84.66** | **12.55** |
| FixedOrder | 77.02 | 5.91 | 130.21 | 13.45 |
| ShortestQueue | 71.32 | 4.33 | 103.89 | 13.57 |

Seed 7:

| Strategy | avg_total | avg_wait | p90_total | avg_walk |
|---|---|---|---|---|
| **Optimized** | **72.26** | **4.25** | **128.48** | **12.55** |
| FixedOrder | 84.39 | 7.08 | 138.89 | 13.82 |
| ShortestQueue | 77.82 | 5.56 | 139.75 | 13.10 |

Seed 123:

| Strategy | avg_total | avg_wait | p90_total | avg_walk |
|---|---|---|---|---|
| **Optimized** | **75.15** | **7.25** | **125.68** | **11.44** |
| FixedOrder | 86.44 | 10.17 | 145.23 | 12.73 |
| ShortestQueue | 81.09 | 8.67 | 126.75 | 12.49 |

**Assessment against verification criteria:**
- (a) Optimized is best on both avg_total and p90_total across all 3 seeds.
- (b) Optimized has the lowest walk on all 3 seeds: seed 42 −0.90/−1.02 min, seed 7 −1.27/−0.55, seed 123 −1.29/−1.05 versus FixedOrder/ShortestQueue → the router genuinely exploits distance.

Live run seed 99 (fresh data dir, speed clamp 60): 52/52 patients, sim_time 405.47 min, avg_total 80.45, avg_wait 7.16, p90_total 160.14, p90_wait 17.94, **avg_walk 11.49**, 188 visits.

**Verification steps performed:**
1. `cargo test`: **50/50 pass** (41 existing tests + 9 new: walk_time clamp/growth, wing distances, Optimized picks a nearby room at equal cost, cost computed from the current room, baselines ignore distance, EnRoute uses real walk time, patient location tracking, avg_walk metric).
2. `npm run build`: OK (tsc + vite; only the pre-existing chunk > 500 kB warning remains).
3. Fresh `MEDIROUTE_DATA_DIR=/tmp/mediroute-r3-test`, ran isolated **PORT=8082**; live sim `{strategy: optimized, speed: 5000, seed: 99}` → waited for finished.
4. `POST /api/sim/compare` seed 42, 7, 123; recorded full avg_total/avg_wait/p90_total/avg_walk for 3 strategies.
5. Checked `/api/rooms` returns `wing` + `position`, `/api/status` returns `avg_walk_min`, frontend served on 8082.
6. Did not touch port 8080 (the user's process pid 150786 untouched); no git commit/push.


**Independent verification (coordinator):** repeated the standard process on a separate server — cargo test 50/50, npm run build OK, fresh data dir → live seed 99 (finished, sim_time 405.5) → compare seeds 42/7/123: matches the report number-for-number (64.10/2.64/84.66/12.55; 72.26/4.25/128.48/12.55; 75.15/7.25/125.68/11.44). Optimized is best on avg_total + p90_total + avg_walk across all 3 seeds; the gap to baselines widened compared with the Round 1 scenario (seed 42: 7.2 min vs 5.0 min).

**Decision:** COMMIT — more realistic physical scenario (walk 1–5 min based on real distances between 3 zones) and the router exploits distance to widen the gap to both of the 2 baselines on every metric.

---

## Round 4 — 2-step lookahead routing (Optimized only)

**Status:** ✅ COMMITTED (independently verified)

**Proposed aspect:**
- Optimized only compares the cost of the next room. A cheap immediate choice can leave an expensive next step (locking a dependency, a far room, or a room still crowded when the patient returns).
- Add one-step lookahead: `cost_total(r) = walk(current, r) + est_wait(r) + est_service(r) + w × lookahead(r)`, where
  `lookahead(r) = min` over `r'` eligible **after** completing `r` of `[walk(r, r') + est_wait_at_arrival(r') + est_service(r')]`.
  If `r` is the last room (or no eligible rooms remain) → `lookahead = 0`.
- Only Optimized changes; FixedOrder/ShortestQueue absolutely unchanged; API/JSON unchanged.

**Design:**
- Step-2 eligibility is computed with `done ∪ {r}`, so a room blocked by a dependency only appears after choosing the right prerequisite (A unlocks B → B becomes a step-2 candidate of A, but is still blocked if X is chosen first).
- Arrival time at the next room: `finish_r + walk(r, r')`, where `finish_r = walk(current, r) + est_wait(r) + est_service(r)`.
- Future est_wait uses **decay**: `max(0, current_est_wait(r') − arrive_in)`. This reuses `congestion::est_wait_min` unchanged (no changes to congestion.rs) and reflects the queue draining while the patient walks/is served in the first room. Tried the no-decay version: clearly worse (see coefficient table).
- Tie-break unchanged: 25% band on `cost_total`, then nearest walk → unblocks → downstream depth → en-route → queue → package position.
- `sim.rs` needs no changes: `RouteContext` already has `required`, `done`, `graph`; the router adds `r` to the done set itself when estimating step 2.
- Coefficient `LOOKAHEAD_WEIGHT = 0.5` (`router.rs`). From w = 0.6, the 25% band on total cost swallows the one-step difference enough to flip the expectation of the existing test `clear_cost_difference_outranks_critical_path_depth`; w = 0.7 additionally breaks `router_uses_learned_service_time_not_config_seed`. w = 0.5 preserves all existing behavior while giving the best / most acceptable results on 3 seeds.

**Choosing the lookahead coefficient** (same process: fresh data dir → live seed 99 → compare 42/7/123; each w level has its own learned stats from its own live run):

| w (decay) | seed 42 avg/p90 | seed 7 avg/p90 | seed 123 avg/p90 |
|---|---|---|---|
| 0.40 | 64.78 / 92.74 | 72.35 / 127.76 | 75.91 / 125.00 |
| 0.45 | 64.61 / 83.69 | 73.27 / 126.89 | 75.91 / 125.00 |
| **0.50** | **63.92 / 75.89** | **72.80 / 126.89** | **75.92 / 121.81** |
| 0.60 | 64.90 / 92.35 | 72.34 / 128.32 | 79.47 / 136.79 |
| 0.70 | 64.06 / 83.69 | 71.27 / 128.50 | 76.41 / 131.20 |
| 0.80 | 65.51 / 85.55 | 71.37 / 128.50 | 75.80 / 131.20 |
| 0.50 no-decay | 66.95 / 88.14 | 72.78 / 126.89 | 79.57 / 129.42 |

- **Chose w = 0.5**: best or tied p90_total on all 3 seeds (75.89 / 126.89 / 121.81), with no seed's avg_total worse than ~1% versus Round 3. Levels 0.6–0.8 make seed 42/123 p90 worse than even Round 3 because discrete decisions jump to a different branch; 0.4/0.45 are less stable on seed 42. The no-decay version is worse on all 3 seeds. The curve is non-monotonic because each small change completely flips the routing order.

**Results — comparison (standard verification, headless — same arrivals/packages/service samples):**

Seed 42:

| Strategy | avg_total | avg_wait | p90_total | avg_walk |
|---|---|---|---|---|
| **Optimized** | **63.92** | **2.73** | **75.89** | **12.05** |
| FixedOrder | 77.02 | 5.91 | 130.21 | 13.45 |
| ShortestQueue | 71.32 | 4.33 | 103.89 | 13.57 |

Seed 7:

| Strategy | avg_total | avg_wait | p90_total | avg_walk |
|---|---|---|---|---|
| **Optimized** | **72.80** | **4.43** | **126.89** | **12.39** |
| FixedOrder | 84.39 | 7.08 | 138.89 | 13.82 |
| ShortestQueue | 77.82 | 5.56 | 139.75 | 13.10 |

Seed 123:

| Strategy | avg_total | avg_wait | p90_total | avg_walk |
|---|---|---|---|---|
| **Optimized** | **75.92** | **7.57** | **121.81** | **11.10** |
| FixedOrder | 86.44 | 10.17 | 145.23 | 12.73 |
| ShortestQueue | 81.09 | 8.67 | 126.75 | 12.49 |

Baselines match Round 3 **number-for-number** across all 3 seeds × 4 metrics (for example seed 42: 77.0249/5.9134/130.2068/13.4523 and 71.3185/4.3273/103.8948/13.5720) → physics, seed, and baseline routing untouched.

**Assessment vs Round 3 (Optimized):**

| Seed | Δ avg_total | Δ avg_wait | Δ p90_total | Δ avg_walk |
|---|---|---|---|---|
| 42 | −0.18 (63.92 vs 64.10) | +0.09 (2.73 vs 2.64) | **−8.77** (75.89 vs 84.66) | −0.50 (12.05 vs 12.55) |
| 7 | +0.54 (72.80 vs 72.26) | +0.18 (4.43 vs 4.25) | **−1.58** (126.89 vs 128.48) | −0.16 (12.39 vs 12.55) |
| 123 | +0.77 (75.92 vs 75.15) | +0.32 (7.57 vs 7.25) | **−3.87** (121.81 vs 125.68) | −0.33 (11.10 vs 11.44) |

- Criterion "better avg_total **or** p90_total without meaningful regression": met via **p90_total falling on all 3 seeds** (−1.6 to −8.8 min; −1.2% to −10.4%), avg_walk down on all 3, avg_total essentially flat (1 seed better, 2 seeds +0.5–0.8 min ≈ +0.75–1.0%), avg_wait +0.09–0.32 min (3–4%) — small relative to the tail benefit.
- Optimized still beats both of the 2 baselines on all 4 metrics across all 3 seeds.
- No clear over-bias: avg_walk keeps falling, meaning lookahead does not drag tours toward far wings.
- Note: this comparison follows the exact Round 3 process (compare uses learned stats from the live seed 99 run of the same code version), so the delta includes the feedback effect of the new stats; baselines are independent of stats and thus unaffected.

Live seed 99 (fresh dir, speed 5000→clamp 60): finished t = 408.15 min, 52/52, 188 visits; avg_total 80.99, avg_wait 7.29, p90_total 149.97, p90_wait 19.91, avg_walk 11.53. (Round 3: t = 405.47, 80.45/7.16/160.14/17.94/11.49 — live p90 also fell.)

**Files changed:** `backend/src/engine/router.rs` (constants + `future_wait`/`lookahead_cost`/`total_cost` + 4 new tests). Unchanged `congestion.rs`, `sim.rs`, `domain.rs`, `scenario.rs`, API, frontend. No commit.

**Verification steps:**
1. `cargo test`: **54/54 pass** (50 existing tests + 4 new: lookahead = 0 for the last room; future wait decays over step-1 time; dependency unlock + 2-step beats greedy 1-step in a constructed case; baselines keep their choices).
2. `npm run build`: OK (only the pre-existing chunk > 500 kB warning).
3. Fresh `MEDIROUTE_DATA_DIR=/tmp/mediroute-r4-test`, separate server `PORT=8082`; live `{strategy:optimized, speed:5000, seed:99}` → waited for finished.
4. `POST /api/sim/compare` seed 42 → 7 → 123; recorded full avg_total/avg_wait/p90_total/avg_walk for 3 strategies.
5. Checked `/api/rooms` returns `wing` + `position`, `/api/status` returns `avg_walk_min` + priority metrics, frontend served HTTP 200 on 8082.
6. Port 8080 untouched; no git commit/push.

**Decision:** Conditional WIN — clearly improves tail latency (p90_total) and walk on all 3 seeds, keeps baselines absolutely unchanged, no API/JSON change; avg_total/avg_wait slightly up on 2 seeds. Worth keeping for independent verification before deciding to commit.

**Independent verification (coordinator):** cargo test 54/54; repeated the standard process (fresh dir → live seed 99 finished t=408.1 → compare 42/7/123): matches the report number-for-number. Versus Round 3: p90_total −8.77/−1.58/−3.87 min on all 3 seeds; avg_total −0.18 (seed 42, better), +0.54/+0.77 (~1%, within noise); avg_walk down on all 3 seeds.

**Decision:** COMMIT — p90_total (the target metric) improves clearly on every seed, the standard seed 42 improves both avg_total and p90_total, and the remaining avg regressions stay within 1%.

---

## Round 5 — Queue re-routing (Optimized only)

**Status:** ❌ DISCARDED (independent verification: p90 unchanged, avg neutral — does not meet the "better" bar)

**Proposed aspect:**
- Optimized picks a room only at the start of a visit. Once in the queue, the patient never changes its mind: the room may go down, the queue may swell because others jumped in, while another eligible room sits empty.
- Let Optimized re-evaluate when the current queue "moves" or the current room has an incident, and switch to another room if the projected total completion time is clearly better.
- FixedOrder and ShortestQueue absolutely keep hard FIFO, no re-evaluation, no switching; the 3-seed results must match Round 4 number-for-number.

**Design:**
- **Trigger points** (Optimized only, only the affected room): `QueueJoin` (after enqueue + dispatch), `ServiceStart`, and `RoomDowntimeStart`. Queue progress (ServiceEnd → dispatch → ServiceStart) also passes through these triggers. `reevaluate_room_queue` walks positions > 0; if a patient leaves the queue, the index stays the same for the next patient.
- **Conditions:** only when `InQueue`, `queue_position > 0`, the correct current room, and the cap not reached. An `InService` patient is never switched; the head of the queue (position 0, about to be served) is not switched either.
- **Cost of staying:** `congestion::projected_completion_min` — the earliest station frees at `max(remaining service, downtime)`; only the `pending_starts + ahead_in_queue` jobs ahead consume the station; returns the patient's own wait + service. Later arrivals are ignored because they cannot jump the held position.
- **Full-tour comparison:** `stay_total = tour_completion(current_room, done ∪ {current_room}, stay_completion)`; for each eligible room `r'`, `target_total = walk(current_location, r') + future_wait(r') + est_service(r') + tour_completion(r', done ∪ {r'}, first_completion)`. `tour_completion` greedily builds the entire remaining tour (including the just-abandoned room), picking the eligible room with the earliest finish time while respecting dependencies; this fully prices the cost of returning to the old room. Tried a one-step target: seed 42 p90_total rose to 84.10 (no lookahead) and 91.86 (with lookahead); full-tour brings p90 back exactly to the Round 4 level.
- The full-tour version subtracts 1 from the `queue_len` of the current room when computing targets (the patient will leave that queue), avoiding double-counting the abandoned slot; otherwise downtime cases are mispriced and no switch happens.
- **Switch:** `target_total < stay_total − SWITCH_THRESHOLD_MIN`; pick the target with the smallest total. On a switch: `VecDeque::remove` from the old queue (preserving FIFO for the rest), increment `switch_count` + `switches_this_visit`, walk from `current_location` (the last completed room — unchanged by the switch) to the new room, then enqueue at the back (urgent is still inserted before normal as before). The walk already made toward the old room is sunk cost; the threshold absorbs this part.
- **Cap:** `MAX_SWITCHES_PER_VISIT = 2`, reset at `ServiceEnd`; a unit test constructs 3 consecutive downtimes to confirm the 3rd switch is blocked.

**Choosing the threshold** (fixed learned stats, same stats for all 4 levels):

| Threshold | seed 42 avg/p90 | seed 7 avg/p90 | seed 123 avg/p90 | switches 42/7/123 |
|---|---|---|---|---|
| 2.0 | 64.02 / 75.89 | 72.01 / 126.89 | 75.92 / 121.81 | 1 / 2 / 1 |
| **3.0** | **64.02 / 75.89** | **72.01 / 126.89** | **75.92 / 121.81** | **1 / 2 / 1** |
| 4.0 | 64.02 / 75.89 | 72.61 / 126.89 | 75.92 / 121.81 | 1 / 1 / 0 |
| 5.0 | 64.02 / 75.89 | 72.61 / 126.89 | 75.92 / 121.81 | 1 / 1 / 0 |

- **Chose 3.0 min** (within the suggested 3–5 range): levels 4–5 block a genuinely beneficial switch on seed 7 (avg_total 72.61 instead of 72.01); level 2 produces exactly the same decisions as level 3, so no need to go lower. Across the 3 seeds no patient switches more than 1 time, so the cap of 2 is only a safety net.

**Results — comparison (standard verification, fresh dir → live seed 99 → compare 42/7/123):**

Seed 42:

| Strategy | avg_total | avg_wait | p90_total | avg_walk | switches | patients switched | avg switches/patient |
|---|---|---|---|---|---|---|---|
| **Optimized** | **64.02** | **2.74** | **75.89** | **12.11** | 1 | 1 | 0.019 |
| FixedOrder | 77.02 | 5.91 | 130.21 | 13.45 | 0 | 0 | 0.000 |
| ShortestQueue | 71.32 | 4.33 | 103.89 | 13.57 | 0 | 0 | 0.000 |

Seed 7:

| Strategy | avg_total | avg_wait | p90_total | avg_walk | switches | patients switched | avg switches/patient |
|---|---|---|---|---|---|---|---|
| **Optimized** | **72.01** | **4.17** | **126.89** | **12.57** | 2 | 2 | 0.038 |
| FixedOrder | 84.39 | 7.08 | 138.89 | 13.82 | 0 | 0 | 0.000 |
| ShortestQueue | 77.82 | 5.56 | 139.75 | 13.10 | 0 | 0 | 0.000 |

Seed 123:

| Strategy | avg_total | avg_wait | p90_total | avg_walk | switches | patients switched | avg switches/patient |
|---|---|---|---|---|---|---|---|
| **Optimized** | **75.92** | **7.56** | **121.81** | **11.15** | 1 | 1 | 0.019 |
| FixedOrder | 86.44 | 10.17 | 145.23 | 12.73 | 0 | 0 | 0.000 |
| ShortestQueue | 81.09 | 8.67 | 126.75 | 12.49 | 0 | 0 | 0.000 |

- Baselines match Round 4 **number-for-number** across all 3 seeds × 4 metrics; `total_switches = 0` for both baselines.
- Optimized vs Round 4: seed 42 +0.10 avg_total (0.16%), +0.01 wait, p90 flat, +0.06 walk; seed 7 **−0.79 avg_total** (−1.1%), **−0.26 wait**, p90 flat, +0.18 walk; seed 123 0.00 avg_total, −0.01 wait, p90 flat, +0.05 walk. p90_total unchanged on all 3 seeds; avg_total clearly better on seed 7, the other seeds fluctuate ≤0.16%. Walk rises slightly due to detours, as expected from the mechanism.
- Switch stats: 4 switches / 156 patients (0.026 switches/patient; 4 patients switched, 1 time each). The mechanism is rare because it only affects waiting patients who are not at the head of the queue; that is why the overall benefit is small but does not harm the tail.

**Files changed:**
- `backend/src/engine/sim.rs`: re-evaluate triggers, `consider_switch`, `switch_patient`, cap + threshold, `switch_count`/`switches_this_visit`, trigger tests.
- `backend/src/engine/congestion.rs`: `projected_completion_min` + 2 tests.
- `backend/src/engine/router.rs`: `tour_completion`, `switch_decision`, extracted `lookahead_after`.
- `backend/src/domain.rs`: `Patient.switch_count`, `Patient.switches_this_visit`.
- `backend/src/engine/stats.rs`: `total_switches`, `patients_switched`, `avg_switches_per_patient`; `PatientView.switch_count` (additive JSON).
- `api.rs`, `engine/learn.rs`, `main.rs`, `scenario.rs`: reflowed by `cargo fmt` only, no logic changes.
- No changes to frontend, API routes, scenario, or baseline behavior.

**Verification steps:**
1. `cargo test`: **63/63 pass** (54 existing tests + 9 new: 2 congestion for projection, 7 sim for switch downtime/below-threshold/head-of-queue/baselines/FIFO/urgent/cap).
2. `npm run build`: OK (only the pre-existing chunk > 500 kB warning).
3. Fresh `MEDIROUTE_DATA_DIR=/tmp/mediroute-r5-test`, separate server `PORT=8082`; live `{strategy:optimized, speed:5000, seed:99}` → finished t = 408.15 min, 52/52, 188 visits, 0 switches; metrics 80.99 / 7.29 / 149.97 / 19.91 / 11.53 (identical to Round 4 because seed 99 offers no switch opportunity).
4. `POST /api/sim/compare` seed 42 → 7 → 123; recorded all 4 metrics + switch stats for 3 strategies; confirmed baselines match Round 4 number-for-number.
5. `/api/patients` returns `switch_count` for all 52 patients; `/api/status` returns the 3 switch metrics; frontend served HTTP 200 on 8082.
6. Port 8080 untouched; no git commit/push.

**Decision:** Conditional WIN — the mechanism works correctly (switches on downtime/large enough gain, preserves FIFO + priority, baselines absolutely unchanged), seed 7 lowers avg_total by 0.79 min and avg_wait by 0.26 min, p90_total unchanged on all 3 seeds. Seed 42 drifts +0.10 min avg_total (0.16%) because one switch looked beneficial but was not in practice; seed 123 neutral. This is a small acceptable trade-off; no commit per instructions.

**Independent verification (coordinator):** cargo test 63/63; repeated the standard process (fresh dir → live seed 99 finished → compare 42/7/123): matches the report number-for-number. Versus Round 4: p90_total did NOT change on all 3 seeds (75.89/126.89/121.81 — the hoped-for tail improvement did not happen); avg_total +0.10/−0.79/0.00; only 4 switches/156 patients.

**Decision:** DISCARD — the loop rule requires a clear "better"; here the target metric p90 is flat, avg is a small wash across seeds, and the mechanism barely activates (0.026 switches/patient) despite ~300 lines of code. Code reverted, cargo test 54/54. Record kept as documentation.

**Lessons learned:** in the current scenario, a patient stuck in a queue is rarely there because of a wrong routing decision but because of overall contention — re-routing only helps when a room is down for long or the comparison changes substantially; both are rare. If downtime increases (15%→30%) or rooms are added that often fail, this mechanism is worth retrying.

---

## Round 6 — Remaining-tour rollout evaluation (Optimized only)

**Status:** ❌ DISCARDED (negative result — no W better than Round 4; no commit)

**Proposed aspect:**
- Round 4 lookahead is only 1 step. Generalize to a greedy rollout of the whole remaining tour: for each candidate `r`, assume `r` is done (`position = r`, `done ∪ {r}`), then repeatedly pick the cheapest next room (`walk + est_wait decayed by arrival time + est_service`) until all required rooms are consumed.
- `rollout_cost(r)` = total cost of the remaining tour **after** `r` (not including the cost to reach `r`); `cost_total(r) = walk(current, r) + est_wait(r) + est_service(r) + W × rollout_cost(r)`, with W swept over 0.3/0.4/0.5/0.6/0.7.
- Rollout does not recompute other patients' capacity contention (only decays est_wait by arrival time), is deterministic, and costs at most O(rooms²) per decision (≤ 9 rooms → ~81 operations/decision; ~17k operations/sim).
- Only Optimized changes; FixedOrder/ShortestQueue, `congestion.rs`, `sim.rs`, API/JSON, frontend unchanged. The entire existing tie-break chain stays.

**Design:**
- `rollout_plan` (`router.rs`): build `tour_done = done ∪ {r}`, `location = r`, `elapsed = walk_to(r) + wait_at(r) + est_service(r)`. Each iteration filters required-and-not-done rooms eligible by dependency, picks the min `walk(location, next) + future_wait(next, elapsed + walk) + est_service(next)`, adds it to the total, advances `elapsed`, and updates `location`/`tour_done`. Returns `(total, path)` — path used by tests.
- `rollout_cost` = the total part; production `total_cost` uses `ROLLOUT_WEIGHT`. The old 1-step lookahead is kept under `#[cfg(test)]` (`lookahead_cost`/`one_step_total_cost`) as a reference fallback.
- The 25% tie band and the order nearest walk → unblocks → downstream depth → en-route → queue → package position stay unchanged, applied to the new `cost_total`.
- W sweep: each level gets a fresh data dir → live seed 99 → compare 42/7/123, so each level's learned stats are its own (following the Round 3/4 process).

**W sweep table (Optimized, headless compare; TB = 3-seed mean):**

| W | seed 42 avg/p90 | seed 7 avg/p90 | seed 123 avg/p90 | TB avg/p90 | walk TB |
|---|---|---|---|---|---|
| R4 1-step (0.5) | 63.92 / 75.89 | 72.80 / 126.89 | 75.92 / 121.81 | 70.88 / 108.20 | 11.85 |
| **0.30** | **64.28 / 82.08** | **75.25 / 127.75** | **80.55 / 121.04** | **73.36 / 110.29** | **11.24** |
| 0.40 | 64.88 / 86.18 | 74.31 / 124.92 | 80.78 / 127.95 | 73.32 / 113.02 | 10.94 |
| 0.50 | 66.43 / 92.86 | 74.65 / 124.92 | 81.58 / 127.95 | 74.22 / 115.24 | 10.69 |
| 0.60 | 66.96 / 86.18 | 74.80 / 124.92 | 81.58 / 127.95 | 74.45 / 113.02 | 10.59 |
| 0.70 | 68.82 / 95.94 | 74.66 / 136.82 | 83.04 / 127.95 | 75.51 / 120.24 | 10.49 |

- Chose **W = 0.3** for the final verification: best mean p90 in the sweep (110.29), avg almost tied with 0.4 (73.36 vs 73.32), beats 0.4 on 2/3 seeds on both avg and p90, and is the lowest level that still keeps all existing tests green.
- Clear trend: as W rises → walk falls steadily (11.24 → 10.49) but avg/p90 worsen. Rollout essentially optimizes tour geometry; the more it is trusted, the more future congestion is ignored.
- Test note: at W ≥ 0.5, the existing test `clear_cost_difference_outranks_critical_path_depth` flips expectation. Whole-tour rollout adds the shared tour part to both options, shrinking the relative gap into the 25% band (for example 47.5 vs 37.5 → 21%), so the critical-path tie-break picks `a`. W = 0.3/0.4 pass 59/59.

**Results — comparison for W=0.3 (standard verification: fresh dir → live seed 99 → compare 42/7/123):**

Seed 42:

| Strategy | avg_total | avg_wait | p90_total | avg_walk |
|---|---|---|---|---|
| **Optimized** | **64.28** | **2.94** | **82.08** | **11.62** |
| FixedOrder | 77.02 | 5.91 | 130.21 | 13.45 |
| ShortestQueue | 71.32 | 4.33 | 103.89 | 13.57 |

Seed 7:

| Strategy | avg_total | avg_wait | p90_total | avg_walk |
|---|---|---|---|---|
| **Optimized** | **75.25** | **5.25** | **127.75** | **11.71** |
| FixedOrder | 84.39 | 7.08 | 138.89 | 13.82 |
| ShortestQueue | 77.82 | 5.56 | 139.75 | 13.10 |

Seed 123:

| Strategy | avg_total | avg_wait | p90_total | avg_walk |
|---|---|---|---|---|
| **Optimized** | **80.55** | **9.13** | **121.04** | **10.39** |
| FixedOrder | 86.44 | 10.17 | 145.23 | 12.73 |
| ShortestQueue | 81.09 | 8.67 | 126.75 | 12.49 |

Baselines match Round 4/3 **number-for-number** across all 3 seeds × 4 metrics → physics, seed, and baseline routing untouched.

**Assessment vs Round 4 (Optimized):**

| Seed | Δ avg_total | Δ avg_wait | Δ p90_total | Δ avg_walk |
|---|---|---|---|---|
| 42 | +0.36 (+0.6%) | +0.21 | +6.19 (+8.2%) | −0.43 |
| 7 | +2.45 (+3.4%) | +0.82 | +0.86 (+0.7%) | −0.68 |
| 123 | +4.63 (+6.1%) | +1.56 | −0.77 (−0.6%) | −0.71 |

- Does not meet the criterion "better avg_total **or** p90_total without meaningful regression": avg_total is worse on all 3 seeds (seeds 7/123 exceed the 1–2% threshold), p90 is better by only 0.77 min on seed 123 but worse by +6.19 on seed 42 and +0.86 on seed 7.
- avg_walk falls steadily by 0.43–0.71 min on all 3 seeds: rollout genuinely pulls tours closer together, but trades wait/tail for it. The reason: rollout ignores future contention and decays est_wait by arrival time, so rooms that will be crowded "in the future" are priced near 0; the higher W, the more decisions are pulled toward short tours instead of avoiding queues.
- Live seed 99 (fresh dir, speed 5000→clamp 60), W=0.3: finished 409.81 min, 52/52, 188 visits; 82.33 / 7.99 / 156.66 / 23.02 / **10.38** (avg_total/avg_wait/p90_total/p90_wait/avg_walk) vs Round 4 80.99 / 7.29 / 149.97 / 19.91 / 11.53 → only walk is better.

**Files changed:** `backend/src/engine/router.rs` (ROLLOUT_WEIGHT + `rollout_plan`/`rollout_cost`/`tour_total_cost`; old lookahead kept under cfg(test); 5 new tests), `docs/rounds/README.md`. Unchanged `congestion.rs`, `sim.rs`, `domain.rs`, `scenario.rs`, API, frontend. No git commit/push; port 8080 untouched.

**Verification steps:**
1. `cargo test`: **59/59 pass** (54 existing tests + 5 new: rollout = 0 for the last room and lookahead equivalence; rollout does not pick a locked room; rollout prefers a nearby room in the remaining tour; equivalence when only 1 visit remains after the candidate; W=0 → immediate cost).
2. `npm run build`: OK (only the pre-existing chunk > 500 kB warning).
3. Fresh `MEDIROUTE_DATA_DIR=/tmp/mediroute-r6-test`, separate server `PORT=8082`; live `{strategy:optimized, speed:5000, seed:99}` → finished 409.81 min, 52/52, 188 visits.
4. `POST /api/sim/compare` seed 42 → 7 → 123; recorded all 4 metrics for 3 strategies; confirmed baselines match Round 4 number-for-number.
5. Repeated the W sweep with the same process for each level (separate fresh dir `/tmp/mediroute-r6-w0*`).

**Decision:** DISCARD (negative result) — no W beats Round 4 on avg_total or p90_total without meaningful regression; this is an honest negative result, not an implementation bug (rollout is deterministic, dependency-valid, baselines intact). W=0.3 is the best level of the sweep but still worse than R4 on avg for all 3 seeds and on p90 for 2/3 seeds. Code kept in the working tree at W=0.3 for the coordinator's independent verification; recommend reverting unless a different adjustment path is found (for example simulating contention inside the rollout, or using rollout only when queues are short).


**Independent verification (coordinator):** cargo test 59/59; repeated the standard process (fresh dir → live seed 99 finished → compare seed 42): matches exactly (64.28/2.94/82.08/11.62). Versus Round 4 seed 42: avg_total +0.36, p90_total +6.19 — clear regression. Code reverted, cargo test 54/54.

**Lessons learned:** whole-tour rollout optimizes geometry (walk falls steadily by 0.4–0.7 min) but est_wait decaying to ~0 in the far future makes it ignore future contention — pushing wait and the tail up. 1-step lookahead (R4) is the sweet spot: far enough to avoid dead ends, near enough for est_wait to still mean something. This direction should stop unless future contention can be modeled better.

---

## Round 7 — Priority-aware wait estimation (Optimized only)

**Status:** ❌ DISCARDED (negative result — avg_total and p90_total both worse than Round 4 on all 3 seeds; no commit)

**Proposed aspect:**
- `est_wait` currently treats every patient the same (near-FIFO), but:
  - (a) an urgent patient making a decision will jump ahead of all waiting normal patients → their true wait includes only in-service remaining + urgent patients ahead, not the whole queue. The router overestimates for urgent patients, so they may avoid a room that is actually good for them.
  - (b) normal patients will be jumped by urgent arrivals (near future) → the true wait is slightly higher than the current `est_wait`; anti-herding counts arrivals but not the queue-jumping effect.
- Split `est_wait` by the deciding patient's class, used for both the current cost and the 1-step lookahead. No changes to the priority queue in `sim.rs`, baselines, API/JSON, frontend, learning.

**Design:**
- `RoomLoad` gains (additive, default 0): `urgent_queue_len`, `urgent_en_route`, `urgent_arrival_rate_per_min`. `sim.rs` counts urgent in queue/en-route and records a timestamp on every urgent enqueue (`urgent_join_times`); the priority insertion mechanism is unchanged.
- `congestion::est_wait_urgent_min`: `projected_wait` with jobs = `pending_starts` (already dequeued before the newcomer can join, so they block every class) + `urgent_queue_len` + `urgent_en_route`. In-service remaining of every class still blocks because visits are not preempted; downtime is still added. Normal queue/en-route is ignored; likely arrivals are also ignored (kept simple, may underestimate when many urgent patients arrive together).
- `congestion::est_wait_normal_min`: take `est_wait_min` (FIFO + likely arrivals) as the base and add **expected urgent jumpers**. Each room's λ = number of urgent enqueues in the last 30 sim-minutes / 30 (`URGENT_RATE_WINDOW_MIN`; at run start λ is naturally low). `jumpers = ceil(λ × base_wait)`; add `jumpers` to the extra jobs of the projected wait. The result is always ≥ generic (test confirms).
- `router.rs`: `next_room` takes `PatientPriority`; `wait_at`/`future_wait`/`lookahead_cost`/`total_cost` use the correct class, including the next room in the 1-step lookahead. Baselines receive the parameter but ignore it (tests confirm identical choices).
- `RoomView` gains `est_wait_urgent_min`, `est_wait_normal_min` (additive JSON); no new metrics, frontend unchanged.
- Files changed: `backend/src/engine/congestion.rs`, `router.rs`, `sim.rs` (load instrumentation + RoomView); `docs/rounds/README.md`. Unchanged `stats.rs`, `api.rs`, `scenario.rs`, `domain.rs`, frontend.

**Tests:** `cargo test` **63/63 pass** (54 existing + 9 new: 6 congestion — urgent ignores normal queue/en-route, urgent counts only urgent competitors, normal adds jumpers, normal ≥ generic, idle/downtime; 2 router — urgent picks a room that normal avoids, baselines ignore priority; 1 sim — queue/en-route split + λ). `npm run build` OK (only the pre-existing chunk > 500 kB warning).

**Results — comparison (standard verification: fresh `/tmp/mediroute-r7-test` → live seed 99 speed 5000→clamp 60 → compare 42/7/123; 8 metrics):**

Seed 42:

| Strategy | avg_total | avg_wait | p90_total | avg_walk | avg_wait_urgent | p90_wait_urgent | avg_wait_normal | p90_wait_normal |
|---|---|---|---|---|---|---|---|---|
| **Optimized** | **65.94** | **3.19** | **94.43** | **12.38** | **1.02** | **4.16** | **3.52** | **11.53** |
| FixedOrder | 77.02 | 5.91 | 130.21 | 13.45 | 1.73 | 6.39 | 6.54 | 23.41 |
| ShortestQueue | 71.32 | 4.33 | 103.89 | 13.57 | 1.88 | 6.40 | 4.70 | 13.80 |

Seed 7:

| Strategy | avg_total | avg_wait | p90_total | avg_walk | avg_wait_urgent | p90_wait_urgent | avg_wait_normal | p90_wait_normal |
|---|---|---|---|---|---|---|---|---|
| **Optimized** | **73.44** | **4.60** | **128.65** | **12.39** | **1.59** | **4.27** | **4.90** | **17.27** |
| FixedOrder | 84.39 | 7.08 | 138.89 | 13.82 | 2.18 | 7.43 | 7.57 | 23.16 |
| ShortestQueue | 77.82 | 5.56 | 139.75 | 13.10 | 1.76 | 6.17 | 5.94 | 16.57 |

Seed 123:

| Strategy | avg_total | avg_wait | p90_total | avg_walk | avg_wait_urgent | p90_wait_urgent | avg_wait_normal | p90_wait_normal |
|---|---|---|---|---|---|---|---|---|
| **Optimized** | **79.27** | **8.47** | **140.25** | **11.36** | **3.00** | **7.79** | **9.05** | **24.48** |
| FixedOrder | 86.44 | 10.17 | 145.23 | 12.73 | 1.94 | 6.85 | 11.04 | 23.38 |
| ShortestQueue | 81.09 | 8.67 | 126.75 | 12.49 | 3.76 | 10.69 | 9.19 | 24.95 |

Baselines match Round 4 **number-for-number** on all 3 seeds × 4 metrics (77.02/5.91/130.21/13.45; 71.32/4.33/103.89/13.57; 84.39/7.08/138.89/13.82; 77.82/5.56/139.75/13.10; 86.44/10.17/145.23/12.73; 81.09/8.67/126.75/12.49) and also the urgent/normal split.

**Measuring the Round 4 split from HEAD** (same process, clean worktree at the R4 commit):

| Seed | avg_wait_urgent | p90_wait_urgent | avg_wait_normal | p90_wait_normal |
|---|---|---|---|---|
| 42 | 1.38 | 6.55 | 2.93 | 10.07 |
| 7 | 1.17 | 3.90 | 4.75 | 15.82 |
| 123 | 2.52 | 8.17 | 8.10 | 24.62 |

**Assessment vs Round 4 (Optimized):**

| Seed | Δ avg_total | Δ avg_wait | Δ p90_total | Δ avg_walk | Δ urg avg/p90 | Δ norm avg/p90 |
|---|---|---|---|---|---|---|
| 42 | +2.02 (+3.2%) | +0.46 | **+18.54 (+24.4%)** | +0.33 | −0.36 / **−2.39** | +0.59 / +1.46 |
| 7 | +0.64 (+0.9%) | +0.17 | +1.76 (+1.4%) | 0.00 | +0.42 / +0.37 | +0.15 / +1.45 |
| 123 | +3.35 (+4.4%) | +0.90 | **+18.44 (+15.1%)** | +0.26 | +0.48 / −0.38 | +0.95 / −0.14 |

- Does not meet the criterion "better avg_total **or** p90_total without meaningful regression": all 3 seeds are worse on avg_total; p90_total is worse on all 3 (2 seeds worse by ~15–24%). Only urgent p90 on seed 42 improves clearly (−2.39) but at the expense of normal and the total.
- Live seed 99: finished 408.15 min, 52/52, 188 visits; **81.03 / 7.35 / 143.03 / 19.91 / 11.37**, urgent 4.18/8.22, normal 7.59/20.78 (R4: 80.99/7.29/149.97/19.91/11.53; re-measured split: urgent 4.20/9.90, normal 7.52/20.78). Live p90_total fell 6.94 but the 3-seed compare is worse — seed 99 is a favorable exception, not a trend.

**Diagnosis (variants, each level fresh dir + live seed 99 + compare 42/7/123):**

| Variant | seed 42 avg/p90 | seed 7 avg/p90 | seed 123 avg/p90 |
|---|---|---|---|
| R4 (reference) | 63.92 / 75.89 | 72.80 / 126.89 | 75.92 / 121.81 |
| urgent-only (normal uses generic) | 63.79 / 83.69 | 73.06 / 128.20 | 75.92 / 121.81 |
| normal-only (urgent uses generic) | 66.32 / 94.43 | 73.18 / 128.40 | 79.27 / 140.25 |
| full + jumpers floor | 64.02 / 92.73 | 73.06 / 128.20 | 75.92 / 121.81 |
| full + jumpers ceil (final) | 65.94 / 94.43 | 73.44 / 128.65 | 79.27 / 140.25 |

- Running the R7 router with **R4's learned stats** gives exactly the same results as R7 stats (65.94/3.19/94.43/12.38; 73.44/4.60/128.65/12.39; 79.27/8.47/140.25/11.36) → the regression is purely from routing, not learning feedback.
- Urgent-only (dropping the jumpers term) nearly ties R4 on avg but p90 is still worse on 2 seeds; normal-only alone reproduces most of the bad p90 level (94.43 / 140.25) → **the normal jumpers term is the largest regression source**, and class-aware urgent by itself does not improve the total either.
- Causes: (a) when urgent patients ignore the normal queue, urgent patients rush into rooms with many normal patients — urgent is indeed served faster, but normal is slowed down; the old estimate (counting the whole normal queue) accidentally acted as load balancing and was better for the total. The premise "urgent avoids rooms that are actually good for them" is false in this scenario (urgent-only makes urgent p90 seed 42 rise from 1.38 to 1.71). (b) The jumpers term (ceil) penalizes every room with urgent history by an extra 1–2 full services (~10–30 min), enough to flip many normal decisions to another tour; seed 42 is typical: 5 General Checkup patients finish their tour at ENT with a 20–30 min wait (R4: 7–11) → p90_total 94.43. Floor makes normal p90 closer to R4 but still cannot save the tail.
- No clear over-bias on walk: seed 42/7 walk rises slightly (+0.33/0.00), seed 123 +0.26.

**Verification steps:**
1. `cargo test`: 63/63 pass; `npm run build`: OK.
2. Fresh `MEDIROUTE_DATA_DIR=/tmp/mediroute-r7-test`, separate server `PORT=8082`; live `{strategy:optimized, speed:5000, seed:99}` → finished t = 408.15 min, 52/52, 188 visits.
3. `POST /api/sim/compare` seed 42 → 7 → 123; recorded all 8 metrics for 3 strategies; baselines match R4 number-for-number; `/api/rooms` also returns `est_wait_urgent_min`/`est_wait_normal_min`.
4. Variants: repeated the process with separate fresh dirs for urgent-only, normal-only, full-floor; measured the R4 split using a clean worktree at HEAD.
5. Port 8080 untouched; no git commit/push.

**Decision:** DISCARD (negative result) — priority-aware wait estimation does not improve the whole: avg_total is worse on all 3 seeds, p90_total is worse by 1.4–24.4%, only urgent p90 on seed 42 is better. Code kept in the working tree (uncommitted) for the coordinator's independent verification; recommend reverting unless a different adjustment path is found (for example discounting part of the normal queue instead of dropping it entirely, or applying the jumpers term only at genuinely high contention).

**Lessons learned:** in this scenario priority is a queue mechanism, not a routing signal. Ignoring the normal queue makes urgent patients herd into crowded rooms and shifts the cost to normal patients; the "wrong" old estimate was actually a form of load regulation that helped. Exploiting priority in routing would require modeling the **externality cost imposed on normal patients** (not just urgents' wait), rather than only changing each class's wait estimate.

**Independent verification (coordinator):** cargo test 63/63; repeated the standard process (fresh dir → live seed 99 finished → compare seed 42): matches exactly (65.94/94.43 — regression vs R4 63.92/75.89). Code reverted, cargo test 54/54.

**Decision:** DISCARD — p90_total regression +18.54 (seed 42) / +18.44 (seed 123). The agent's diagnosis confirms: urgent-only estimate nearly ties, but normal-only reproduces most of the regression; dropping the normal queue from the urgent estimate makes urgent herd into crowded rooms, shifting cost to normal patients (5 General Checkup patients on seed 42 finish their tour at ENT with a 20–30 min wait).

**Lessons learned:** splitting wait estimates by class is only safe when the urgent volume is small enough for second-order effects; here ~10% urgent is enough for the normal-estimate distortion (jumpers) to bend the tours of 90% of patients. If retried: change only est_wait_urgent (keep normal = generic), drop the jumpers term.

---

## Round 8 — Time-aware future congestion model (Optimized only)

**Status:** ✅ COMMITTED (independently verified on 5 seeds — mean better than Round 4 on both avg_total and p90_total)

**Proposed aspect:**
- Diagnosis from Round 6: linear-decay future-wait `max(0, est_wait − arrive_in)` is blind to future contention. During rush, wait does not fall and may rise because new arrivals exceed capacity drain; during the lull/end-of-day it falls quickly.
- Replace decay with a static demand model: `future_wait(room, h)` rebuilds the current station backlog, then adds expected arrivals from the scenario's arrival schedule over `(now, now + h]`, draining them with capacity (including downtime), and returns the wait of a patient arriving `h` minutes later.
- Used only for Optimized's step-2 lookahead (keeping Round 4's 1-step structure, no whole-tour rollout). Current `wait_at`, baselines, priority queue, API/JSON, and frontend unchanged.

**Design:**

*Demand model (`scenario.rs`, offline, no dynamic learning):*
- `ArrivalDemand::from_scenario(scenario, N)` with `N = patient_count`:
  - `λ_hosp(t) = 0.65·N / 120` for `t ∈ [0, 120)`; `0` for `[120, 150)`; `0.35·N / 60` for `[150, 210)`; `0` after `210` (matching the rush profile: 65% in 0–120, 35% in 150–210, lull 120–150).
  - Room popularity: `share(r) = Σ_{package p containing r} weight(p) / Σ_p weight(p)`.
  - `λ_r(t) = share(r) · λ_hosp(t)`; expected arrivals over `[a, b]` integrate per segment: `E_r(a,b) = Σ_segments overlap(a,b,segment) · rate_segment · share(r)`. Total hospital-wide arrivals over the 2 windows = `N` (conservation test).
- Demo examples: `share(blood_test) = 0.80`, `share(ent) = 0.45`, `share(ultrasound) = 0.30`.

*Future wait (`congestion.rs`):*
- For a room with capacity `C`, service estimate `s`, downtime `d`:
  - Initialize `C` station free-times: `q_i = max(in_service_remaining_i, d)`; unused station = `d`.
  - Add the committed jobs (`pending_starts + queue_len + en_route`) with ready = 0 via list scheduling: always pick the earliest station, `q = max(q, ready) + s`.
  - Expected arrivals in `(now, now+h]` are split into 1-minute slices: the slice at offset `k` has `e_k` arrivals → add `e_k` jobs (fractional) with `ready = k`, work `= e_k · s`. Anti-herding `likely_arrivals` (competitors free at `ready_in`) are also added at exactly `ready_in ≤ h` with their weight — no double-count because they are not in the current queue.
  - `future_wait(room, h) = max(0, min_i q_i − h)`.
- When `λ = 0` (lull 120–150, after 210, or tests without demand) the model degrades correctly to `max(0, current_wait − h)` — compatible with Round 4 behavior.
- During rush, if `λ_r · s > C` the future backlog grows linearly; for example Orthopedics: `share = 0.30`, `λ_peak = 0.65·52/120 ≈ 0.282/min` → `λ_r ≈ 0.085/min`, `× 16 min service = 1.35 > C = 1`, so an empty room still accumulates queue as the horizon grows.
- `future_wait` only replaces the lookahead's `future_wait`; `wait_at` (next-room cost) keeps `congestion::est_wait_min`.

*Router (`router.rs`):*
- `RouteContext` gains `demand: Option<&ArrivalDemand>` and `now_min: f64` (additive, existing tests use `None`/`0.0`).
- `lookahead(r) = min_{r' eligible after r} [walk(r,r') + future_wait(r', arrive_in) + est_service(r')]`, `arrive_in = walk(current,r) + wait_at(r) + est_service(r) + walk(r,r')` — exactly when the patient actually reaches `r'`.
- `LOOKAHEAD_WEIGHT` stays `0.5`; the 25% tie-band and tie-break unchanged.

**Choosing the lookahead coefficient** (each level fresh data dir + live seed 99 + compare 42/7/123, its own learned stats; the table shows Optimized avg_total / p90_total):

| w | seed 42 | seed 7 | seed 123 |
|---|---|---|---|
| 0.40 | 64.83 / 82.62 | 72.80 / 126.47 | 76.78 / 130.16 |
| 0.45 | 66.19 / 87.86 | 73.13 / 128.20 | 76.91 / 129.34 |
| **0.50** | **64.07 / 77.79** | **73.13 / 128.20** | **72.29 / 116.37** |
| 0.55 | 65.26 / 80.53 | 73.70 / 128.20 | 80.72 / 122.27 |
| 0.60 | 64.38 / 83.14 | 73.31 / 126.89 | 80.13 / 134.62 |
| 0.50 without likely arrivals | 62.86 / 75.66 | 72.70 / 128.30 | 78.56 / 142.34 |
| 0.50 max(old decay, model) | 64.07 / 77.79 | 73.13 / 128.20 | 72.29 / 116.37 |

- **Chose w = 0.5**: best on seed 42/123 for both avg_total and p90_total; seed 7 is only ~0.5% avg / ~1.4% p90 behind the best level. w = 0.55/0.60 clearly worsen seed 123 (avg 80.7/80.1); w = 0.40/0.45 are good on seed 7 but worse on seed 42/123 p90.
- Dropping likely arrivals (only demand λ) improves seed 42/7 but seed 123 p90 jumps to 142.34 (+16.9% vs Round 4) → keep likely arrivals at the correct ready time.
- Wrapping with `max(old decay, model)` gives **bit-identical** results to the pure model on all 3 seeds: in this scenario the model is already ≥ decay at the horizons used; the seed 42 regression is not because it prices higher than decay but because rush pressure changes the step-2 ranking. Keep the pure model for simplicity.

**Results — comparison (standard verification, headless):**

Seed 42:

| Strategy | avg_total | avg_wait | p90_total | avg_walk |
|---|---|---|---|---|
| **Optimized** | **64.07** | **2.91** | **77.79** | **11.51** |
| FixedOrder | 77.02 | 5.91 | 130.21 | 13.45 |
| ShortestQueue | 71.32 | 4.33 | 103.89 | 13.57 |

Seed 7:

| Strategy | avg_total | avg_wait | p90_total | avg_walk |
|---|---|---|---|---|
| **Optimized** | **73.13** | **4.68** | **128.20** | **11.76** |
| FixedOrder | 84.39 | 7.08 | 138.89 | 13.82 |
| ShortestQueue | 77.82 | 5.56 | 139.75 | 13.10 |

Seed 123:

| Strategy | avg_total | avg_wait | p90_total | avg_walk |
|---|---|---|---|---|
| **Optimized** | **72.29** | **6.60** | **116.37** | **10.81** |
| FixedOrder | 86.44 | 10.17 | 145.23 | 12.73 |
| ShortestQueue | 81.09 | 8.67 | 126.75 | 12.49 |

Baselines match Round 4 **number-for-number** on all 3 seeds × 4 metrics (77.0249/5.9134/130.2068/13.4523; 71.3185/4.3273/103.8948/13.5720; 84.3898/7.0846/138.8932/13.8233; 77.8194/5.5577/139.7541/13.0962; 86.4409/10.1679/145.2265/12.7288; 81.0868/8.6726/126.7505/12.4931) → physics, seed, and baseline routing untouched.

**Assessment vs Round 4 (Optimized):**

| Seed | Δ avg_total | Δ avg_wait | Δ p90_total | Δ avg_walk |
|---|---|---|---|---|
| 42 | +0.15 (+0.23%) | +0.18 (+6.78%) | +1.90 (+2.50%) | −0.54 (−4.47%) |
| 7 | +0.33 (+0.46%) | +0.25 (+5.67%) | +1.31 (+1.03%) | −0.63 (−5.05%) |
| 123 | −3.63 (−4.78%) | −0.97 (−12.85%) | −5.44 (−4.47%) | −0.29 (−2.65%) |
| **Mean 3 seed** | **−1.05 (−1.48%)** | **−0.18 (−3.65%)** | **−0.75 (−0.69%)** | **−0.49 (−4.11%)** |

- By 3-seed mean, all 4 metrics are better than Round 4; the benefit concentrates on seed 123 (avg_total −4.78%, p90_total −4.47%, avg_wait −12.85%), where rush contention is strongest.
- Per seed: seed 123 clearly meets the criterion; seed 7 p90 +1.03% is within the 1–2% threshold and avg +0.46% counts as flat; seed 42 p90 +2.50% **slightly exceeds the 2% threshold** and avg +0.23% (within noise). So this is not claimed as an absolute win — it is a conditional aggregate win.
- Optimized still beats both of the 2 baselines on all 4 metrics across all 3 seeds.
- avg_wait rises 5.7–6.8% on seeds 42/7 but avg_total is nearly unchanged and walk falls 4.5–5.1%: routing moves to shorter, less tail-heavy tours; the tail is only slightly worse on seed 42.
- No sign of over-bias: avg_walk falls on all 3 seeds.

Live seed 99 (fresh dir, speed 5000→clamp 60): finished t = 399.75 min, 52/52, 188 visits; **78.01 / 6.61 / 146.60 / 16.47 / 11.05** (avg_total/avg_wait/p90_total/p90_wait/avg_walk). Round 4: 80.99 / 7.29 / 149.97 / 19.91 / 11.53 → live is also better on all 5 metrics (avg_total −2.98, p90_total −3.37, p90_wait −3.44, walk −0.48).

**Files changed:** `backend/src/scenario.rs` (`ArrivalSegment`/`ArrivalDemand` + 2 tests), `backend/src/engine/congestion.rs` (`future_wait_min` + `push_job` + 6 tests), `backend/src/engine/router.rs` (`RouteContext.demand/now_min`, `future_wait` uses the model, 2 new tests; 17 literal tests gained fields, no behavior change), `backend/src/engine/sim.rs` (World builds `ArrivalDemand` and passes it into RouteContext), `docs/rounds/README.md`. Unchanged current `wait_at`/predictive wait, FixedOrder/ShortestQueue, priority queue, `domain.rs`, `stats.rs`, `learn.rs`, API/JSON, frontend.

**Verification steps:**
1. `cargo test`: **64/64 pass** (54 existing tests + 10 new: 2 scenario — conservation of N + lull, room share by package; 6 congestion — no-demand matches old decay, increases during rush, returns to 0 at lull/after 210, reflects room share, likely arrivals at the correct ready time, downtime; 2 router — lookahead uses the correct `arrive_in` + future_wait grows with horizon, baselines keep their choices).
2. `npm run build`: OK (only the pre-existing chunk > 500 kB warning).
3. Fresh `MEDIROUTE_DATA_DIR=/tmp/mediroute-r8-test`, separate server `PORT=8082`; live `{strategy:optimized, speed:5000, seed:99}` → finished 399.75 min, 52/52, 188 visits.
4. `POST /api/sim/compare` seed 42 → 7 → 123; recorded all 4 metrics for 3 strategies; confirmed baselines match Round 4 number-for-number.
5. Repeated the w sweep with separate fresh dirs `/tmp/mediroute-r8-w0*`; variants without likely arrivals and `max(decay, model)`.
6. Port 8080 untouched; no git commit/push.

**Decision:** CONDITIONAL AGGREGATE WIN — no commit (per round instructions). The 3-seed mean improves avg_total, avg_wait, p90_total, and avg_walk; baselines absolutely unchanged; API/JSON unchanged. It falls slightly short under a strict per-seed criterion: seed 42 p90_total +2.50% (>2%), while seed 7 p90 +1.03% and seed 123 improves strongly. Code kept in the working tree (uncommitted) for the coordinator's independent verification.

**Lessons learned:** confirms the Round 6 diagnosis — linear decay ignoring arrival pressure is systematically wrong during rush; the λ + station-drain model makes future-wait rise/fall with the correct phase and delivers a large tail benefit on the high-contention seed (seed 123) without changing the current predictive wait. The benefit is uneven across seeds and fairly sensitive to competitor handling (likely arrivals): dropping them helps seed 42 but destroys the seed 123 tail. Next direction if per-seed stability is needed: threshold the model on capacity overshoot (`λ_r · s` vs `C`) instead of always adding expected arrivals, or blend with weights against the old decay — but `max(decay, model)` was tried and is bit-identical, so a different mechanism is needed, not just clamping.

**Independent verification (coordinator) — extended to 5 seeds:** cargo test 64/64; repeated the standard process on both R8 (working tree) and R4 (worktree HEAD): fresh dir → live seed 99 → compare seeds 42/7/123/5/2024. Results: R8 loses slightly on uncongested seeds (42: +0.15 avg/+1.90 p90; 7: +0.33/+1.31; 2024: +0.83 avg but p90 −1.76) but wins big on congested seeds (123: −3.63/−5.44; **5: −0.72/−37.00**). Mean 5 seeds: avg_total 79.95→79.34 (−0.61), p90_total 143.39→135.19 (−8.20). Excluding seed 5, the 4-seed mean is still better on both metrics.

**Decision:** COMMIT — the mean is better on both avg_total and p90_total across 5 seeds; the biggest improvement lands exactly in the target region (high contention), and regressions only occur on uncongested seeds below 2.5% p90. The future-congestion model fixes the blind spot diagnosed in Round 6 (wait decaying to 0).

---

## Round 9 — Adaptive EWMA learning rate (learning only, no routing changes)

**Status:** ❌ DISCARDED (negative end-to-end — no commit)

**Proposed aspect:**
- Current learning (since Round 1) uses a **fixed α = 0.3** EWMA for every room, at all times. Two consequences:
  - **Slow cold start**: `est_service` initializes from the seed config, deliberately offset from the true mean (demo: config 10 min, true mean 13 min). With α = 0.3, after 5 samples the estimate only reaches ~12.50 (still 0.50 off); routing in the first ~10–15 min relies on a wrong estimate.
  - **Noisy steady state**: service samples fluctuate ±20% around the true mean; α = 0.3 keeps the estimate fluctuating sample-by-sample, making the `est_service` cost term bumpy and causing route churn.
- Propose an adaptive α by each room's `visit_count`: **high when samples are few** (fast convergence from cold start), **decreasing as samples accumulate** (lower steady-state variance):
  `α(n) = max(α_min, α_init / (1 + n/k))`.
- Only `learn.rs` changes (+ diagnostic field); **unchanged** router logic, congestion, sim physics, baselines, priority queue, API/JSON (only additive fields in the room view), and frontend.

**Design:**

*`learn.rs`:*
- `EwmaConfig { alpha_init, alpha_min, decay_k }`, default **0.7 / 0.1 / 10**; `alpha(n) = clamp(alpha_init/(1+n/k), alpha_min, alpha_init)`.
  - With the default: `α(0)=0.70`, `α(1)=0.636`, `α(5)=0.467`, `α(10)=0.35`, `α(20)=0.233`, `α(36)=0.152`, `α(60)=0.10` (hits the floor), staying at `0.10` for every `n ≥ 60`.
  - α is applied simultaneously to `est_service_min`, `est_wait_observed_avg`, and the new diagnostic.
- `Learner` holds `EwmaConfig`; `new()` uses the default, plus `with_ewma()` (for sweeps/tests) and `current_alpha(n)` (for the room view while idle).
- Diagnostic **`avg_abs_service_dev_min`** = EWMA of `|service_sample − est before update|`, using the same α; observation-only for uncertainty, **not** part of routing/cost.
- **Compatible persistence**: new fields in `RoomStats` carry `#[serde(default)]`; old `learning.json` missing the fields loads normally with value 0, existing fields (`est_service_min`, `est_wait_observed_avg`, `visit_count`) stay intact, and the next record updates the new fields. No overwriting/deleting old data.

*`sim.rs` (additive view only):* `RoomView` adds `learned_alpha` (current α by the room's `visit_count`) and `avg_abs_service_dev_min`. No simulation logic changes.

*Control:* `EwmaConfig { alpha_init: 0.3, alpha_min: 0.3, ... }` reproduces Round 8 **bit-identical** in the live run and all 3 seed compares → confirms routing/sim/baselines untouched.

**Parameter sweep** (each combo fresh data dir, live seed 99 → compare 42/7/123; numbers are Optimized; 36 combos α_init ∈ {0.4,0.5,0.6,0.7} × α_min ∈ {0.1,0.15,0.2} × k ∈ {5,10,20}, plus out-of-range diagnostics):

| Config (α_init/α_min/k) | Live avg | Live p90 | Cmp mean avg | Cmp mean p90 | s42 avg | s7 avg | s123 avg |
|---|---|---|---|---|---|---|---|
| **R8 control α=0.3** | **78.01** | **146.60** | **69.83** | **107.45** | 64.07 | 73.13 | 72.29 |
| 0.4 / 0.1 / 10 | 79.63 | 139.71 | 72.95 | 116.73 | 64.02 | 73.13 | 81.69 |
| 0.5 / 0.1 / 5 | 79.63 | 139.71 | 72.13 | 112.98 | 64.02 | 73.13 | 79.23 |
| 0.6 / 0.1 / 10 | 82.44 | 155.96 | 72.50 | 109.42 | 64.02 | 73.13 | 80.36 |
| **0.7 / 0.1 / 10 (selected)** | 79.96 | 150.32 | 73.03 | 118.60 | 64.29 | 73.13 | 81.69 |
| 0.7 / 0.1 / 20 | 81.07 | 149.27 | 72.52 | 109.42 | 64.07 | 73.13 | 80.36 |
| 0.7 / 0.2 / 10 | 79.96 | 150.32 | 72.96 | 116.73 | 64.07 | 73.13 | 81.69 |
| diagnostic 0.7 / 0.3 / 5 | 79.96 | 150.32 | 69.83 | 107.45 | 64.07 | 73.13 | 72.29 |

- **No combo in range improves the compare mean**: best avg_total is 72.13 (0.5/0.1/5), best p90_total is 109.42 (0.6/0.1/10 and 0.7/0.1/20) — all worse than R8 (69.83 / 107.45). The regression concentrates on **seed 123** (79.2–81.7 vs 72.29); seeds 42/7 are nearly unchanged.
- The `α_min = 0.3, k = 5` diagnostic (outside the required range) **matches R8 exactly in the compare** because after the live run each room already has 14–36 visits → `α(n) = 0.3`, exactly the fixed alpha throughout the compare. This is direct evidence of the mechanism: adaptive benefit only appears when learning is truly cold; once warm, α below 0.3 makes the estimate track the new seed's sample stream more slowly.
- Chose **0.7 / 0.1 / 10** as the representative config: in range, highest α_init (fastest cold start), best cold-compare among in-range combos (see below), and the clearest illustration of the cold-vs-warm trade-off.

**Cold-start quality (synthetic + live):**

| Check | Adaptive 0.7/0.1/10 | Fixed 0.3 | Observation |
|---|---|---|---|
| 5 samples, seed 10 → 13 (estimate) | 12.9685 (off by 0.0315) | 12.4958 (off by 0.5040) | adaptive off by **16× less** |
| Steady-state variance (500 estimates, n≥300, sample ±20%) | 0.1006 | 0.3483 | adaptive **~3.5× lower** |
| Live seed 99: time to MAE < 1.0 | t = 54.0 | t = 78.0 | 24 min earlier |
| Live seed 99: time to MAE < 0.75 | t = 78.0 | t = 90.0 | 12 min earlier |
| Live seed 99: end-of-run MAE | 0.459 | 0.427 | 0.03 higher (see analysis) |

Adaptive's end-of-run MAE is not lower because the live run has only 16–36 visits/room: the final estimate point is dominated by finite-sample noise of the seed rather than variance; the variance advantage only shows on long series (synthetic test with 800 samples).

**Results — live run seed 99 (fresh dir, speed 5000, true cold start):**

| Metric | R8 (α=0.3) | Round 9 (0.7/0.1/10) | Δ |
|---|---|---|---|
| avg_total | 78.01 | 79.96 | +1.95 (+2.50%) |
| avg_wait | 6.61 | 7.13 | +0.52 (+7.87%) |
| p90_total | 146.60 | 150.32 | +3.72 (+2.54%) |
| p90_wait | 16.47 | 23.02 | +6.55 (+39.8%) |
| avg_walk | 11.05 | 11.10 | +0.05 (+0.45%) |
| finished (sim time) | 399.75 | 392.21 | −7.54 min |
| final MAE | 0.427 | 0.459 | +0.032 |

Live: 52/52, 188 visits, urgent 5.00/10.25, normal 7.29/23.26 (avg/p90 wait). Adaptive converges the estimate faster early on but **live metrics are worse**, mainly p90_wait: the high α early on follows each ±20% service sample (noise, not signal), changing room rankings early and pushing the normal group's tail up (normal p90_wait 23.26 vs 14.59 for R8), even though the urgent group is better (5.00 vs 5.42).

**3-seed compare — exact verification process (live seed 99 first, then compare):**

Seed 42:

| Strategy | avg_total | avg_wait | p90_total | avg_walk |
|---|---|---|---|---|
| **Optimized (R9)** | 64.29 | 3.03 | 83.40 | 11.32 |
| Optimized (R8) | 64.07 | 2.91 | 77.79 | 11.51 |
| FixedOrder | 77.02 | 5.91 | 130.21 | 13.45 |
| ShortestQueue | 71.32 | 4.33 | 103.89 | 13.57 |

Seed 7:

| Strategy | avg_total | avg_wait | p90_total | avg_walk |
|---|---|---|---|---|
| **Optimized (R9)** | 73.13 | 4.68 | 128.20 | 11.76 |
| Optimized (R8) | 73.13 | 4.68 | 128.20 | 11.76 |
| FixedOrder | 84.39 | 7.08 | 138.89 | 13.82 |
| ShortestQueue | 77.82 | 5.56 | 139.75 | 13.10 |

Seed 123:

| Strategy | avg_total | avg_wait | p90_total | avg_walk |
|---|---|---|---|---|
| **Optimized (R9)** | 81.69 | 9.45 | 144.21 | 10.45 |
| Optimized (R8) | 72.29 | 6.60 | 116.37 | 10.81 |
| FixedOrder | 86.44 | 10.17 | 145.23 | 12.73 |
| ShortestQueue | 81.09 | 8.67 | 126.75 | 12.49 |

Mean 3 seed Optimized: **73.04 / 5.72 / 118.60 / 11.18** vs R8 **69.83 / 4.73 / 107.45 / 11.36** → avg_total **+3.21 (+4.59%)**, avg_wait +0.99 (+20.9%), p90_total **+11.15 (+10.38%)**, avg_walk −0.18 (−1.62%). Baselines match R8 **number-for-number** on all 3 seeds × 4 metrics (77.0249/5.9134/130.2068/13.4523; 71.3185/4.3273/103.8948/13.5720; 84.3898/7.0846/138.8932/13.8233; 77.8194/5.5577/139.7541/13.0962; 86.4409/10.1679/145.2265/12.7288; 81.0868/8.6726/126.7505/12.4931) → physics/seed/baseline routing unchanged.

**Cold compare (fresh dir, no live warm-up) — where adaptive actually has room:**

| Config | Mean avg_total | Mean p90_total | s42 avg/p90 | s7 avg/p90 | s123 avg/p90 |
|---|---|---|---|---|---|
| R8 control α=0.3 | 73.00 | 116.88 | 64.07 / 77.79 | 73.09 / 128.65 | 81.82 / 144.21 |
| **0.7 / 0.1 / 10** | **71.05** | **109.30** | 64.07 / 77.79 | 73.17 / 128.20 | **75.92 / 121.92** |
| diagnostic 0.7 / 0.3 / 5 | 71.09 | 109.34 | 64.07 / 77.79 | 73.27 / 128.30 | 75.92 / 121.92 |

Cold compare: mean avg_total −1.94 (−2.66%), p90_total −7.58 (−6.49%); seed 123 −7.21% avg / −15.46% p90, seed 42/7 neutral. So adaptive works as expected when learning starts from zero, but **the standard verification process (live seed 99 first) has already warm-started the compare**, hiding this benefit, while the drawback of low α at n≥14 shows through.

**Cold-start vs steady-state analysis:**
- **Cold** (n = 0–10): α 0.7 → 0.35 converges fast, the estimate approaches the true mean much earlier; synthetic 16× and cold compare −6.5% p90 confirm.
- **Warm** (n = 14–36 after live): α(n) = 0.15–0.29 < 0.3. The compare is only 52 patients/188 visits, short enough that the estimate "sticks" to the warm state; low α slows correction toward the compare seed's realized sample mean. Route rankings change → seed 123 worsens sharply. The floor-0.3 diagnostic tying R8 proves this is the cause, not a routing bug.
- **Live**: high α early in the run reacts to ±20% service noise (sample variance, not drift), causing churn; estimate accuracy was never the routing bottleneck (R8 was already good enough), so faster convergence does not translate into better metrics.
- **Steady-state variance**: objective (b) is met in the long synthetic test (3.5× lower), but in a 52-patient run this benefit does not offset the reduced responsiveness of route decisions.

**Files changed:** `backend/src/domain.rs` (`RoomStats.avg_abs_service_dev_min` + `#[serde(default)]`), `backend/src/engine/learn.rs` (`EwmaConfig`, adaptive α, diagnostic EWMA, `with_ewma`/`current_alpha`, 4 new tests), `backend/src/engine/sim.rs` (`RoomView` gains `learned_alpha` + `avg_abs_service_dev_min`), `backend/src/engine/router.rs` (literal tests gained fields only, no logic change), `docs/rounds/README.md`. Unchanged router/congestion logic, sim physics, baselines, priority queue, existing API contract, frontend. The sweep used temporary env overrides; **removed** — the final code hard-codes 0.7/0.1/10.

**Verification steps:**
1. `cargo test`: **68/68 pass** (64 existing tests + 4 new: α decreasing from init hits the floor exactly at n = 60; 5-sample convergence at least 40% faster than fixed 0.3; steady-state variance < 80% of fixed; a legacy snapshot missing the new fields loads, defaults to 0, and the next record upgrades correctly; the old persistence test updates expectations to α(0)=0.7 and checks the diagnostic).
2. `npm run build`: OK (frontend unchanged; only the pre-existing chunk > 500 kB warning).
3. Fresh `MEDIROUTE_DATA_DIR=/tmp/mediroute-r9-test`, separate server `PORT=8082`; live `{strategy:optimized, speed:5000, seed:99}` → finished 392.21 min, 52/52, 188 visits.
4. `POST /api/sim/compare` seed 42 → 7 → 123; baselines match R8 number-for-number; the fixed α=0.3 control reproduces R8 bit-identical.
5. Sweep 36 combos + 14 diagnostics (each combo fresh dir `/tmp/mediroute-r9-sweep/*`), plus cold compares without live warm-up.
6. Real legacy `backend/data/learning.json`: server loads without warnings, stats intact, new fields = 0, `learned_alpha` appears in `/api/rooms`.
7. Port 8080 untouched; no git commit/push.

**Assessment vs Round 8:**
- (a) Cold-start convergence: **met** — synthetic 16× closer after 5 samples; live MAE < 1 24 min earlier, MAE < 0.75 12 min earlier; cold compare mean −2.66% avg_total / −6.49% p90_total.
- (b) Steady-state variance: **met on long series** — 0.1006 vs 0.3483 (~3.5× lower) in the 800-sample test.
- (c) End-to-end per the verification process: **NOT met** — live +2.50% avg_total / +2.54% p90_total / +39.8% p90_wait; post-live compare +4.59% avg_total / +10.38% p90_total, with a heavy regression on seed 123 (+13.0% avg / +23.9% p90). No seed improves, baselines unchanged.

**Decision:** **NO COMMIT** — fails the end-to-end criterion despite meeting (a) and (b). The working tree retains code + docs for the coordinator's independent verification; nothing was committed.

**Lessons learned:** adaptive α is the right solution for *cold start*, but the verification process (live warm-up before compare) turns most of the learning warm, where α below 0.3 only makes the estimate track the new seed more slowly. The real problem is not "estimates converging faster" but "route rankings stable under ±20% noise once enough samples exist". A reasonable next direction: apply a high init α only to rooms **with no snapshot yet** (visit_count = 0 in persisted stats) and keep α = 0.3 for warm rooms; or use the `avg_abs_service_dev_min` diagnostic to detect true drift before raising α, instead of mechanically raising α for small n. Both need re-measurement because live cold start can still churn as seen in Round 9.

**Independent verification (coordinator):** cargo test 68/68; repeated the standard process (fresh dir → live seed 99 → compare seed 123): matches exactly (81.69/144.21 — regression vs R8 72.29/116.37, p90 even worse than ShortestQueue 126.75). Code reverted, cargo test 64/64.

**Decision:** DISCARD — clear end-to-end regression after warm-up (+4.59% avg, +10.38% p90 mean across 3 seeds). The agent's α=0.3 control reproduces R8 bit-identical, confirming the causal cause.

**Lessons learned:** adaptive α only wins at pure cold start (first-run MAE 16× better) but in a process with warm-up, low α when warm makes the estimate track shifts slowly — the service mean does not change during the day, so faster tracking brings no benefit, only noise. Fixed α = 0.3 is a good balance for a static workload.

---

## Round 10 — Downtime-aware routing (Optimized only)

**Status:** ✅ COMMITTED (verified independently — small win, no regression)

**Proposed aspect:**
- The router already received `downtime_remaining_min`, but the next-step cost still calculated wait at `now`: `walk + est_wait(now) + service`. When a room is down, this calculation both over-charges the walk portion (the patient has not arrived yet) and ignores the fact that the room may reopen before the patient arrives.
- Round 10 required: (1) account for remaining downtime precisely in the earliest slot; (2) do not charge downtime if it ends before the patient arrives; (3) the risk model uses only past/current information (outages that already happened, current remaining downtime), **does not read RNG/future schedule**; (4) change only Optimized, keeping FixedOrder/ShortestQueue, sim physics, API/JSON, frontend, and learning unchanged.

**Actual downtime mechanism (read from code, not assumed):**

`sim.rs::schedule_downtime_windows` runs exactly once in `World::new`:
```rust
let latest_start = downtime_latest_start_min();
for room in 0..self.scenario_rooms.len() {
    if !rng.gen_bool(DOWNTIME_PROBABILITY) {
        continue;
    }
    let start = rng.gen_range(DOWNTIME_EARLIEST_START_MIN..latest_start);
    let duration = rng.gen_range(DOWNTIME_MIN_MIN..DOWNTIME_MAX_MIN);
    self.schedule(start, Event::RoomDowntimeStart { room, duration_min: duration });
}
```
- 15% probability per room; start uniform in `[15, 148)` (`latest = 210·0.8 − 20`); duration 10–20 minutes.
- `RoomDowntimeStart` sets `down_until = now + duration`, increments `downtime_episodes`, and schedules `RoomDowntimeEnd`. Patients **already in service finish normally**; `dispatch_room` refuses to start new service while `now < down_until`; `RoomDowntimeEnd` clears `down_until` and then dispatches the queue again.
- Because the schedule runs only once, each room has **at most one outage per run**. A new test scanned 60 seeds and confirmed every start lies in `[15, 148)` and no room has 2 starts.
- The change in `sim.rs` is **additive telemetry** only: it adds `RoomState.downtime_episodes` (counts outages already started) and shares the `downtime_latest_start_min()` helper with the router. No RNG calls are added or removed, the RNG order is unchanged, and physics is unchanged → the seed stream and baseline behavior stay the same.

**Design:**

*Cost at arrival time (`congestion.rs` + `router.rs`):*
- `est_wait_at_arrival_min(load, service, capacity, arrive_in)` builds station free-times as `max(in_service_remaining, downtime_remaining)`, pours in queue/pending/en-route/likely-arrivals, and returns `max(0, min_station − arrive_in)`.
- `wait_at(room)` uses this function **only when** `downtime_remaining_min > 0`; rooms without downtime keep `est_wait_min` as in R8. Therefore:
  - downtime remaining 2 minutes, walk 3.5 → wait 0 (reopens before arrival);
  - downtime remaining 12 minutes, walk 3.5 → wait 8.5 (only the overlap after arrival);
  - queue that forms after reopening is still fully counted.
- Honest note: `est_wait_min` and `future_wait_min` **already** accounted for downtime correctly beforehand via the floor `max(in_service_remaining, downtime)`; no fix was needed there. The only bug was the first-step wait-from-now adding walk on top.
- Tried extending arrival-time pricing to **all** rooms (not just rooms currently down), with risk disabled: seed 42 64.68/81.47, seed 7 72.39/132.45, seed 123 80.87/132.18 (avg/p90) → mean 72.65/115.36, worse than R8 69.83/107.45. Kept the scope narrow to downtime only.

*Risk (`router.rs`):*
- `RoomLoad.downtime_episodes` (from the telemetry above) indicates whether the room has used its single outage yet.
- `downtime_risk(room)`:
  - `episodes > 0` → `0` (outage already spent, safe until the end of the run);
  - `now ≥ 148` → `0` (start window closed);
  - `now ≤ 15` → `0.15`;
  - otherwise use the Bayesian posterior, conditional on no outage having been observed yet:
    `0.15 · (148 − now)/133 / (1 − 0.15 · (now − 15)/133)`.
  - It does not read the event heap, seed, or RNG; it uses only observed episodes + the public policy.
- **Usage:** risk is a tie-break **after walk** inside the 25% tie-band, not an additive cost. Variants measured:
  - additive `+risk·est_service·0.1` (the literal suggestion): seeds 42/7 bit-identical to R8, but seed 123 72.29→78.03 avg / 116.37→129.42 p90; mean +1.9 avg / +4.4 p90 → rejected.
  - additive `0.01`: seed 123 even worse (81.17/144.21). A tiny amount is still enough to change tie-band membership and cascade into later decisions → rejected.
  - risk before walk: pulls avg_walk up (seed 42 11.51→12.93; seed 123 10.81→12.74), mean avg 72.62 → rejected.
  - risk after walk but multiplied by `est_service`: still changes ranking by service among equal-walk pairs (many rooms clamped at 5.0 from the entrance), mean avg 71.10 → rejected.
  - **chosen:** risk after walk, distinguishing only spent vs unspent (no service factor). Rooms without an outage share the same risk at the same time; only outage history changes the ranking.
- Final tie-break order: cost band 25% → walk → **downtime risk** → unblocks → downstream depth → en-route → queue → package position.

**Results — compare (standard verification: fresh dir → live seed 99 → compare 42/7/123):**

Seed 42:

| Strategy | avg_total | avg_wait | p90_total | avg_walk |
|---|---|---|---|---|
| **Optimized (R10)** | **64.07** | **2.91** | **77.79** | **11.51** |
| Optimized (R8) | 64.07 | 2.91 | 77.79 | 11.51 |
| FixedOrder | 77.02 | 5.91 | 130.21 | 13.45 |
| ShortestQueue | 71.32 | 4.33 | 103.89 | 13.57 |

Seed 7:

| Strategy | avg_total | avg_wait | p90_total | avg_walk |
|---|---|---|---|---|
| **Optimized (R10)** | **72.23** | **4.42** | **128.20** | **11.85** |
| Optimized (R8) | 73.13 | 4.68 | 128.20 | 11.76 |
| FixedOrder | 84.39 | 7.08 | 138.89 | 13.82 |
| ShortestQueue | 77.82 | 5.56 | 139.75 | 13.10 |

Seed 123:

| Strategy | avg_total | avg_wait | p90_total | avg_walk |
|---|---|---|---|---|
| **Optimized (R10)** | **72.29** | **6.60** | **116.37** | **10.81** |
| Optimized (R8) | 72.29 | 6.60 | 116.37 | 10.81 |
| FixedOrder | 86.44 | 10.17 | 145.23 | 12.73 |
| ShortestQueue | 81.09 | 8.67 | 126.75 | 12.49 |

**Assessment vs Round 8 (Optimized):**

| Seed | Δ avg_total | Δ avg_wait | Δ p90_total | Δ avg_walk |
|---|---|---|---|---|
| 42 | 0 | 0 | 0 | 0 |
| 7 | −0.90 (−1.23%) | −0.26 (−5.51%) | 0 | +0.09 (+0.72%) |
| 123 | 0 | 0 | 0 | 0 |
| **Mean 3 seeds** | **−0.30 (−0.43%)** | **−0.09 (−1.82%)** | **0** | **+0.03 (+0.25%)** |

- Met the round criteria: mean avg_total better, p90_total unchanged, **no seed regressed**. The benefit is concentrated in seed 7: a few near-tie decisions chose a room whose outage was over instead of a room still at risk, reducing both avg_total and avg_wait; walk increased by 0.085 minutes but by less than the total decrease.
- Baselines match **number-for-number** with R8 on all 3 seeds × 4 metrics (77.0249/5.9134/130.2068/13.4523; 71.3185/4.3273/103.8948/13.5720; 84.3898/7.0846/138.8932/13.8233; 77.8194/5.5577/139.7541/13.0962; 86.4409/10.1679/145.2265/12.7288; 81.0868/8.6726/126.7505/12.4931).

Live seed 99 (fresh dir, speed 5000): finished at `t = 399.74754874492817` minutes, 52/52, 188 visits; **78.0142 / 6.6068 / 146.5977 / 16.4670 / 11.0451** (avg_total/avg_wait/p90_total/p90_wait/avg_walk) — **exactly matching R8**, confirming that live routing was not changed by the risk tie-break on this seed and that the learned stats used for compare are the same state.

**Files changed:** `backend/src/scenario.rs` (`DOWNTIME_EARLIEST_START_MIN`, shared `downtime_latest_start_min()`, behavior unchanged), `backend/src/engine/sim.rs` (`RoomState.downtime_episodes` + exposure in `room_loads`; telemetry only, RNG/physics unchanged; 2 tests), `backend/src/engine/congestion.rs` (`projected_wait_at` refactor + `est_wait_at_arrival_min`; 4 tests), `backend/src/engine/router.rs` (`RoomLoad.downtime_episodes`, arrival-time `wait_at` for rooms currently down, `downtime_risk` posterior, risk tie-break after walk; 4 tests), `docs/rounds/README.md`. Unchanged: FixedOrder/ShortestQueue, the sim's downtime mechanism, priority queue, `learn.rs`, API/JSON, frontend.

**Verification steps:**
1. `cargo test`: **74/74 pass** (64 existing tests + 10 new: 2 sim — outage window bounded/non-repeating across 60 seeds, load retains the episode after reopen; 4 congestion — short downtime cheaper than long, downtime ending before arrival is not charged, wait uses the correct reopen time, queue after reopen is still counted; 4 router — cost at arrival time, risk by history + start window, risk only breaks ties after walk, baselines keep their choices).
2. `npm run build`: OK (frontend unchanged; only the pre-existing chunk > 500 kB warning).
3. Fresh `MEDIROUTE_DATA_DIR=/tmp/mediroute-r10-test`, separate server `PORT=8082`; live `{strategy:optimized, speed:5000, seed:99}` → finished 399.75 minutes, 52/52, 188 visits, metrics matching R8.
4. `POST /api/sim/compare` seed 42 → 7 → 123; recorded all 4 metrics for 3 strategies; confirmed baselines match R8 number-for-number.
5. Diagnostic sweep: additive risk 0.1 / 0.01, arrival-time wait for all rooms, risk before walk, risk after walk with service factor — all were worse; only spent-vs-unspent after walk won.
6. Port 8080 untouched; no git commit/push.

**Decision:** **SMALL WIN — no commit** as the round required. Mean across 3 seeds improved avg_total (−0.43%) and avg_wait (−1.82%), p90_total unchanged, no seed worse; baselines and live seed 99 bit-identical to R8. The working tree retains the code + docs for the coordinator's independent verification.

**Lessons learned:** with the mechanism "each room has at most one 10–20 minute outage", future risk is very small (posterior ≈ 0.06–0.15, expected delay around 0.2 minutes), so it is not strong enough to change a route meaningfully. Making risk an additive cost is actually **harmful** because the 25% tie-band compares proportionally: a 0.01–0.2 minute term is enough to change band membership and cascade. General lesson: in a cost-band architecture, low-confidence information must be a late tie-break, not an additive term; and an honest risk model must distinguish "outage already spent" from "no outage yet", but must not invent an extra ranking by service when all rooms are still outage-free.

**Independent verification (coordinator):** cargo test 74/74; repeated the standard process (fresh dir → live seed 99 → compare 42/7/123): matches exactly — seed 7 improved 73.13→72.23 avg_total / 4.68→4.42 avg_wait; seeds 42/123 unchanged number-for-number; baselines unchanged.

**Decision:** COMMIT — mean avg_total −0.43%, no per-seed regression, and the model is now correct: wait is evaluated at the patient's expected **ARRIVAL** time (downtime ending before arrival is no longer penalized), and the Bayesian posterior risk is used only as a late tie-break.

---

## Round 11 — Unifying anti-herding with the λ-model (Optimized only)

**Status:** ⚠️ NO COMMIT — regression vs R10, reported honestly as the round required.

**Proposed aspect:**
- HEAD (R10) has **two parallel competitor models**:
  - (a) the Round 1-style likely-arrivals heuristic: projects each patient currently in the system onto subsequent rooms with weight `1/|eligible|`, used in predictive wait (`room_loads` → `est_wait_min`);
  - (b) the λ-model `ArrivalDemand` (R8): arrival rate by segment × `share(room)`, used in lookahead `future_wait_min`.
- Consequence: the predictive path estimates competitors discretely per patient and does not know λ; lookahead adds both λ and likely arrivals. During rush, the two models can over/under-count inconsistently.
- Round 11 required: replace the heuristic in predictive wait with the λ-model `E_r(now, now+horizon)`, horizon = expected time until the patient arrives + wait; λ=0 → no phantom competitors; after the replacement there is only **one estimate source** (ArrivalDemand); queue/pending/en-route remain facts counted in full; only Optimized changes.

**Design:**

*Predictive wait (`congestion.rs::est_wait_with_demand_min`):*
- No demand (legacy context/test) → fall back to `est_wait_min` as before.
- With demand:
  - `base = projected_wait(load, service, capacity, 0)` — wait due to committed load;
  - `horizon = walk + base` — when the patient is expected to start service;
  - build station free-times from `in_service_remaining`, floored by `downtime_remaining_min`;
  - pour `pending_starts + queue_len + en_route` jobs at ready 0, full weight — **facts, anti-herding kept as before**;
  - add λ arrivals in 1-minute slices over `[now, now+horizon]`: work = `service × E_r(slice) × scale`, ready at the exact arrival minute;
  - return the earliest free station = expected wait.
- λ=0 (lull 120–150, after 210) → no jobs added → wait reflects only committed load, **no phantom competitors**.
- Competitors are proportional to `share(room)` because `E_r` is linear in share.
- `wait_at`: rooms currently down keep the R10 path `est_wait_at_arrival_min`; the rest use the new function with `PREDICTIVE_DEMAND_SCALE` = **0.8**.

*One estimate source (`future_wait_min`):*
- When demand exists, lookahead uses λ only; `likely_arrivals` remains only as a fallback when `demand = None`. R10 added both (the R10 author argued there was no double-count because likely arrivals are patients already in the system, not included in λ), but the round requirement was one source after replacement, so it was removed.
- `sim.rs::room_loads` drops `add_likely_arrivals` entirely and maps `in_service_remaining`; only facts remain: `queue_len`, `pending_starts`, `in_service`, `in_service_remaining`, `en_route`, downtime. RNG, physics, priority queue, and learning unchanged.

**Scale sweep (mean of 3 seeds 42/7/123, compare on fresh dir + live seed 99):**

| Scale | s42 avg/p90 | s7 avg/p90 | s123 avg/p90 | Mean avg | Mean p90 | Live s99 avg/wait/p90 |
|---|---|---|---|---|---|---|
| 0.4 (diagnostic) | 64.74/84.15 | 76.42/128.34 | 78.63/129.42 | 73.26 | 113.97 | 81.82/7.66/148.94 |
| 0.6 (diagnostic) | 64.63/82.17 | 74.95/131.72 | 81.38/132.18 | 73.66 | 115.36 | 79.97/7.10/150.19 |
| **0.8 (chosen)** | **64.11/76.44** | **75.35/130.37** | **77.57/132.18** | **72.34** | **113.00** | **80.87/7.33/139.06** |
| 1.0 | 64.09/76.38 | 73.75/131.02 | 82.61/144.18 | 73.48 | 117.19 | 84.05/8.22/163.81 |
| 1.2 | 64.87/83.26 | 73.75/129.67 | 79.89/127.55 | 72.83 | 113.49 | 83.75/8.06/163.81 |
| R10 (ref) | 64.07/77.79 | 72.23/128.20 | 72.29/116.37 | 69.53 | 107.45 | 78.01/6.61/146.60 |

- 0.8 won both mean avg (72.34) and mean p90 (113.00) in the sweep; 0.4/0.6 are diagnostics outside the requested 0.8/1.0/1.2 set, confirming the regression is not a calibration issue. There is no monotonic trend by scale → the problem is **lost information**, not the coefficient.

**Results — compare at scale 0.8 (standard verification: fresh dir → live seed 99 → compare 42/7/123):**

Seed 42:

| Strategy | avg_total | avg_wait | p90_total | avg_walk |
|---|---|---|---|---|
| Optimized (R11) | 64.11 | 2.95 | 76.44 | 11.41 |
| Optimized (R10) | 64.07 | 2.91 | 77.79 | 11.51 |
| FixedOrder | 77.02 | 5.91 | 130.21 | 13.45 |
| ShortestQueue | 71.32 | 4.33 | 103.89 | 13.57 |

Seed 7:

| Strategy | avg_total | avg_wait | p90_total | avg_walk |
|---|---|---|---|---|
| Optimized (R11) | 75.35 | 5.20 | 130.37 | 11.99 |
| Optimized (R10) | 72.23 | 4.42 | 128.20 | 11.85 |
| FixedOrder | 84.39 | 7.08 | 138.89 | 13.82 |
| ShortestQueue | 77.82 | 5.56 | 139.75 | 13.10 |

Seed 123:

| Strategy | avg_total | avg_wait | p90_total | avg_walk |
|---|---|---|---|---|
| Optimized (R11) | 77.57 | 8.19 | 132.18 | 10.64 |
| Optimized (R10) | 72.29 | 6.60 | 116.37 | 10.81 |
| FixedOrder | 86.44 | 10.17 | 145.23 | 12.73 |
| ShortestQueue | 81.09 | 8.67 | 126.75 | 12.49 |

**Assessment vs Round 10 (Optimized, mean 3 seeds):**

| Metric | R10 | R11 (0.8) | Δ |
|---|---|---|---|
| avg_total | 69.53 | 72.34 | **+2.81 (+4.04%)** |
| avg_wait | 4.64 | 5.45 | **+0.80 (+17.3%)** |
| p90_total | 107.45 | 113.00 | **+5.55 (+5.16%)** |
| avg_walk | 11.39 | 11.35 | −0.04 (−0.36%) |

- Worse than R10 on **both** mandatory criteria (avg_total and p90_total). Per-seed avg_total is worse everywhere: s42 +0.04, s7 +3.13, s123 +5.27. p90 is better only on s42 (−1.34), worse on s7 (+2.17) and s123 (+15.82).
- Live seed 99: 80.87/7.33/139.06/11.28 (t = 408.37 minutes) vs R10 78.01/6.61/146.60/11.05 — avg worse by 2.86, p90 better by 7.54; not enough to offset the compare mean.
- Baselines match **number-for-number** with R10 on all 3 seeds × 4 metrics: FixedOrder 77.0249/5.9134/130.2068/13.4523; 84.3898/7.0846/138.8932/13.8233; 86.4409/10.1679/145.2265/12.7288. ShortestQueue 71.3185/4.3273/103.8948/13.5720; 77.8194/5.5577/139.7541/13.0962; 81.0868/8.6726/126.7505/12.4931.

**Cause of the regression (honest):**
- The unified model removes **in-system cross-room projections** from lookahead: λ knows only arrivals from outside, not which rooms the patients already in the system will spill into over the next few dozen minutes. When many patients have already entered (mid-rush), lookahead under-predicts congestion at hot rooms → Optimized herds into them more strongly; s123 is the clearest case (avg_wait 6.60 → 8.19, p90 116.37 → 132.18).
- The predictive λ during the walk can over-charge hot rooms mid-peak, but avg_walk barely changes (11.39 → 11.35), so the main loss is on the lookahead side.
- Scale cannot rescue it: 0.4/0.6/0.8/1.0/1.2 all lose to R10 on the mean; 0.8 is merely the best point tried. Keeping likely arrivals in lookahead alongside predictive λ could recover most of it, but that violates the "one source only" requirement, so it was not done.

**Files changed:** `backend/src/engine/congestion.rs` (`est_wait_with_demand_min`, shared `station_free_times` refactor, single-source `future_wait_min`; 4 new tests), `backend/src/engine/router.rs` (`PREDICTIVE_DEMAND_SCALE = 0.8`, λ path in `wait_at` for rooms not down, R10 downtime path unchanged; 1 new test), `backend/src/engine/sim.rs` (removed the heuristic + `in_service_remaining` mapping, facts-only `room_loads()`, removed unused imports; 1 new test), `docs/rounds/README.md`. Unchanged: FixedOrder/ShortestQueue, sim mechanism, API/JSON, frontend, learning, R10 downtime handling.

**Verification steps:**
1. `cargo test`: **80/80 pass** (74 existing tests + 6 new: 4 congestion — rush > lull at equal load, λ=0 no phantom, competitor proportional to share (ratio 9.0 exact), no double-count with legacy likely weights/future_wait; 1 sim — `room_loads` facts only; 1 router — predictive wait by demand, baselines keep their choices).
2. `npm run build`: OK (frontend unchanged; only the pre-existing chunk > 500 kB warning).
3. Fresh `MEDIROUTE_DATA_DIR=/tmp/mediroute-r11-test`, separate server `PORT=8082`; live `{strategy:optimized, speed:5000, seed:99}` → finished at `t = 408.3704369444138` minutes, 52/52, 80.8696/7.3316/139.0584/11.2799.
4. `POST /api/sim/compare` seed 42 → 7 → 123; recorded all 4 metrics; baselines match R10 number-for-number.
5. Sweep with a separate fresh dir for each scale 0.4/0.6/0.8/1.0/1.2; 0.8 was chosen.
6. Port 8080 untouched; **no git commit/push**.

**Decision:** **NO COMMIT** — did not meet the criterion "better than R10 on avg_total or p90_total (mean)". The working tree retains the code + docs for the coordinator's independent verification.

**Lessons learned:** the λ-model is a good model for arrivals from outside but **cannot replace** the projection of patients already in the system; the two sources complement each other rather than being two estimates of the same quantity. Forcing a single λ source loses the signal "a hot room is about to be flooded by the patients already inside", and anti-herding stops working exactly when it is most needed (mid-rush, when queues have already formed at other rooms). General lesson: before unifying two models, determine whether they have the same support; if the supports differ (external arrivals vs in-system routing), "one source" is information loss, not deduplication.

**Independent verification (coordinator):** cargo test 80/80; repeated the standard process (fresh dir → live seed 99 → compare seed 123): matches exactly (77.57/132.18 — regression vs R10 72.29/116.37). Code reverted, cargo test 74/74.

**Decision:** DISCARD — mean avg_total +2.81, p90 +5.55; no scale (0.4–1.2) is better than R10.

**Lessons learned:** the old anti-herding heuristic does not duplicate λ — it projects the behavior of patients ALREADY IN the system (in-process routing decisions), something λ does not know (λ models only arrivals from outside). Replacing it = information loss, not dedupe. Architectural conclusion: the 2 competitor sources are complementary: λ for future arrivals, the heuristic for internal patients; any later refinement must add both.

---

## Round 12 — Room-load variance minimization (Optimized only)

**Status:** ⚠️ NO COMMIT — every variant (tie-break within the cost band and additive term) was worse than R10 on the 3-seed mean; the code was reverted exactly to R10, keeping only the record.

**Proposed aspect:**
- R10 optimizes each decision by `walk + est_wait + service + 0.5·lookahead`. `est_wait` sees only the queue and committed work; two rooms with the same wait can still be accumulating very different amounts of work (many jobs in service, many en-route, many λ arrivals coming), especially at rooms with capacity 2–3.
- Anti-herding (R1/R10) counts competitors **for the room under consideration**; balance must be a **cross-room** signal: avoid adding work to an already-heavy room even when the current wait does not yet reflect it.
- Round requirement: projected load = facts + λ external in the window + internal routing projections (R11 already proved anti-herding is mandatory); aggregate by capacity; integrate as a small tie-break inside the 25% band **or** a small additive term; change only Optimized; leave FixedOrder/ShortestQueue, sim, API/JSON, frontend, learning unchanged.
- R10 reference Optimized: s42 64.07/2.91/77.79/11.51; s7 72.23/4.42/128.20/11.85; s123 72.29/6.60/116.37/10.81 (avg_total/avg_wait/p90_total/avg_walk).

**Design (evaluated, then reverted):**

*Projected total load (`router.rs::projected_total_load`):*
```
committed = Σ max(0, in_service_remaining)
          + (queue_len + pending_starts + en_route) × est_service
internal  = Σ weight of likely_arrivals with ready_in_min ≤ 30
external  = E_r(now, now + 30)                 // ArrivalDemand R8
load      = (committed + (internal + external) × est_service) / capacity
```
- Unit: service-minutes per station. A `jobs_per_station` variant divides the **number of jobs** by capacity instead of multiplying by service (swept, did not win).
- `in_service` without remaining (fallback) counts as a full service; internal uses the R10 heuristic unchanged; external uses the package's exact room share.
- Two integration approaches:
  1. **Tie-break** within the existing cost band (band sweep 0.05–0.25), tried at positions: after walk, after risk, after downstream depth, after en-route, after queue. Optional gates: relative load tolerance, max wait difference, max absolute wait.
  2. **Additive**: `total_cost += α × load`, α sweep 0.002–0.2; band and R10 tie-break chain unchanged.
- 6 new tests were written and passed during evaluation: same cost → choose the lower-load room; outside the cost band balance must not override; load = facts + λ + internal exactly; baselines keep their choices; distance still beats balance; single eligible room unchanged. These tests were reverted together with the code.

**Tie-break sweep (window 30 minutes, internal + λ, mean of 3 seeds 42/7/123):**

| Configuration | s42 avg/p90 | s7 avg/p90 | s123 avg/p90 | Mean avg/p90 | Live s99 avg/p90 |
|---|---|---|---|---|---|
| R10 (ref) | 64.07/77.79 | 72.23/128.20 | 72.29/116.37 | **69.53/107.45** | 78.01/146.60 |
| off control | 64.07/77.79 | 72.23/128.20 | 72.29/116.37 | 69.53/107.45 | 78.01/146.60 |
| after risk, band 25% | 66.43/88.38 | 72.71/130.55 | 78.64/124.16 | 72.59/114.37 | 81.07/139.48 |
| after depth, band 25% | 66.38/93.69 | 72.71/130.55 | 78.64/124.16 | 72.58/116.13 | 81.07/139.48 |
| after en-route, band 25% | 64.25/81.24 | 72.51/128.65 | 78.64/124.16 | **71.80/111.35** | 81.08/150.19 |
| after queue, band 25% | 64.25/81.24 | 72.51/128.65 | 78.64/124.16 | 71.80/111.35 | 81.08/150.19 |
| after risk, band 5% | 65.31/90.78 | 73.38/128.30 | 81.89/144.21 | 73.53/121.10 | 79.48/154.46 |
| after risk, band 10% | 64.94/82.93 | 73.32/128.20 | 81.73/140.25 | 73.33/117.13 | 79.38/154.46 |
| after risk, band 15% | 64.59/82.96 | 74.19/124.59 | 78.29/127.95 | 72.35/111.84 | 81.07/139.48 |
| facts only (drop λ + internal) | 66.08/86.18 | 72.16/131.02 | 81.79/144.21 | 73.34/120.47 | 80.56/147.78 |
| internal only (drop λ) | 65.61/93.69 | 72.71/130.55 | 78.64/124.16 | 72.32/116.13 | 81.81/155.22 |
| λ external only (drop internal) | 66.10/94.60 | 73.42/124.59 | 78.64/124.16 | 72.72/114.45 | 81.08/150.19 |

Gate/aggregation variants (none won):

| Configuration | s42 avg/p90 | s7 avg/p90 | s123 avg/p90 | Mean avg/p90 |
|---|---|---|---|---|
| after en-route + wait gate ≤ 5 | 64.25/81.24 | 73.27/128.65 | 78.64/124.16 | 72.05/111.35 |
| after en-route + max wait ≤ 10 | 64.25/81.24 | 73.49/128.65 | 78.64/124.16 | 72.13/111.35 |
| after en-route + relative tol 25% | 64.94/85.59 | 72.44/124.41 | 78.64/124.16 | 72.01/111.38 |
| jobs/station, after risk | 65.94/90.78 | 73.35/124.59 | 78.64/124.16 | 72.64/113.18 |
| jobs/station, after en-route | 64.94/85.59 | 72.51/128.65 | 78.64/124.16 | 72.03/112.80 |
| window 10 minutes, after en-route | 64.25/81.24 | 72.51/128.65 | 78.64/124.16 | 71.80/111.35 |

**Additive α sweep (R10 band + tie-break chain unchanged, balance tie-break off):**

| α | s42 avg/p90 | s7 avg/p90 | s123 avg/p90 | Mean avg/p90 | Live s99 avg/p90 |
|---|---|---|---|---|---|
| 0.002 | 64.07/77.79 | 72.23/128.20 | 82.11/144.21 | 72.80/116.73 | 78.01/146.60 |
| 0.005 | 64.07/77.79 | 72.23/128.20 | 82.11/144.21 | 72.80/116.73 | 78.01/146.60 |
| 0.01 | 64.07/77.79 | 72.23/128.20 | 78.03/129.42 | **71.44/111.80** | 78.01/146.60 |
| 0.02 | 64.58/83.04 | 72.23/128.20 | 79.35/131.24 | 72.05/114.16 | 77.47/143.53 |
| 0.05 | 65.18/81.96 | 72.66/125.21 | 79.35/131.24 | 72.40/112.80 | 77.32/143.53 |
| 0.1 | 64.66/82.98 | 72.64/124.92 | 79.48/131.24 | 72.26/113.05 | 83.17/174.16 |
| 0.2 | 64.84/85.04 | 71.36/128.30 | 78.43/127.95 | 71.54/113.76 | 81.53/166.49 |

**Assessment vs Round 10:**
- Best tie-break (after en-route / after queue, band 25%): mean avg 71.80 (**+2.27, +3.27%**), p90 111.35 (**+3.90, +3.63%**); live 81.08/150.19 vs 78.01/146.60. Worse on both criteria.
- Best additive (α = 0.01): mean avg 71.44 (**+1.91, +2.75%**), p90 111.80 (**+4.35, +4.05%**); live matches R10 but s123 72.29/116.37 → 78.03/129.42. Worse on both criteria.
- No configuration beat R10. s42 and s7 usually stayed unchanged or moved slightly; **s123 is where the regression lives**: most tie-break variants land on exactly the same path 78.64/124.16; bands 5–10% and facts-only are even worse (81.7–81.9 avg / 140–144 p90); α = 0.002 also lands on 82.11/144.21. Many very different configurations converge on the same path → one early decision flipped and cascaded, rather than continuous optimization.
- α = 0.002 corresponds to ~0.02 minutes on a load of ~10 minutes/station yet is still enough to flip the s123 path; α = 0.005 gives exactly the same result as α = 0.002 while α = 0.01 differs → discrete response, confirming the cost-band membership mechanism.
- Baselines match **number-for-number** with R10 on all 3 seeds × 4 metrics (s42 FO 77.0249/5.9134/130.2068/13.4523, SQ 71.3185/4.3273/103.8948/13.5720; s7 FO 84.3898/7.0846/138.8932/13.8233, SQ 77.8194/5.5577/139.7541/13.0962; s123 FO 86.4409/10.1679/145.2265/12.7288, SQ 81.0868/8.6726/126.7505/12.4931). Live seed 99 after revert matches R10 exactly: 78.0142/6.6068/146.5977/11.0451.

**Why it failed (honest):**
- 7/9 rooms have capacity 1: "work per station" nearly coincides with `est_wait + service`, so the load signal adds no information, only noise.
- For rooms with capacity 2–3, the differing load lies in jobs not yet arrived (internal/λ); prioritizing that difference in the 25% band can override the critical-path tie-break (unblocks/depth) tuned since R3–R10, damaging the overall tour even if it is locally "more balanced".
- The 25% band compares **proportionally**, so any term (even ~0.02 minutes) changes membership and cascades. "Small" in magnitude does not mean "small" in behavior.
- Truly minimizing variance requires global assignment/batch or a two-level objective; a per-decision tie-break is not the right intervention point.

**Files changed:** no code changes remain — `backend/src/engine/router.rs` (implementation + temporary sweep knobs) and files rustfmt'ed during the experiment were reverted to HEAD. Only `docs/rounds/README.md` adds this section. The evaluated implementation patch was stored outside the repo at `/tmp/r12-experiment.patch` for reference; not committed.

**Verification steps:**
1. During evaluation: `cargo test` **80/80 pass** (74 existing + 6 new). After revert: **74/74 pass**.
2. `npm run build`: OK (frontend unchanged; only the pre-existing chunk > 500 kB warning).
3. Fresh `MEDIROUTE_DATA_DIR=/tmp/mediroute-r12-test`, separate server `PORT=8082`; live `{strategy:optimized, speed:5000, seed:99}` → finished 52/52, **78.0142/6.6068/146.5977/11.0451** (matches R10).
4. `POST /api/sim/compare` seed 42 → 7 → 123: Optimized **64.0700/2.9150/77.7868/11.5110; 72.2299/4.4230/128.1961/11.8492; 72.2939/6.5969/116.3665/10.8056** — exactly R10; baselines exact.
5. Sweep: 10 tie-break configurations + 6 gate/aggregation variants + 7 α values; a separate fresh dir per configuration, live seed 99 + compare 42/7/123.
6. Port 8080 untouched; **no git commit/push**.

**Decision:** **DISCARD — no commit.** No configuration beat R10 on avg_total or p90_total (mean); every variant regressed, worst on seed 123. The working tree returned exactly to R10 (code) + this record.

**Lessons learned:** in a cost-band architecture, adding a weak secondary signal as a "small" tie-break is **not** small at all: it changes band membership and cascades into later decisions. The same holds for an additive term — α = 0.002 (≈0.02 minutes) is enough to flip a path. Cross-room load is mostly already inside `est_wait + lookahead` at capacity-1 rooms; at large-capacity rooms, the work-per-station difference appears only in transients, and prioritizing it breaks the tuned critical-path order. To truly minimize variance, change the intervention point (global/batch objective), not add another tie-break key to a greedy per-patient router. Secondary lesson: a 3-seed benchmark is very sensitive — one early flip on s123 dominates the whole mean; sweeps need more seeds to distinguish "weak signal" from "cascade noise".

**Independent verification (coordinator):** the repo is clean after the agent's self-revert (only docs modified), cargo test 74/74 — HEAD code is still R10. The agent evaluated 2 directions (multi-position tie-break + additive α sweep 0.002–0.2) with 6 new tests passing during evaluation; every configuration was worse than R10 (mean avg +1.91 to +2.27). Experiment patch saved at /tmp/r12-experiment.patch.

**Decision:** DISCARD — every variant regressed. Key diagnosis: the load signal mostly DUPLICATES information already in est_wait + lookahead (both already aggregate the same facts + λ + internal projection); the 25% cost band is very sensitive (α=0.002 is enough to flip a decision). To really reduce variance, change the intervention point (global/batch objective) — the batch direction already failed in R2.

**Series lesson (R2, R12):** the per-decision router is saturated at a local optimum — every local intervention (batch window, balance tie-break, additive load) regresses because it breaks the tuned cost structure. Remaining promising directions: change SERVING (scheduling inside the room — e.g., service order by remaining tour instead of FIFO), or change the SCENARIO (more capacity/rooms) and re-measure.

---

## Round 13 — SRTF room scheduling (changing the intervention point: serving policy)

**Status:** ❌ DISCARDED (5-seed verification: mean p90 regressed due to starvation — trading avg for p90, contrary to the objective)

**Proposed aspect:**
- The per-decision router is saturated (R2, R12). This round does **not touch the router**; it changes the intervention point to **service order inside the room**.
- Scheduling theory: SPT/SRTF minimizes mean flow time in single/multi-server queues. FIFO within each priority class makes a patient with 1 room left (finishes fast) wait behind a patient with 5 rooms left.
- Apply the **same policy to all 3 strategies** because this is a hospital-wide serving policy; that keeps the routing comparison fair. The inevitable consequence: FixedOrder/ShortestQueue numbers **change vs R10**, which is expected, not a bug.
- Router/congestion, API/JSON, frontend, learning, downtime, arrivals unchanged. `est_wait` in the router still models the queue as FIFO; SRTF makes actual wait deviate slightly from the estimate — acceptable and measured in the results.

**Design:**
- **Enqueue:** always `push_back` — the physical queue keeps arrival order; urgent is not inserted at enqueue time.
- **Dispatch (when a station is free):** scan the queue and select by:
  1. **highest priority class present** in the queue (urgent > normal) — urgent always goes before normal regardless of tour;
  2. within that class, **smallest remaining tour**;
  3. equal tour → **earliest queue position** (arrival order / FIFO).
- **Remaining tour** = sum of **learned** `est_service_min` (learner stats) for the `required_rooms` not yet `done`. The room currently queued is included in this sum, but it is a constant across all candidates in the same queue, so it does not affect order.
- **No preemption:** an in-service patient always runs to completion; SRTF only picks the next patient when a slot is free.
- **Deterministic:** the queue is a `VecDeque`, selection scans by index, and strict `<` comparison keeps the first index on ties; no RNG added.
- Implementation lives in `sim.rs`: `enqueue`, `remaining_tour_min`, `next_dispatch_index`, `dispatch_room`. `router.rs` and `congestion.rs` unchanged.

**Results — compare** (standard verification: fresh `MEDIROUTE_DATA_DIR=/tmp/mediroute-r13-test` → live seed 99 speed 5000→clamp 60 → compare 42/7/123; headless, same arrivals/packages/service samples):

Seed 42:

| Strategy | avg_total | avg_wait | p90_total | avg_walk |
|---|---|---|---|---|
| **Optimized** | **62.3441** | **2.4480** | **73.9682** | **11.5003** |
| FixedOrder | 77.1379 | 5.9442 | 132.5264 | 13.4523 |
| ShortestQueue | 70.3007 | 4.0495 | 106.4523 | 13.5746 |

Seed 7:

| Strategy | avg_total | avg_wait | p90_total | avg_walk |
|---|---|---|---|---|
| **Optimized** | **71.9474** | **4.3287** | **134.5026** | **11.9275** |
| FixedOrder | 83.9108 | 6.9594 | 140.6774 | 13.8233 |
| ShortestQueue | 78.3727 | 5.6160 | 138.8932 | 13.4266 |

Seed 123:

| Strategy | avg_total | avg_wait | p90_total | avg_walk |
|---|---|---|---|---|
| **Optimized** | **72.2136** | **6.5611** | **99.1683** | **10.8479** |
| FixedOrder | 84.3932 | 9.5697 | 110.5047 | 12.7288 |
| ShortestQueue | 80.4086 | 8.4379 | 109.7889 | 12.6182 |

Baselines **no longer match R10** — expected because the serving policy changed. Example of the largest change: seed 123 FixedOrder −2.05 avg_total / −34.72 p90_total; ShortestQueue −0.68 / −16.96. The routing logic of both baselines is intact; only the order in which patients are served inside rooms changed. R13 is therefore assessed by (a) Optimized vs R10 numbers, (b) internal comparison of the 3 strategies, (c) p90 guardrail — not by using baselines to cross-check against R10.

**Assessment vs Round 10 (Optimized):**

| Seed | Δ avg_total | Δ avg_wait | Δ p90_total | Δ avg_walk |
|---|---|---|---|---|
| 42 | −1.7259 (−2.69%) | −0.4670 (−16.02%) | −3.8186 (−4.91%) | −0.0107 (−0.09%) |
| 7 | −0.2825 (−0.39%) | −0.0943 (−2.13%) | **+6.3065 (+4.92%)** | +0.0783 (+0.66%) |
| 123 | −0.0803 (−0.11%) | −0.0358 (−0.54%) | −17.1982 (−14.78%) | +0.0423 (+0.39%) |
| **Mean 3 seeds** | **−0.6962 (−1.00%)** | **−0.1990 (−4.28%)** | **−4.9034 (−4.56%)** | **+0.0366 (+0.32%)** |

- **Criterion (a) — Optimized improves vs R10:** avg_total better on all 3 seeds (−2.69% / −0.39% / −0.11%), avg_wait better on all 3, p90_total better on seeds 42 and 123 (−4.91% / −14.78%) but **worse on seed 7 +4.92%**. The 3-seed mean is better on avg_total, avg_wait, and p90_total. Met in the sense of mean + per-seed avg_total; caveat: seed 7 p90 is close to the guardrail.
- **Criterion (b) — Optimized still best on avg_total + p90_total:** met on all 3 seeds. avg_total gaps: s42 7.96 minutes ahead of ShortestQueue/14.79 ahead of FixedOrder; s7 6.43/11.96; s123 8.20/12.18. p90_total gaps: s42 32.48/58.56; s7 6.16/8.72; s123 10.62/11.34.
- **Criterion (c) — p90_total not worse than R10 by more than 5%:** met, but close to the threshold on seed 7: 134.5026 / 128.1961 = **+4.92%** (< 5%); seed 42 −4.91%, seed 123 −14.78%.
- The largest benefit is on seed 123 — where contention is high and FIFO used to push the tail up strongly: Optimized p90 −17.20 minutes, FixedOrder p90 −34.72, ShortestQueue p90 −16.96. SRTF improves the tail for all 3 strategies, meaning the mechanism acts as theory predicts. Seed 7 is the only case where Optimized p90 worsens; avg_total is still better than R10 by 0.28 minutes.

Live seed 99 (fresh dir, speed 5000→clamp 60): finished at `t = 399.74754874492817` minutes, 52/52, 188 visits; **78.1911 / 6.6557 / 150.9359 / 16.2417 / 11.0451** (avg_total/avg_wait/p90_total/p90_wait/avg_walk) vs R10 live 78.0142 / 6.6068 / 146.5977 / 16.4670 / 11.0451. Live is nearly neutral (avg +0.18, p90 +4.34, p90_wait −0.23, walk equal); SRTF's strength lies in the high-contention compare seeds.

**Tests — expectation change and new tests:**
- Among the 74 existing tests, **1 test changed expectation** — `urgent_patients_jump_ahead_of_normal_queue` (checks the physical queue order after `enqueue`) was replaced by `urgent_patients_are_dispatched_ahead_of_normal_queue` (checks the actual dispatch order: capacity 3 → ServiceStart order `[1, 3, 0]`, remaining `[2]`). Reason: the new policy intentionally keeps the queue in arrival order and applies priority at dispatch; keeping the old assertion would lock in behavior that was replaced. No other test was modified.
- **6 new tests:** (1) `srtf_dispatch_picks_shortest_remaining_tour_within_class` — same class, tour 8 beats tour 18; (2) `urgent_dispatch_beats_any_remaining_tour_length` — the longest urgent tour still goes before a short normal tour; (3) `equal_remaining_tours_dispatch_in_fifo_order` — equal tours keep arrival order; (4) `srtf_uses_learned_service_estimates` — one observation makes the ECG estimate exceed Blood Test and reverses the selected patient; (5) `srtf_never_preempts_an_in_service_patient` — in-service is untouched; the queue dispatches only after a slot frees; (6) `two_patient_srtf_dispatch_is_deterministic` — two worlds with the same seed/setup give the same dispatch order + queue state.
- `cargo test`: **80/80 pass** (74 existing + 6 new, 1 existing renamed/expectation changed).

**Verification steps:**
1. `cargo test`: 80/80 pass.
2. `npm run build`: OK (frontend unchanged; only the pre-existing chunk > 500 kB warning).
3. Fresh `MEDIROUTE_DATA_DIR=/tmp/mediroute-r13-test`, separate server `PORT=8082`; live `{strategy:optimized, speed:5000, seed:99}` → finished t = 399.7475 minutes, 52/52, 188 visits.
4. `POST /api/sim/compare` seed 42 → 7 → 123; recorded avg_total/avg_wait/p90_total/avg_walk for **all 3 strategies**; repeated seed 7 with bit-identical results (deterministic).
5. Port 8080 untouched; **no git commit/push**.

**Files changed:** `backend/src/engine/sim.rs` (enqueue + SRTF selection/dispatch + 6 new tests, 1 existing test expectation changed), `README.md` (serving policy description + test count), `docs/rounds/README.md`. Unchanged: `router.rs`, `congestion.rs`, `domain.rs`, `scenario.rs`, `stats.rs`, `learn.rs`, `api.rs`, `main.rs`, frontend.

**Decision:** **NO COMMIT as the round required.** On technical criteria: (a) met with caveat (mean + avg_total better; seed 7 p90 +4.92%), (b) met, (c) met close to the threshold. If this were a normal commit round, commit would be recommended because the mean improves all 3 main metrics and the tail-improvement mechanism is clear on high-contention seeds; however, seed 7 p90 has only 0.08 percentage points of margin below the 5% threshold, so independent verification is needed before deciding.

**Lessons learned:** (1) The per-decision router is saturated exactly as R12 concluded; **serving order is a real intervention point**: only changing dispatch order, without changing a single line of routing, cuts the tail strongly on seed 123 for all 3 strategies. (2) SRTF uses learned estimates instead of service samples, so it needs no RNG; it is deterministic and applicable to every strategy. (3) Priority and SRTF must combine at dispatch (not at enqueue) so urgent always wins, even when inserted mid-queue. (4) The router still models the queue as FIFO in `est_wait`, so SRTF creates a small lag between estimate and actual wait; no systemic harm observed, but this is a candidate for further tuning. (5) The 3-seed benchmark is still sensitive: the seed 7 p90 regression nearly touches the guardrail, repeating R12's lesson — more seeds are needed to distinguish mechanism improvement from cascade noise.

**Independent verification (coordinator) — extended to 5 seeds:** cargo test 80/80; repeated the standard process on R13 and R10 (worktree): matches the report on the 3 standard seeds (all round criteria (a)(b)(c) met). BUT on seeds 5/2024: seed 5 (most congested) p90_total 240.05→277.42 (**+15.6%, +37.4 minutes**) and Optimized loses #1 avg to ShortestQueue (116.29 vs 112.85). Mean 5 seeds: avg_total −0.88 but p90_total **+4.53**.

**Decision:** DISCARD — the goal is to reduce BOTH avg_total and p90; SRTF improves avg slightly but worsens mean p90 (exactly SRTF's starvation weakness: long-tour patients starve precisely when contention is high). The round's 3-seed criterion was met but does not generalize — 5-seed evidence (standard since R8) shows this is a trade-off in the wrong direction. Code reverted, cargo test 74/74.

**Lessons learned:** if scheduling is tried again, a starvation-resistant hybrid is needed (e.g., aging: effective_tour = tour / (1 + wait_so_far / k)) so SRTF does not starve long-tour patients — or SRTF restricted to short queues. R13's problem is not the serving-policy idea but the lack of a tail-protection mechanism.

---

## Round 14 — Aging-protected SRTF room scheduling (retry Round 13)

**Status:** ❌ DISCARDED (independent verification confirms: (a)(b) met but (c) failed — baselines benefit asymmetrically), not an implementation bug.

**Proposed aspect:**
- R13 (pure SRTF) won on the 3-seed mean but was discarded for starvation: seed 5 p90_total +15.6% (+37.4 minutes) and Optimized lost #1 avg_total to ShortestQueue.
- Round 14 keeps the intervention point **serving policy inside the room**, applies **the same policy to all 3 strategies** (hospital-wide policy), and adds **aging** so long-tour patients are not starved: `effective_tour = remaining_tour / (1 + wait_so_far / K)`.
- Router (`router.rs`, `congestion.rs`), API/JSON, frontend, learning, downtime, arrivals, priority class semantics **unchanged**. Because the serving policy changes, FixedOrder/ShortestQueue numbers **change vs R10 — this is expected**, not routing regression.
- Assess on **5 seeds** (42, 7, 123, 5, 2024) against 3 criteria: (a) 5-seed mean better than R10 on avg_total and p90_total; (b) no seed's p90_total worse than R10 by more than 5%; (c) Optimized still #1 avg_total + p90_total on all 5 seeds.

**Design:**
- **Enqueue:** always `push_back` — the physical queue keeps arrival order; priority is applied at dispatch, not at enqueue.
- **Dispatch (when a station is free):** (1) highest priority class present (urgent > normal, semantics unchanged); (2) smallest `effective_tour_min`; (3) earliest queue index (FIFO tie-break).
- **Remaining tour:** sum of **learned** `est_service_min` for `required_rooms` not yet `done` (including the room currently queued — a shared constant within the same queue, so it does not change order).
- **Wait so far:** `now − queue_joined_at` (when the patient reached the room and joined the queue).
- **No preemption:** in-service always runs to completion; only the next patient is chosen when a slot frees. Deterministic: scan `VecDeque` by index, strict `<` comparison, tie keeps the first index; no RNG added.
- **K chosen:** `AGING_K_MIN = 10.0` minutes (mandatory sweep 10/20/40; 10 best — see below). K → ∞ = pure SRTF (R13); smaller K means wait dominates more. Note: K → 0 does **not** approach absolute FIFO but approaches `wait / remaining_tour` order; this is why small K cannot recreate the R10 FIFO baseline.
- Implementation lives in `sim.rs`: `AGING_K_MIN`, `aging_k`, `remaining_tour_min`, `effective_tour_min`, `next_dispatch_index`, `dispatch_room`. `router.rs` and `congestion.rs` unchanged.

**K sweep** (each level fresh data dir → live seed 99 → compare 5 seeds; numbers are Optimized avg_total/p90_total):

| K | seed 42 | seed 7 | seed 123 | seed 5 | seed 2024 | Mean avg/p90 | (a) | (b) | (c) |
|---|---|---|---|---|---|---|---|---|---|
| R10 (ref) | 64.07 / 77.79 | 72.23 / 128.20 | 72.29 / 116.37 | 118.29 / 240.05 | 68.94 / 113.54 | 79.16 / 135.19 | — | — | — |
| **10 (chosen)** | **63.65 / 75.59** | **72.87 / 128.20** | **73.04 / 116.37** | **116.81 / 240.05** | **68.94 / 113.54** | **79.06 / 134.75** | ✅ | ✅ | ❌ 2 cells |
| 20 | 63.65 / 75.59 | 71.99 / 128.30 | 72.89 / 116.37 | 117.79 / 240.05 | 68.94 / 113.54 | 79.05 / 134.77 | ✅ | ✅ | ❌ 2 cells |
| 40 | 62.34 / 73.97 | 71.99 / 128.30 | 72.96 / 116.37 | 117.71 / **253.06** | 68.65 / 113.54 | 78.73 / **137.05** | ❌ p90 | ❌ seed 5 +5.42% | ❌ |
| 5 (diagnostic) | 63.65 / 75.59 | 72.87 / 128.20 | 72.07 / 116.37 | 117.33 / 240.05 | 68.94 / 113.54 | 78.97 / 134.75 | ✅ | ✅ | ❌ 2 cells |
| 1 (diagnostic) | 64.16 / 78.92 | 71.40 / 128.30 | 72.51 / 116.37 | 117.79 / 240.05 | 68.94 / 113.54 | 78.96 / **135.44** | ❌ p90 | ✅ | ❌ 2 cells |
| 0.1 (diagnostic) | 63.89 / 81.47 | 71.40 / 128.30 | 72.46 / 116.37 | 117.79 / 240.05 | 68.94 / 113.54 | 78.90 / **135.95** | ❌ p90 | ✅ | ❌ 2 cells |
| Guard T=20, K=10 (diagnostic) | 62.34 / 73.97 | 72.94 / 132.48 | 72.44 / 116.37 | 117.74 / **279.16** | 68.94 / 113.54 | 78.88 / **143.10** | ❌ p90 | ❌ seed 5 +16.3% | ❌ |

- **K=10 chosen** in the mandatory set: passes (a) with positive margin on both of the 2 metrics, passes (b) with the worst seed exactly equal to R10 (seed 5 p90 240.049 vs 240.05), and has the best (c) margin (seed 123 p90 only 1.31 minutes behind FixedOrder; seed 5 avg 3.60 minutes behind SQ). K=20 has a mean avg better by 0.01 minutes but visibly worse (c) (seed 123 p90 is #3, behind both SQ and FO).
- **K=40 hits guardrail (b)**: seed 5 p90 253.06 = **+5.42%** vs 240.05, and mean p90 137.05 is worse than R10 → fails (a). Aging at K=40 is still strong enough to pull seed 5 down from 277.42 (R13) to 253.06 but not enough to return to R10.
- **K=1 / K=0.1** cannot save (c) and even make mean p90 worse than R10 (+0.18% / +0.56%). Reason: K→0 gives `wait/remaining_tour` order, not FIFO; baseline numbers still do not return to R10.
- **Guard T=20** (aging starts only after 20 minutes of waiting) rescues seed 123 p90 (Optimized 116.37 < FixedOrder 128.03) but destroys seed 5 p90 (279.16, +16.3%) and mean p90 (+5.86%) → rejected. No threshold reconciles the two sides; see analysis (c).
- **K → ∞ = R13:** seed 123 p90 99.17 (would be #1) but seed 5 p90 277.42 (+15.6%) and seed 5 avg #2 — exactly the negative result already discarded.

**Results — compare at K=10** (standard verification: fresh `/tmp/mediroute-r14-test` → PORT=8082 → live seed 99 speed 5000 → compare 5 seeds; headless, same arrivals/packages/service samples):

Seed 42:

| Strategy | avg_total | avg_wait | p90_total | avg_walk |
|---|---|---|---|---|
| **Optimized** | **63.65** | **2.80** | **75.59** | **11.50** |
| FixedOrder | 77.48 | 6.04 | 132.94 | 13.45 |
| ShortestQueue | 70.28 | 4.07 | 119.07 | 13.48 |

Seed 7:

| Strategy | avg_total | avg_wait | p90_total | avg_walk |
|---|---|---|---|---|
| **Optimized** | **72.87** | **4.61** | **128.20** | **11.76** |
| FixedOrder | 82.42 | 6.57 | 140.68 | 13.82 |
| ShortestQueue | 79.59 | 5.90 | 138.89 | 13.54 |

Seed 123:

| Strategy | avg_total | avg_wait | p90_total | avg_walk |
|---|---|---|---|---|
| **Optimized** | **73.04** | **6.81** | 116.37 | **10.82** |
| FixedOrder | 84.69 | 9.66 | **115.06** | 12.73 |
| ShortestQueue | 80.83 | 8.56 | 118.76 | 12.64 |

Seed 5:

| Strategy | avg_total | avg_wait | p90_total | avg_walk |
|---|---|---|---|---|
| Optimized | 116.81 | 16.73 | **240.05** | **11.52** |
| FixedOrder | 118.69 | 16.86 | 346.30 | 12.93 |
| ShortestQueue | **113.21** | **15.33** | 309.73 | 13.09 |

Seed 2024:

| Strategy | avg_total | avg_wait | p90_total | avg_walk |
|---|---|---|---|---|
| **Optimized** | **68.94** | **3.79** | **113.54** | **11.75** |
| FixedOrder | 79.12 | 5.93 | 131.23 | 13.89 |
| ShortestQueue | 74.01 | 4.75 | 124.37 | 13.21 |

Baselines **no longer match R10** — expected because the serving policy changed. Direct R10 measurement (clean worktree at HEAD, same compare seeds; routing baselines do not read stats): seed 5 FixedOrder **124.15 / 18.34 / 240.27 / 12.93**, ShortestQueue **119.38 / 16.99 / 246.61 / 13.13**; seed 123 FixedOrder **86.44 / 10.17 / 145.23 / 12.73**, ShortestQueue **81.09 / 8.67 / 126.75 / 12.49**. The new serving policy helps SQ seed 5 avg by −6.17 minutes and FO seed 123 p90 by −30.17 minutes — exactly the source that makes criterion (c) fail.

**Assessment vs Round 10 (Optimized, mean 5 seeds):**

| Metric | R10 | R14 (K=10) | Δ |
|---|---|---|---|
| avg_total | 79.16 | **79.06** | **−0.10 (−0.13%)** |
| p90_total | 135.19 | **134.75** | **−0.44 (−0.33%)** |
| avg_wait | 6.57* | 5.95 | −0.62 |
| avg_walk | 11.51* | 11.47 | −0.04 |

*Mean from the 5 given references.

Per-seed Optimized vs R10:

| Seed | Δ avg_total | Δ p90_total | p90 vs R10 |
|---|---|---|---|
| 42 | −0.42 (−0.66%) | −2.19 (−2.82%) | −2.82% |
| 7 | +0.64 (+0.88%) | −0.00 | −0.00% |
| 123 | +0.75 (+1.03%) | −0.00 | −0.00% |
| 5 | −1.48 (−1.25%) | −0.00 | −0.00% |
| 2024 | −0.00 | −0.00 | −0.00% |

**Assessment by criterion:**
- **(a) Optimized 5-seed mean better than R10 on avg_total AND p90_total: ✅ MET.** 79.06 vs 79.16 (−0.13%); 134.75 vs 135.19 (−0.33%).
- **(b) No seed's p90_total worse than R10 by more than 5%: ✅ MET.** No seed is worse at all; seed 42 −2.82%, the others vary by ≤0.001% (seed 5 exactly 240.05). Compared with R13 (seed 5 +15.6%), aging fixed the starvation point correctly.
- **(c) Optimized still #1 avg_total + p90_total on all 5 seeds: ❌ NOT MET.** Met 8/10 cells:
  - seeds 42, 7, 2024: Optimized #1 on both avg_total and p90_total.
  - seed 123: Optimized #1 avg_total (73.04) but **p90_total #2** — 116.37 vs FixedOrder 115.06 (ShortestQueue 118.76).
  - seed 5: Optimized #1 p90_total (240.05) but **avg_total #2** — 116.81 vs ShortestQueue 113.21 (FixedOrder 118.69).
  - Considering the **5-seed mean** (not per-seed), Optimized is still absolutely #1 on both of the 2 metrics: avg 79.06 vs SQ 83.58 / FO 88.48; p90 134.75 vs SQ 162.16 / FO 173.24.

**Why (c) failed (honest):**
- R10 already beat seed 5 extremely narrowly: Optimized 118.29/240.05 vs ShortestQueue 119.38/246.61 — a margin of only 1.09 minutes avg and 0.22 minutes p90. The SRTF/aging serving policy helps **SQ much more** (avg 119.38 → 113.21, −6.17 minutes) than Optimized (118.29 → 116.81, −1.48 minutes), because myopic SQ routing leaves more long tours in queues and SRTF/aging clears them faster. Optimized's absolute gain is not enough to offset SQ's improvement → loses #1 even though Optimized's own numbers are better than R10.
- Seed 123 p90: the serving policy helps FixedOrder strongly (145.23 → 115.06) while aging keeps Optimized at 116.37. Only pure SRTF (K → ∞) brings Optimized down to 99.17 and wins #1 p90, but then seed 5 p90 jumps to 277.42 (+15.6%) and fails (b). This is the **trade-off boundary** between protecting seed 5's tail (needs aging) and the seed 123 p90 advantage (needs pure SRTF) — no K in the tried range satisfies both.
- A guard threshold does not solve it either: T=20 rescues seed 123 p90 but seed 5 p90 279.16 (+16.3%, fails (b)); T=0 (no guard) fails (c). Small K (1/0.1) does not bring baselines back to FIFO either because the K→0 limit is `wait/tour`, not FIFO.
- Structural conclusion: when applying **the same serving policy to all 3 strategies**, criterion (c) conflicts with fixing starvation on seed 5; serving-only cannot both improve Optimized's tail and preserve its relative advantage over baselines at exactly R10's very thin marginal seeds.

Live seed 99 (fresh dir, speed 5000): finished at `t = 399.74754874492817` minutes, 52/52, 188 visits; **78.1911 / 6.6557 / 150.9359 / 16.4670 / 11.0451** (avg_total/avg_wait/p90_total/p90_wait/avg_walk) — exactly matching R10/R13 live, confirming the policy does not change order on this seed; the learned stats used for compare are the same state. Repeating compare seeds 42/7/123/5/2024 on two independent runs gives **bit-identical** results.

**Tests:**
- `cargo test`: **80/80 pass** (74 existing + 6 new; **1 existing test renamed/expectation changed**).
- Changed existing test: `urgent_patients_jump_ahead_of_normal_queue` → `urgent_patients_are_dispatched_ahead_of_normal_queue` — checks the **actual dispatch order** (`[1, 3, 0]`, remaining `[2]`) instead of physical queue order, because the new policy intentionally keeps the queue in arrival order and applies priority at dispatch (same as R13). No other existing test was modified.
- 6 new tests: (1) `aging_prioritizes_the_patient_who_waited_longer` — same tour, the longer-waiting patient is chosen; (2) `large_aging_k_reproduces_srtf_order` — very large K, short tour wins even if just arrived; (3) `small_aging_k_lets_a_long_waiting_long_tour_go_first` — small K, earlier long-tour wins; (4) `urgent_dispatch_beats_any_aging_discount` — urgent still before normal despite a strong aging discount for normal; (5) `aging_never_preempts_an_in_service_patient` — in-service is not interrupted; (6) `aging_dispatch_order_is_deterministic` — two worlds with the same seed/setup give the same dispatch order + queue state.

**Files changed:** `backend/src/engine/sim.rs` (`AGING_K_MIN = 10`, `aging_k`, `remaining_tour_min`, `effective_tour_min`, `next_dispatch_index`, new enqueue/dispatch, 6 new tests, 1 existing test expectation changed), `README.md` (serving policy description + test count 80), `docs/rounds/README.md`. Unchanged: `router.rs`, `congestion.rs`, `domain.rs`, `scenario.rs`, `stats.rs`, `learn.rs`, `api.rs`, `main.rs`, frontend.

**Verification steps:**
1. `cargo test`: 80/80 pass; `npm run build`: OK (frontend unchanged; only the pre-existing chunk > 500 kB warning).
2. Fresh `MEDIROUTE_DATA_DIR=/tmp/mediroute-r14-test`, separate server `PORT=8082`; live `{strategy:optimized, speed:5000, seed:99}` → finished 399.7475 minutes, 52/52, 188 visits.
3. `POST /api/sim/compare` seed 42 → 7 → 123 → 5 → 2024; recorded avg_total/avg_wait/p90_total/avg_walk for **all 3 strategies**.
4. Sweep K = 10/20/40, each level fresh data dir + live seed 99 + compare 5 seeds; additional diagnostics K=5/1/0.1 and guard threshold T=20. Measured R10 baselines for seeds 5/123 in a clean worktree at HEAD.
5. Port 8080 untouched; **no git commit/push**.

**Decision:** **NO COMMIT.** Round 14 fixed R13's starvation bug correctly: seed 5 p90 back to exactly the R10 level, no seed violates the guardrail, 5-seed mean beats R10 on both avg_total and p90_total. But criterion (c) fails because the serving policy helps baselines more on the two marginal seeds (seed 5 avg to ShortestQueue, seed 123 p90 to FixedOrder), and no K/guard reconciles it. Code remains in the working tree with K=10 for the coordinator's independent verification; honest report, no commit.

**Lessons learned:** (1) Aging solves starvation correctly (R13 seed 5 +15.6% → R14 0.00%) while retaining the tail benefit on seed 42 (−2.82%) — the `remaining / (1 + wait/K)` mechanism is the right direction. (2) But the "Optimized still #1 per-seed" criterion does not depend on Optimized alone: when the serving policy changes for every strategy, baselines benefit too, and on seed 5 R10 was only 1.09 minutes avg ahead of SQ — any SRTF-like reordering helps SQ more. (3) There is a real trade-off boundary between the seed 5 tail (needs aging) and seed 123 p90 (needs pure SRTF); achieving both requires a different intervention (e.g., a separate policy for capacity-1 vs capacity-3 rooms, or a routing change so Optimized benefits asymmetrically), not just tuning K. (4) K→0 in the formula is not FIFO — the limit is `wait/tour`; controlling FIFO requires a separate policy branch. (5) R12's lesson is confirmed: per-seed benchmarks are very sensitive; 5 seeds and a per-seed #1 check are needed, not only the mean.

**Independent verification (coordinator):** cargo test 80/80; repeated the standard process (fresh dir → live seed 99 → compare seeds 5 and 123): matches exactly — seed 5 Optimized 116.81 loses to ShortestQueue 113.21 (avg), seed 123 Optimized p90 116.37 loses to FixedOrder 115.06. Code reverted, cargo test 74/74.

**Decision:** DISCARD — the round's criterion (c) (Optimized #1 on all 5 seeds) fails in 2 cells. Interesting structural cause: a good serving policy helps EVERY strategy, but helps baselines MORE than Optimized (SQ −6.17 vs Optimized −1.48 on seed 5 avg) because Optimized already balances load via routing — the remaining scheduling benefit is smaller for the already-smart strategy.

**R13-R14 series conclusion:** serving policy (SRTF/aging) is a dead end for the goal "Optimized better than baselines" — every serving improvement is shared by all 3 strategies and the distinguishing margin shrinks. This direction only has value if the goal changes to "absolute improvement for the whole hospital" (in which case R14 K=10 is a win: both mean avg and p90 better, no seed regression). The patch can be reused from the docs.

---

## Round 15 — Dependency-bundle completion tie-break (Optimized only)

**Status:** ❌ DISCARDED (absolutely neutral — bit-identical to R10, the tweak does not activate in the DAG demo); did not meet the "better" criterion, so no commit.

**Proposed aspect:**
- The `unblocks` tie-break from R1 counts only the number of directly dependent rooms still required. A room still scores even if its dependent is missing another prerequisite — choosing that room does not yet unlock anyone's eligibility.
- Round 15 replaces **the formula of the `unblocks` aspect itself** with a **bundle-completion score**: room `r` gets a bonus if `r` is the LAST missing prerequisite of ≥1 other required room for the patient, i.e., after completing `r`, the other room becomes eligible immediately. Principle: scattered prerequisites keep the patient waiting longer; finishing an in-progress bundle early unlocks routing freedom sooner.
- Tie-break position unchanged: cost band 25% → walk → downtime risk → **bundle completion (replaces unblocks)** → downstream depth → en-route → queue → package position. No cost term added, no band change, predictive wait/lookahead/downtime/sim/baselines/API/frontend/learning unchanged.

**Formula:**

```
missing_before(d) = { p ∈ prereq(d) : p ∈ required(patient) ∧ p ∉ done(patient) }
bundle_completion_score(r) =
    |{ d : d ∈ required(patient) \ done(patient), d ≠ r,
         r ∈ prereq(d), missing_before(d) = {r} }|
```

- Compared in descending order; ties fall through to downstream depth as before.
- Differs from old `unblocks` in exactly 1 place: a dependent with ≥2 unfinished prerequisites no longer counts for any prerequisite. With single-prerequisite dependencies, the score equals `unblocks`.

**Results — compare** (standard verification: fresh `MEDIROUTE_DATA_DIR=/tmp/mediroute-r15-test` → `PORT=8082` → live seed 99 speed 5000 → compare 5 seeds; headless, same arrivals/packages/service samples):

Seed 42:

| Strategy | avg_total | avg_wait | p90_total | avg_walk |
|---|---|---|---|---|
| **Optimized** | **64.0700** | **2.9150** | **77.7868** | **11.5110** |
| FixedOrder | 77.0249 | 5.9134 | 130.2068 | 13.4523 |
| ShortestQueue | 71.3185 | 4.3273 | 103.8948 | 13.5720 |

Seed 7:

| Strategy | avg_total | avg_wait | p90_total | avg_walk |
|---|---|---|---|---|
| **Optimized** | **72.2299** | **4.4230** | **128.1961** | **11.8492** |
| FixedOrder | 84.3898 | 7.0846 | 138.8932 | 13.8233 |
| ShortestQueue | 77.8194 | 5.5577 | 139.7541 | 13.0962 |

Seed 123:

| Strategy | avg_total | avg_wait | p90_total | avg_walk |
|---|---|---|---|---|
| **Optimized** | **72.2939** | **6.5969** | **116.3665** | **10.8056** |
| FixedOrder | 86.4409 | 10.1679 | 145.2265 | 12.7288 |
| ShortestQueue | 81.0868 | 8.6726 | 126.7505 | 12.4931 |

Seed 5:

| Strategy | avg_total | avg_wait | p90_total | avg_walk |
|---|---|---|---|---|
| **Optimized** | **118.2913** | **17.1310** | **240.0492** | **11.5275** |
| FixedOrder | 124.1452 | 18.3369 | 240.2734 | 12.9288 |
| ShortestQueue | 119.3817 | 16.9931 | 246.6130 | 13.1272 |

Seed 2024:

| Strategy | avg_total | avg_wait | p90_total | avg_walk |
|---|---|---|---|---|
| **Optimized** | **68.9360** | **3.7923** | **113.5368** | **11.7520** |
| FixedOrder | 79.4512 | 6.0142 | 144.2094 | 13.8921 |
| ShortestQueue | 74.2294 | 4.8259 | 124.3680 | 13.1494 |

Mean 5 seeds (Optimized):

| Metric | R10 | R15 | Δ |
|---|---|---|---|
| avg_total | 79.1642 | **79.1642** | 0 |
| avg_wait | 6.9716 | **6.9716** | 0 |
| p90_total | 135.1871 | **135.1871** | 0 |
| avg_walk | 11.4891 | **11.4891** | 0 |

- All results are **bit-identical to R10**, not just Optimized: baselines match number-for-number (serving policy still FIFO), and live seed 99 also matches exactly, so the learned stats used for compare are the same state.
- Live seed 99 (fresh dir, speed 5000→clamp 60): finished at `t = 399.74754874492817` minutes, 52/52, 188 visits; **78.0142 / 6.6068 / 146.5977 / 16.4670 / 11.0451** (avg_total/avg_wait/p90_total/p90_wait/avg_walk) — identical to R10.

**Assessment by criterion:**
- **(a) 5-seed mean better than R10 on avg_total AND p90_total: ❌ NOT MET.** Exactly equal (79.1642 vs 79.1642; 135.1871 vs 135.1871) — "better" in the strict sense did not happen.
- **(b) No seed's p90_total worse than R10 by more than 5%: ✅ MET.** Δ = 0 on all 5 seeds.
- **(c) Optimized still #1 on every seed and every main metric: ✅ MET.** avg_total/p90_total respectively: s42 64.07/77.79; s7 72.23/128.20; s123 72.29/116.37; s5 118.29/240.05 (close: 1.09 ahead of ShortestQueue on avg, 0.22 ahead of FixedOrder on p90); s2024 68.94/113.54.
- Baselines exactly match R10 (FIFO serving), confirming the change is confined to Optimized and does not touch the sim.

**Why it is neutral (analysis):**
- In the demo DAG, **Cardiology is the only room with 2 prerequisites** (Blood Test, ECG). The bundle score differs from `unblocks` only when both Blood and ECG are unfinished: then both lose 1 point (old 1/1 → new 0/0), so the relative Blood–ECG ranking is unchanged.
- Compared with independent depth-0 rooms (ENT, Eye Exam, ...) in walk pairs clamped equal, **downstream depth** — the key immediately after the bundle — still ranks Blood/ECG above them because they have a Cardiology chain. The remaining depth-1 room is X-Ray, which never ties walk with Blood/ECG at the 10 possible positions (entrance + 9 rooms), so there is no pair where the bundle score could flip the result.
- In other words, in the current workload the old `unblocks` and the bundle score produce the same order in every comparison that actually reaches this key; synthetic tests confirm the new mechanism works correctly, but the demo scenario has no asymmetric bundle to exploit.
- Live seed 99 bit-identical further confirms: routing decisions on the live path do not change, so the learned stats for compare do not change either.

**Tests:**
- `cargo test`: **79/79 pass** (74 existing + 5 new). **No existing test had to change expectation**; the two old tie-break tests (`optimized_tie_break_unblocks_dependent_rooms`, `tie_break_prefers_deeper_dependency_chain_within_cost_band`) still pass unchanged because their behavior does not depend on the replaced part.
- 5 new tests: (1) `bundle_completion_prefers_the_last_missing_prerequisite_within_the_band` — ECG is Cardiology's last missing prerequisite and beats ENT with the same cost/walk; (2) `prerequisite_with_another_missing_prerequisite_earns_no_bundle_bonus` — Blood/ECG are both missing because Cardiology needs both → score 0, X-Ray (Orthopedics' last prerequisite) wins; (3) `bundle_bonus_does_not_override_cost_outside_the_band` — unlock has a bundle bonus but service 40 vs 10, cost outside the band so it loses; (4) `baselines_ignore_bundle_completion` — FixedOrder/ShortestQueue still choose Blood; (5) `single_room_patient_is_unaffected_by_bundle_scoring` — dependencies do not apply when the other room is not required; only-room remains the only choice.

**Files changed:** `backend/src/engine/router.rs` (replaced `unblocks` with `bundle_completion_score`, used at the same tie-break position; 5 new tests), `docs/rounds/README.md` (this section). Unchanged: `congestion.rs`, `sim.rs`, `domain.rs`, `scenario.rs`, `stats.rs`, `learn.rs`, `api.rs`, `main.rs`, frontend, top-level README.

**Verification steps:**
1. `cargo test`: 79/79 pass. `npm run build`: OK (frontend unchanged; only the pre-existing chunk > 500 kB warning).
2. Fresh `MEDIROUTE_DATA_DIR=/tmp/mediroute-r15-test`, separate server `PORT=8082`; live `{strategy:optimized, speed:5000, seed:99}` → finished t = 399.7475 minutes, 52/52, 188 visits, metrics matching R10.
3. `POST /api/sim/compare` seed 42 → 7 → 123 → 5 → 2024; recorded avg_total/avg_wait/p90_total/avg_walk for all 3 strategies; compared every number with R10 (baseline measured at HEAD on the same process before the change): 100% match.
4. Port 8080 untouched; **no git commit/push**.

**Decision:** **NO COMMIT** — no measurable improvement, and no regression either. The working tree retains code + docs for the coordinator's independent verification.

**Lessons learned:** bundle completion is a genuine refinement of `unblocks`, but its signal is **symmetrized** in the demo DAG: Cardiology has exactly 2 prerequisites, so the two candidates always lose points equally, and against independent rooms downstream depth already says the same thing. Making the bundle score matter requires asymmetric bundles (≥2 branches with different depth/cost, or multiple dependents sharing a prerequisite), which the current scenario does not have. This is a clean negative result: correct mechanism, absent use case.

**Independent verification (coordinator):** cargo test 79/79 (before revert), 74/74 (after revert). The agent presented complete bit-identical evidence (15 pairs × 4 metrics + live run) and a convincing root cause: Cardiology is the only room with 2 prerequisites; when both of the 2 are unfinished, the new and old scores are both 0 — there is no asymmetric bundle in the demo DAG to exploit.

**Decision:** DISCARD — neutral is not "better". The idea only has value if the DAG has multiple asymmetric multi-prerequisite rooms (e.g., a room requiring 2 separate labs + 1 imaging); in the current demo, no such case exists.

**15-round lesson:** per-decision router (R2, R12, R15 fail), serving policy (R13, R14 fail for the comparison objective), learning (R9 fail) have all been tried. Success lies in: more accurate physical models (R3 distances), controlled lookahead (R4), future congestion modeling (R8), correct model details (R10). The remaining direction, if continued: a structurally richer scenario (deeper DAG, more multi-prerequisite rooms) and re-measure — but that changes the benchmark, not the algorithm.

---

## Round 16 — Variance-aware routing (mean-variance cost, Optimized only)

**Status:** ✅ COMMITTED (verified independently — mean p90 −1.34%, mean avg +0.016%, Optimized #1 10/10) and p90_total on 5/5 seeds) — **NO COMMIT** per the round constraint. There is a protocol-sensitive caveat: live seed 99 is worse and the cold-stats control does not retain the p90 benefit (reported honestly below).

**Proposed aspect:**
- R10 uses the risk-neutral cost `walk + est_wait + est_service + 0.5·lookahead`; `est_wait` is the expectation and does not distinguish two rooms with the same mean but different variance. The p90 tail is dominated by long service chains at rooms with few slots: capacity 1 with 3 waiting has accumulated variance; capacity 3 with 3 waiting has 3 independent slots.
- Round 16 adds a mean-variance term **only for Optimized**: `cost_risk = cost_R10 + KAPPA · σ_wait`, where σ_wait includes service dispersion, queue drain, and arrival λ over the horizon. Unchanged: sim, serving policy/FIFO, predictive wait, lookahead, 25% tie-band, downtime, arrivals, priority, learning, API/JSON, frontend, baselines.

**σ_wait formula (`congestion.rs::wait_sigma_min`):**

```text
σ_service = 0.11547 · est_service        // service ~ Uniform(0.8μ, 1.2μ): 0.4μ/√12
n = in_service + queue_len + pending_starts + en_route
    + ceil(Σ weight of likely_arrivals with ready_in_min ≤ h)
Var_queue = 0                            if n = 0
          = σ_service²                   if 0 < n < c
          = n · σ_service² / c           if n ≥ c
λ_h = E_r(now, now + h)                  // ArrivalDemand::expected_arrivals
Var_arrival = λ_h · σ_service² / c       // Poisson count variance → wait variance
σ_wait = √(Var_queue + Var_arrival)
h = walk(now → room) + wait_at(room)     // horizon until the patient is expected to start service
```

- `est_service` is the learned EWMA mean; `RoomStats` does not store variance/abs-dev (reverted in R9), so σ_service comes from the simulator's known Uniform distribution. If `n < c`, the wait contains only one service draw, so Var = σ_service².
- `n` counts **every job ahead**, including in-service (each in-service patient is a chain entry), queue, pending start, en-route, and likely in-system arrivals. This reading matches the literal "n jobs over c independent slots" and was chosen via model sweep (see the variant table). In-service remaining is known, so this is a conservative spread — dropping in-service was tried and the result did not meet the per-seed criteria.
- λ arrivals use horizon `h` from now until the patient is expected to start service. λ=0 (lull 120–150, after 210) → no arrival variance.

**Router integration (`router.rs`):**
- `RISK_KAPPA = 0.5`. `mean_cost` keeps the R10 formula; `cost_risk(room, κ) = mean_cost + κ·σ_wait`; `total_cost` uses κ = RISK_KAPPA.
- KAPPA = 0 reproduces R10 **bit-identically** — fully verified on 5 seeds + live seed 99, not just unit tests.
- Tie-band and the order walk → downtime risk → unblocks → downstream depth → en-route → queue → package position unchanged. Baselines do not read cost/σ, so they are unchanged.

**Model variants (diagnostic at KAPPA = 0.5, same standard verification):**

| Variant | mean avg | mean p90 | Note |
|---|---|---|---|
| **n includes in-service + likely, h = walk+wait (chosen)** | **79.1771** | **133.3764** | #1 on both of the 2 metrics on 5/5 seeds |
| n drops in-service (keeps likely) | 79.4567 | 133.3764 | seed 5 avg #2: 120.37 vs SQ 119.38 |
| drop likely from n (keep in-service) | 80.1173 | 136.6280 | fails mean avg (+1.20%) and mean p90 (+1.07%) |
| h = walk only (drop in-service) | 80.5490 | 136.9988 | fails both means (+1.75% / +1.34%) |

- Counting in-service is the decisive difference: it raises σ for busy rooms, helping seed 5 keep #1 avg while retaining the tail gain. Dropping in-service leaves only a near-win (seed 5 avg 120.37 > SQ 119.38).
- The remaining variants confirm: dropping likely or shortening the horizon to walk loses the p90 benefit.

**KAPPA sweep (final model; each level fresh data dir → live seed 99 → compare 42/7/123/5/2024):**

| KAPPA | s42 avg/p90 | s7 avg/p90 | s123 avg/p90 | s5 avg/p90 | s2024 avg/p90 | Mean avg | Mean p90 | (a) mean | (b) #1/seed |
|---|---|---|---|---|---|---|---|---|---|
| 0 (control) | 64.07 / 77.79 | 72.23 / 128.20 | 72.29 / 116.37 | 118.29 / 240.05 | 68.94 / 113.54 | 79.1642 | 135.1871 | ref | 10/10 |
| **0.5 (chosen)** | **64.67 / 78.41** | **72.23 / 128.20** | **72.16 / 116.37** | **118.61 / 228.61** | **68.22 / 115.30** | **79.1771** | **133.3764** | ✅ | **10/10** |
| 1.0 | 64.67 / 78.41 | 72.23 / 128.20 | 76.68 / 129.34 | 118.98 / 228.61 | 67.77 / 115.30 | 80.0675 | 135.9708 | ❌ avg +1.14%, p90 +0.58% | 8/10 (s123 p90 #3) |
| 1.5 | 64.26 / 81.26 | 74.95 / 128.20 | 82.75 / 144.21 | 118.82 / 211.63 | 70.34 / 125.04 | 82.2250 | 138.0675 | ❌ avg +3.87%, p90 +2.13% | ❌ |
| 2.0 | 63.90 / 92.01 | 74.95 / 128.20 | 82.75 / 144.21 | 118.82 / 211.63 | 70.34 / 125.04 | 82.1521 | 140.2189 | ❌ avg +3.77%, p90 +3.72% | ❌ |

- 0.5 is the only point better than R10 on mean p90 while keeping mean avg within +1%. 1.0 starts losing both means and drops seed 123 p90 behind ShortestQueue; 1.5/2.0 change paths strongly, pushing seed 42 p90 to 81–92 and seed 123 to 144.21.
- The K=0 control matches R10 number-for-number on all 5 seeds and live seed 99 (see verification steps), confirming `cost_risk(κ=0)` does not change any decision.

**Results — compare at KAPPA = 0.5** (standard verification: fresh `/tmp/mediroute-r16-test` → `PORT=8082` → live seed 99 speed 5000 → compare 5 seeds):

| Seed | Strategy | avg_total | avg_wait | p90_total | avg_walk |
|---|---|---|---|---|---|
| 42 | **Optimized** | **64.6739** | **3.0465** | **78.4098** | **11.6320** |
| 42 | Fixed Order | 77.0249 | 5.9134 | 130.2068 | 13.4523 |
| 42 | Shortest Queue | 71.3185 | 4.3273 | 103.8948 | 13.5720 |
| 7 | **Optimized** | **72.2299** | **4.4230** | **128.1961** | **11.8492** |
| 7 | Fixed Order | 84.3898 | 7.0846 | 138.8932 | 13.8233 |
| 7 | Shortest Queue | 77.8194 | 5.5577 | 139.7541 | 13.0962 |
| 123 | **Optimized** | **72.1606** | **6.5564** | **116.3665** | **10.8109** |
| 123 | Fixed Order | 86.4409 | 10.1679 | 145.2265 | 12.7288 |
| 123 | Shortest Queue | 81.0868 | 8.6726 | 126.7505 | 12.4931 |
| 5 | **Optimized** | **118.6053** | **17.1754** | **228.6066** | **11.6777** |
| 5 | Fixed Order | 124.1452 | 18.3369 | 240.2734 | 12.9288 |
| 5 | Shortest Queue | 119.3817 | 16.9931 | 246.6130 | 13.1272 |
| 2024 | **Optimized** | **68.2158** | **3.6053** | **115.3030** | **11.7367** |
| 2024 | Fixed Order | 79.4512 | 6.0142 | 144.2094 | 13.8921 |
| 2024 | Shortest Queue | 74.2294 | 4.8259 | 124.3680 | 13.1494 |

- Baselines match **number-for-number** with R10 on all 15 cells × 4 metrics, confirming the serving policy is still FIFO and only Optimized's cost changed.
- Mean 5 seeds (Optimized): avg_total 79.1771, avg_wait 6.9613, p90_total 133.3764, avg_walk 11.5413; FixedOrder 90.2904 / 9.5034 / 159.7619 / 13.3650; ShortestQueue 84.7672 / 8.0753 / 148.2761 / 13.0876.

**Assessment vs Round 10 (Optimized):**

| Seed | Δ avg_total | Δ avg_wait | Δ p90_total | Δ avg_walk |
|---|---|---|---|---|
| 42 | +0.6039 (+0.94%) | +0.1315 (+4.51%) | +0.6230 (+0.80%) | +0.1210 (+1.05%) |
| 7 | 0 | 0 | 0 | 0 |
| 123 | −0.1333 (−0.18%) | −0.0405 (−0.61%) | 0 | +0.0053 (+0.05%) |
| 5 | +0.3140 (+0.27%) | +0.0444 (+0.26%) | **−11.4426 (−4.77%)** | +0.1502 (+1.30%) |
| 2024 | −0.7202 (−1.04%) | −0.1870 (−4.93%) | +1.7662 (+1.56%) | −0.0153 (−0.13%) |
| **Mean 5 seeds** | **+0.0129 (+0.016%)** | **−0.0103 (−0.148%)** | **−1.8107 (−1.339%)** | **+0.0522 (+0.455%)** |

**Assessment by criterion:**
- **(a) Mean p90_total over 5 seeds better than R10: ✅ MET.** 133.3764 vs 135.1871 (−1.8107, −1.34%).
- **(b) Mean avg_total not worse by more than 1%: ✅ MET.** 79.1771 vs 79.1642 (+0.0129, +0.016%).
- **(c) Optimized still #1 on every seed on the main metrics: ✅ MET 10/10 cells.**
  - avg_total: s42 64.67 < 71.32 (SQ); s7 72.23 < 77.82; s123 72.16 < 81.09; s5 **118.61 < 119.38 (SQ, margin 0.78 minutes)**; s2024 68.22 < 74.23.
  - p90_total: s42 78.41 < 103.89; s7 128.20 < 139.75; s123 116.37 < 126.75; s5 228.61 < 240.27 (FO, margin 11.67); s2024 115.30 < 124.37.
- The benefit is concentrated in seed 5 — the highest-contention seed: p90 −11.44 minutes while avg is still #1. Other seeds are near neutral; seed 42 rises slightly (+0.60 avg, +0.62 p90), seed 2024 rises p90 +1.77 but drops avg 0.72.

**Honest caveat — protocol sensitivity:**
- **Live seed 99 (fresh dir, speed 5000):** finished at `t = 408.14508086711777` minutes, 52/52, 188 visits; **83.4505 / 8.0682 / 157.1367 / 20.5701 / 11.1977** vs R10 **78.0142 / 6.6068 / 146.5977 / 16.4670 / 11.0451** — avg +5.44, p90 +10.54. R16 routing makes live seed 99 clearly worse even though the 5-seed compare wins. Because compare uses the learned stats of the live run, this is also part of the compare result.
- **Cold-stats control (fresh dir, compare directly without live; both use the initial seed stats):**
  - K=0 (R10): s42 64.07/77.79, s7 73.09/128.65, s123 82.37/144.21, s5 121.22/230.52, s2024 68.48/113.54 → mean **79.8480 / 138.9397**.
  - K=0.5: s42 64.50/83.43, s7 72.46/128.30, s123 82.75/144.21, s5 119.57/228.61, s2024 68.22/115.30 → mean **79.4999 / 139.9697**.
  - Routing-only: mean avg −0.35 but mean p90 **+1.03** — the p90 benefit disappears when stats are identical. Conclusion: the win on the standard protocol includes the contribution of the live seed 99 path/learned-stats change, not purely the variance-aware cost. Coordinator independent verification is needed before considering it commit-worthy.

**Tests:**
- `cargo test`: **81/81 pass** (74 existing + 7 new; no existing test changed expectation).
- 4 congestion tests: `wait_sigma_shrinks_with_parallel_capacity` (cap 3 < cap 1 at the same n = 3, exactly √3·σ_service vs σ_service), `wait_sigma_grows_with_queued_jobs` (n = 0..4 increases monotonically), `wait_sigma_grows_with_arrival_rate` (doubling λ → σ increases; lull → 0), `wait_sigma_counts_in_service_jobs` (in-service is a chain entry; empty room + no demand → 0).
- 3 router tests: `zero_kappa_cost_matches_the_round_ten_formula` (κ=0 matches the R10 formula exactly at f64 precision in a context with demand/non-zero σ), `cost_risk_prefers_lower_variance_when_means_are_equal` (equal mean wait but different chain variance → cost_risk ranks correctly; κ=0 equal), `risk_term_does_not_change_baseline_choices` (FixedOrder/ShortestQueue still choose as before despite variance differences).

**Files changed:** `backend/src/engine/congestion.rs` (`SERVICE_STD_FRACTION`, `wait_sigma_min`; 4 tests), `backend/src/engine/router.rs` (`RISK_KAPPA = 0.5`, `wait_sigma`, `mean_cost`, `cost_risk`, `total_cost` wrapper; 3 tests), `docs/rounds/README.md`. Unchanged: `sim.rs`, `api.rs`, `main.rs`, `domain.rs`, `scenario.rs`, `stats.rs`, `learn.rs`, frontend, baselines, serving policy, tie-band, lookahead.

**Verification steps:**
1. `cargo test`: **81/81 pass**.
2. `npm run build`: OK (frontend unchanged; only the pre-existing chunk > 500 kB warning).
3. Fresh `MEDIROUTE_DATA_DIR=/tmp/mediroute-r16-test`, separate server `PORT=8082`; live `{strategy:optimized, speed:5000, seed:99}` → finished 408.145 minutes, 52/52, 188 visits, metrics recorded above.
4. `POST /api/sim/compare` seed 42 → 7 → 123 → 5 → 2024; recorded avg_total/avg_wait/p90_total/avg_walk for all 3 strategies; baselines match R10 number-for-number.
5. Sweep KAPPA 0.5/1.0/1.5/2.0, each level fresh data dir + live seed 99 + compare 5 seeds; K=0 control run through the same process and bit-identical to R10 (live + 15 compare cells).
6. Model variants: n without in-service, without likely, h=walk; cold-stats control (compare without live) for K=0 and K=0.5.
7. Port 8080 untouched; **no git commit/push**.

**Decision:** **MET the round's technical criteria on the standard verification** (mean p90 better, mean avg within +1%, Optimized #1 10/10 cells), but **NO COMMIT** per the round constraint and because the result is protocol-sensitive: live seed 99 worse by +10.54 p90, cold-stats control loses the p90 benefit. The working tree keeps KAPPA = 0.5 plus docs for the coordinator's independent verification.

**Lessons learned:** (1) Mean-variance works as theory predicts on the highest-contention seed (s5 p90 −11.44) but is a weak signal: KAPPA 1.0+ immediately over-penalizes and worsens the s123/2024 paths; only 0.5 is a balance point. (2) How `n` is counted decides the result: including in-service is required to achieve per-seed #1, dropping it leaves only a near-win; dropping likely or shortening the horizon loses the tail gain. (3) The methodological lesson matters more: the "live seed 99 → compare" benchmark mixes routing with learned-stats drift; the cold-stats control shows the p90 benefit is not purely due to the new cost. To conclude robustly, either fix the stats snapshot during compare or add more seeds/cold controls; otherwise a change that worsens the live path can still "win" thanks to different stats. (4) K=0 bit-identical is a mandatory control and was confirmed on all 5 seeds + live, so every measured difference is attributable to the σ term, not the refactor.


**Independent verification (coordinator):** cargo test 81/81; repeated the standard process (fresh dir → live seed 99 → compare 5 seeds): matches every number exactly (mean avg 79.18, mean p90 133.38). Vs R10: mean p90 −1.81 minutes (−1.34%), mean avg +0.01 (neutral), seed 5 p90 −11.44, Optimized #1 on all 5 seeds × 2 main metrics.

**Decision:** COMMIT — meets all criteria (mean p90 wins, avg neutral, #1 across the board). The caveat is acknowledged from the agent's report: part of the p90 gain comes through learned-stats drift on the live path (cold control p90 moves +1.03) — the direct κ·σ mechanism helps the most congested seed (−11.4 p90) and is a real signal; the aggregate gain is a combination of routing + learning interaction, as expected in a closed-loop system.

## Round 17 — Post-peak lull exploitation (Optimized only)

**Status:** ✅ COMMITTED (verified independently by the coordinator; mean p90 −2.55%, mean avg +0.914% within the 1% threshold, Optimized #1 10/10 cells). Caveat: the p90 gain comes from seed 5 (−16.98); the other 4 seeds keep R16 p90 exactly, mean avg is close to the +1% threshold; live seed 99 bit-identical to R16.

**Proposed aspect:**
- Arrivals have rush 0–120 (65%), lull 120–150 (λ=0), secondary rush 150–210 (35%). R8 future_wait already used λ over the horizon, and R10 already priced downtime at ETA, but first-step wait was still `wait_at` = "wait counted from now", while actual wait is measured from when the patient **reaches the room**: walk time is counted twice (once in cost, once in expected wait). This bias is largest around peak→lull because the queue drains while the patient is walking.
- Round 17 changes only how **first-step wait** is priced in Optimized's `mean_cost` in the post-peak window: use demand-aware wait at ETA and allow only reductions, never increases. Unchanged: baselines, sim physics, serving/FIFO, R8 lookahead, 25% tie-band, R16 σ term, R10 downtime, routing window, learning, API/JSON, frontend.

**Measuring the bias before fixing** (temporary instrumentation in `sim.rs`, fully reverted; routing decisions in `[100,160)`):

| Seed | Decisions | n in window | Pred mean (wait_at) | Actual mean | Bias | MAE | Bias/MAE demand@ETA | Bias/MAE min(pred,demand) |
|---|---|---|---|---|---|---|---|---|
| 42 | 191 | 42 | 3.91 | 2.07 | +1.84 | 1.88 | +1.92 / 2.01 | +0.82 / 0.90 |
| 7 | 199 | 46 | 14.15 | 6.20 | +7.96 | 8.07 | +3.19 / 3.39 | +2.56 / 2.85 |
| 123 | 178 | 40 | 15.35 | 7.88 | +7.47 | 8.08 | +2.43 / 3.96 | +1.98 / 3.61 |

| Sub-window | s42 pred / demand / min bias | s7 pred / demand / min bias | s123 pred / demand / min bias |
|---|---|---|---|
| Peak-edge `[100,120)` | +0.49 / +2.72 / +0.48 | +8.70 / +4.23 / +3.39 | +7.98 / +4.83 / +4.24 |
| Lull `[120,150)` | +3.85 / +1.46 / +1.19 (MAE 1.27) | +9.38 / +1.98 / +1.64 (MAE 2.13) | +8.99 / +0.09 / −0.07 (MAE 1.85) |

- There is a systematic positive bias on all 3 seeds, largest during the lull: `wait_at` counts drain during the walk, so it over-predicts by roughly the walk. Demand-aware wait at ETA is calibrated much better during the lull, but it over-prices the final minutes of rush (`[100,120)` bias +2.7…+4.8 because λ is still high while the queue has already drained). The demand estimator is therefore used only to **trim** over-prediction, never to increase cost.

**Design (`router.rs`, Optimized only):**
- Constant `POST_PEAK_WINDOW_START_MIN = PRIMARY_PEAK_END_MIN − 20 = 100`; applied window `[100,150)` — the last 20 minutes of the primary rush plus the entire lull, where the measured bias is largest and patient tours can straddle the lull/secondary rush.
- `first_step_wait(room)` replaces `wait_at(room)` **only for the first-step wait inside `mean_cost`**:
  - Outside the window, without a demand context (headless/baseline), or when the room is down → return `wait_at` unchanged (R10 downtime pricing preserved).
  - Inside the window: `arrival_wait = future_wait_min(load, est_service, capacity, Some(demand), room, now, walk(room))`; return `min(wait_at, arrival_wait)`.
- `lookahead_cost` (R8) and `wait_sigma` (R16) still use `wait_at`; tie-band, tie-break, baselines unchanged. Mechanism: fix exactly where walk is counted twice, without changing the horizon or any other weight.

**Why min-cap** (5-seed sweep, same standard verification):

| Variant | Mean avg | Mean p90 | Note |
|---|---|---|---|
| R16 control | 79.1771 | 133.3764 | ref |
| Demand wait at all times | 82.8775 | 150.9572 | fails badly: λ over-prices outside post-peak |
| Demand wait only in lull `[120,150)` | 79.5887 | 133.7231 | fails mean p90 (does not improve tail) |
| Demand wait when ETA ≥ 120 | 80.7419 | 133.7582 | fails both means |
| `[100,150)`, no min-cap | 80.0501 | 130.5205 | p90 met but avg +1.10% misses |
| **`[100,150)`, min-cap (chosen)** | **79.9008** | **129.9813** | meets both |
| `[105,150)`, min-cap | 79.6616 | 130.0022 | met but the 105 boundary is arbitrary, no measured basis |
| Cross-ETA, min-cap | 80.8591 | 133.0441 | fails mean p90 |
| Arrival-lull, min-cap | 79.5550 | 133.3764 | fails mean p90 |
| Finish-lull, min-cap | 81.5753 | 133.1937 | fails mean p90 |

- No variant both meets mean p90 and keeps avg better than the chosen point. Min-cap is the condition that avoids breaking the final rush minutes; `[100,150)` is the condition that actually changes seed 5's tail.

**Results — compare** (fresh `/tmp/mediroute-r17-test` → `PORT=8082` → live seed 99 → compare 5 seeds):

| Seed | Strategy | avg_total | avg_wait | p90_total | avg_walk |
|---|---|---|---|---|---|
| 42 | **Optimized** | **65.0999** | **3.1666** | **78.4098** | **11.6167** |
| 42 | Fixed Order | 77.0249 | 5.9134 | 130.2068 | 13.4523 |
| 42 | Shortest Queue | 71.3185 | 4.3273 | 103.8948 | 13.5720 |
| 7 | **Optimized** | **73.6461** | **4.8477** | **128.1961** | **11.6399** |
| 7 | Fixed Order | 84.3898 | 7.0846 | 138.8932 | 13.8233 |
| 7 | Shortest Queue | 77.8194 | 5.5577 | 139.7541 | 13.0962 |
| 123 | **Optimized** | **73.2556** | **6.8929** | **116.3665** | **10.7541** |
| 123 | Fixed Order | 86.4409 | 10.1679 | 145.2265 | 12.7288 |
| 123 | Shortest Queue | 81.0868 | 8.6726 | 126.7505 | 12.4931 |
| 5 | **Optimized** | **117.5001** | **16.9063** | **211.6310** | **11.5661** |
| 5 | Fixed Order | 124.1452 | 18.3369 | 240.2734 | 12.9288 |
| 5 | Shortest Queue | 119.3817 | 16.9931 | 246.6130 | 13.1272 |
| 2024 | **Optimized** | **70.0024** | **4.0933** | **115.3030** | **11.6839** |
| 2024 | Fixed Order | 79.4512 | 6.0142 | 144.2094 | 13.8921 |
| 2024 | Shortest Queue | 74.2294 | 4.8259 | 124.3680 | 13.1494 |

- Baselines match **number-for-number** with R16 on all 15 cells × 4 metrics (serving policy still FIFO, baselines do not read cost).
- Mean 5 seeds (Optimized): avg_total 79.9008, avg_wait 7.1813, p90_total 129.9813, avg_walk 11.4521; FixedOrder 90.2904 / 9.5034 / 159.7619 / 13.3650; ShortestQueue 84.7672 / 8.0753 / 148.2761 / 13.0876.

**Assessment vs Round 16 (Optimized):**

| Seed | Δ avg_total | Δ avg_wait | Δ p90_total | Δ avg_walk |
|---|---|---|---|---|
| 42 | +0.4260 (+0.66%) | +0.1201 (+3.94%) | 0 | −0.0153 (−0.13%) |
| 7 | +1.4162 (+1.96%) | +0.4247 (+9.60%) | 0 | −0.2093 (−1.77%) |
| 123 | +1.0950 (+1.52%) | +0.3365 (+5.13%) | 0 | −0.0568 (−0.53%) |
| 5 | −1.1052 (−0.93%) | −0.2691 (−1.57%) | **−16.9756 (−7.43%)** | −0.1116 (−0.96%) |
| 2024 | +1.7866 (+2.62%) | +0.4880 (+13.54%) | 0 | −0.0528 (−0.45%) |
| **Mean 5 seeds** | **+0.7237 (+0.914%)** | **+0.2200 (+3.16%)** | **−3.3951 (−2.55%)** | **−0.0892 (−0.77%)** |

**Assessment by criterion:**
- **(a) Mean p90_total over 5 seeds better than R16: ✅ MET.** 129.9813 vs 133.3764 (−3.3951, −2.55%).
- **(b) Mean avg_total not worse by more than 1%: ✅ MET.** 79.9008 vs 79.1771 (+0.7237, +0.914%) — close to the threshold.
- **(c) Optimized #1 on every seed on the main metrics: ✅ MET 10/10 cells.**
  - avg_total: s42 65.10 < 71.32 (SQ); s7 73.65 < 77.82; s123 73.26 < 81.09; s5 117.50 < 119.38 (SQ, margin 1.88 minutes); s2024 70.00 < 74.23.
  - p90_total: s42 78.41 < 103.89; s7 128.20 < 139.75; s123 116.37 < 126.75; s5 211.63 < 240.27 (FO, margin 28.64); s2024 115.30 < 124.37.
- The improvement is concentrated on seed 5 — the highest-contention seed and the only one whose tail changed; the other 4 seeds keep p90 unchanged and rise slightly in avg (most on s2024 +2.62%). The mean avg therefore stays within the threshold only because seed 5 drops 1.11.

**Robustness (10 seeds outside the standard set: 11, 77, 999, 1234, 31337, 8, 13, 21, 34, 55; same protocol):** control mean 69.6281 / 101.3457 → R17 69.1945 / 101.1801 (avg −0.62%, p90 −0.16%); avg better on 7/10 seeds. Neutral signal, no systematic regression.

**Honest caveats:**
- **Live seed 99 bit-identical to R16**: finished at `t = 408.14508086711777` minutes, 52/52, 188 visits, **83.4505 / 8.0682 / 157.1367 / 20.5701 / 11.1977** — the live path is unchanged, so there is no new learned-stats drift; all differences lie in the compare routing branches.
- The p90 gain is uneven: seed 5 only; this is a narrow change (first-step wait only in `[100,150)`) and mean avg +0.914% is near the 1% limit, so coordinator independent verification is needed before considering it commit-worthy.

**Tests:**
- `cargo test`: **87/87 pass** (81 existing + 6 new; no existing test changed expectation).
- 6 router tests: `first_step_wait_uses_the_demand_aware_arrival_wait_in_post_peak` (now=125, walk 3.5, queue 2, service 10, cap 1 → 16.5 instead of 20, cost 30); `first_step_wait_keeps_the_wait_from_now_outside_the_post_peak_window` (now 50/99.9/150/180 and demand=None → exactly `wait_at`); `first_step_wait_never_inflates_over_the_wait_from_now` (demand wait at ETA higher → keep `wait_at`); `peak_backlogged_room_projected_wait_falls_as_the_primary_peak_ends` (Orthopedics cap 1, queue 2: wait at t=118 < at t=100, while `wait_at` is unchanged); `post_peak_pricing_does_not_change_baseline_choices` (FixedOrder/ShortestQueue choose as before at now=125); `post_peak_pricing_leaves_the_round_sixteen_sigma_horizon_unchanged` (σ still uses horizon walk + `wait_at`).

**Files changed:** `backend/src/engine/router.rs` (`POST_PEAK_WINDOW_START_MIN`, `first_step_wait`, `mean_cost`; 6 tests), `docs/rounds/README.md`. Measurement instrumentation in `sim.rs` fully reverted — `git diff` contains only `router.rs` + docs. Unchanged: `congestion.rs`, `scenario.rs`, `sim.rs`, `api.rs`, `main.rs`, `domain.rs`, `stats.rs`, `learn.rs`, frontend, baselines, serving policy, tie-band, lookahead, σ term, downtime.

**Verification steps:**
1. `cargo test`: **87/87 pass**.
2. `npm run build`: OK (frontend unchanged; pre-existing chunk > 500 kB warning).
3. Fresh `MEDIROUTE_DATA_DIR=/tmp/mediroute-r17-test`, separate server `PORT=8082` (8080 untouched); live `{strategy:optimized, speed:5000, seed:99}` (API clamp max 60 as before) → finished 408.145 minutes, 52/52, 188 visits, metrics recorded above — bit-identical to R16.
4. `POST /api/sim/compare` seed 42 → 7 → 123 → 5 → 2024; all 4 metrics × 3 strategies; baselines match R16 number-for-number, Optimized #1 10/10 cells.
5. Variant sweep: demand-wait all the time / lull only / cross-ETA / `[100,150)` without min-cap / `[105,150)` / cross + min / arrival-lull / finish-lull; each variant run on all 5 seeds from the same stats snapshot.
6. Robustness: 10 seeds outside the standard set, control vs R17 on the same protocol.
7. Port 8080 untouched; **no git commit/push**.

**Decision:** COMMIT. Meets all 3 technical criteria of the round on the standard verification; coordinator independent verification (fresh data dir, PORT=8082, live seed 99, compare 5 seeds) matches the agent's report **number-for-number**. The p90 gain is narrow (concentrated on seed 5) but has a clear measurement basis (bias instrumentation in the doc), the 10-seed out-of-set robustness is clean (avg −0.62%), tests 87/87, baselines bit-identical. The p90 value is worth committing even though mean avg +0.914% is near the threshold — this is the tail-vs-mean trade-off accepted since R16.

**Independent verification (coordinator):** `cargo test` 87/87 pass; separate server PORT=8082, fresh `/tmp/mediroute-r17-verify`, live seed 99 speed 5000, compare seeds 42/7/123/5/2024 — Optimized: s42 65.10/78.41, s7 73.65/128.20, s123 73.26/116.37, s5 117.50/211.63, s2024 70.00/115.30 (avg_total/p90_total) — matches the agent's report. Mean avg 79.90 (+0.914% vs R16), mean p90 129.98 (−2.55%). Optimized #1 10/10 cells. Baselines match R16 number-for-number.

**Lessons learned:** (1) There is systematic over-prediction in first-step wait (walk counted twice: once in cost, once in wait-at-now) measurable by instrumentation before the fix — measure first, fix later, and accept a new estimator only when it *reduces* bias on the measured data. (2) Demand-aware wait at ETA calibrates well in the lull but over-prices the end of rush → min-cap is the right mechanism (never let the new prediction increase cost). (3) The `[100,150)` window (peak-edge + lull) is the only variant among 10 sweeps that both meets mean p90 and keeps avg within the threshold — variant sweeps on the same protocol are how to choose a design when multiple options are plausible.

## Round 18 — Secondary-peak anticipation

**Status:** ❌ DISCARD — the under-prediction hypothesis was refuted by measurement: positive bias (over-prediction) on **5/5 seeds, 3/3 sub-windows**; `wait_at` under-predicts in only 5/263 decisions. No routing/congestion code changed; the working tree only adds this docs section.

**Proposed aspect:**
- R17 just fixed first-step wait in the post-peak window `[100,150)` with the **min-cap** `min(wait_at, demand_wait@ETA)`, because demand@ETA was measured to over-price the final minutes of the primary rush. The opposite side has not been tried: when the first step's ETA falls before/in the secondary rush `[150,210)`, new arrivals are rebuilding queues, so `demand_wait@ETA` may be **larger** than `wait_at`; if so, `wait_at` is under-pricing rising congestion and the patient is routed to a room that is "free now" but has a long queue on arrival.
- Proposal (if there is a signal): asymmetric **max-cap** `max(wait_at, demand_wait@ETA)` in the ETA window `[135,210)`, without breaking R17's min-cap `[100,150)`.

**Instrumentation (temporary in `sim.rs`, fully reverted):**
- In `World::next_room`, record each decision: `now`, `walk`, `wait_at` (the exact base of `first_step_wait`, including the downtime branch), `demand_wait@ETA = future_wait_min(..., now, walk)`, and the R17 predictor (min-cap when `now ∈ [100,150)` and no downtime, otherwise `wait_at`).
- Actual wait comes from `VisitRecord.wait_min` for the exact (patient, room) pair; each patient visits each room once, so the match is 1-1.
- The stats snapshot comes from live seed 99 (standard protocol: fresh learner → live optimized seed 99 → `Learner::from_stats`), then headless Optimized runs on 5 seeds. Decision window by **ETA = now + walk** ∈ `[135,210)`, split into `[135,150)`, `[150,180)`, `[180,210)`. Bias = pred − actual (positive = over-predict).
- The entire probe used `#[cfg(test)]` + temporary fields and was removed; `git diff` was empty before writing the docs.

**Bias by seed (ETA ∈ [135,210), minutes):**

| Seed | n | Actual mean | `wait_at` bias / MAE | `demand@ETA` bias / MAE | R17 bias / MAE | max-cap bias / MAE |
|---|---|---|---|---|---|---|
| 42 | 65 | 4.27 | **+6.12** / 6.69 | +3.43 / 3.93 | +5.98 / 6.59 | +6.79 / 7.25 |
| 7 | 47 | 5.02 | **+3.58** / 3.58 | +2.32 / 2.44 | +1.87 / 1.99 | +4.75 / 4.75 |
| 123 | 43 | 10.92 | **+9.66** / 10.04 | +3.84 / 4.13 | +7.76 / 8.13 | +10.56 / 10.86 |
| 5 | 51 | 22.62 | **+12.00** / 12.15 | +2.10 / 6.23 | +9.15 / 11.33 | +12.53 / 12.68 |
| 2024 | 57 | 6.74 | **+7.19** / 7.54 | +4.08 / 4.51 | +5.28 / 5.78 | +8.41 / 8.69 |
| Mean 5 seeds | 263 | — | **+7.71 / 8.00** | +3.15 / 4.25 | +6.01 / 6.76 | +8.60 / 8.85 |

- Frequency of the hypothesized mechanism: `demand@ETA > wait_at` in **122/263** decisions (46.4%) — the "queue grows with ETA" mechanism is real, mostly in `[150,180)` (52/95 = 54.7%) and `[180,210)` (67/127 = 52.8%).
- But against actual: `wait_at < actual` in only **5/263** (1.9%); `demand@ETA < actual` in only **13/263** (4.9%). Both estimators over-predict; max-cap picks the larger estimate, so it over-prices even more.

**Bias by sub-window (mean of 5 seeds):**

| Sub-window | n | Actual mean | `wait_at` bias / MAE | `demand@ETA` bias / MAE | R17 bias / MAE | max-cap bias / MAE | `dem>at` |
|---|---|---|---|---|---|---|---|
| `[135,150)` | 41 | 14.14 | **+10.81 / 10.81** | +1.88 / 4.06 | +1.86 / 4.04 | +11.16 / 11.16 | 3/41 |
| `[150,180)` | 95 | 7.58 | **+5.06 / 5.63** | +2.91 / 4.21 | +4.64 / 5.21 | +5.88 / 6.34 | 52/95 |
| `[180,210)` | 127 | 10.48 | **+8.76 / 8.90** | +3.73 / 3.97 | +8.76 / 8.90 | +9.87 / 9.98 | 67/127 |

**Split by decision time** (mean 5 seeds, bias / MAE):

| Group | n | `wait_at` | `demand@ETA` | R17 | max-cap | `wait_at < actual` |
|---|---|---|---|---|---|---|
| decided `now < 150` (R17 min-cap active) | 47 | +9.94 / 10.34 | +1.61 / 3.82 | **+1.54 / 3.84** | +10.29 / 10.60 | 2/47 |
| decided `now ≥ 150` (in secondary rush) | 216 | **+7.22 / 7.47** | +3.59 / 4.34 | = `wait_at` | +8.24 / 8.45 | 3/216 |

**Interpretation — why there is no under-pricing signal:**
- R17's symmetric mechanism exists but is **not strong enough to create real under-prediction**: on average `demand@ETA − wait_at = −4.56` minutes over the whole window (only seed 7's `[180,210)` sub-window is slightly positive at +0.91). When demand@ETA exceeds wait_at the gap is small; when it is below, the gap is large (walk-time drain + demand model over-estimating arrivals by room share).
- Physical reason: `wait_at` already includes drain during the walk, and the rooms Optimized chooses are usually shedding load; actual wait averages 4–23 minutes but is below the prediction in 98% of decisions. In the secondary rush, `demand@ETA` also still over-predicts actual (+3.59 in the decided `now ≥ 150` group), so "pricing at ETA" is not the correct price.
- Therefore **max-cap was not implemented**: it makes bias/MAE worse than `wait_at` on 5/5 seeds and 3/3 sub-windows (mean MAE 8.85 vs 8.00), i.e., it would actively inflate cost.
- Side observation (outside the round scope, not benchmarked): R17's min-cap clearly improves calibration in the `now < 150` group (bias +1.54 vs +9.94 for wait_at). Demand@ETA is also better calibrated than wait_at in the secondary rush (MAE 4.25 vs 8.00). This suggests **extending min-cap** to the secondary rush — but that is the opposite direction from this round's rising-demand pricing hypothesis and unverified, so it was not done.

**Variant sweep:** **Not run** — per methodology, a sweep is only performed if there is a systematic negative bias. Positive bias was measured on every seed/sub-window, so the round stopped before the pricing design step; no improvement was forced.

**5-seed compare** (fresh `/tmp/mediroute-r18-test`, `PORT=8082`, live seed 99 speed 5000 → compare 42/7/123/5/2024, on the **final tree after revert**):

| Seed | Strategy | avg_total | avg_wait | p90_total | avg_walk |
|---|---|---|---|---|---|
| 42 | **Optimized** | **65.0999** | **3.1666** | **78.4098** | **11.6167** |
| 42 | Fixed Order | 77.0249 | 5.9134 | 130.2068 | 13.4523 |
| 42 | Shortest Queue | 71.3185 | 4.3273 | 103.8948 | 13.5720 |
| 7 | **Optimized** | **73.6461** | **4.8477** | **128.1961** | **11.6399** |
| 7 | Fixed Order | 84.3898 | 7.0846 | 138.8932 | 13.8233 |
| 7 | Shortest Queue | 77.8194 | 5.5577 | 139.7541 | 13.0962 |
| 123 | **Optimized** | **73.2556** | **6.8929** | **116.3665** | **10.7541** |
| 123 | Fixed Order | 86.4409 | 10.1679 | 145.2265 | 12.7288 |
| 123 | Shortest Queue | 81.0868 | 8.6726 | 126.7505 | 12.4931 |
| 5 | **Optimized** | **117.5001** | **16.9063** | **211.6310** | **11.5661** |
| 5 | Fixed Order | 124.1452 | 18.3369 | 240.2734 | 12.9288 |
| 5 | Shortest Queue | 119.3817 | 16.9931 | 246.6130 | 13.1272 |
| 2024 | **Optimized** | **70.0024** | **4.0933** | **115.3030** | **11.6839** |
| 2024 | Fixed Order | 79.4512 | 6.0142 | 144.2094 | 13.8921 |
| 2024 | Shortest Queue | 74.2294 | 4.8259 | 124.3680 | 13.1494 |

- Mean 5 seeds (Optimized): avg_total **79.9008**, avg_wait 7.1814, p90_total **129.9813**, avg_walk 11.4521; FixedOrder 90.2904 / 9.5034 / 159.7619 / 13.3650; ShortestQueue 84.7672 / 8.0753 / 148.2761 / 13.0876.
- All 15 cells × 4 metrics **bit-identical to R17**, including live seed 99: finished at `t = 408.14508086711777`, 52/52, 188 visits, 83.4505 / 8.0682 / 157.1367 / 20.5701 / 11.1977. Confirms the final tree does not change behavior.

**Assessment by criterion:**
- **(a) Mean p90_total better than R17: ❌ NOT MET** — unchanged at 129.9813 (no change).
- **(b) Mean avg_total not worse by more than 1%: ✅ MET** — 79.9008, Δ = 0.
- **(c) Optimized #1 on every seed and main metric: ✅ MET 10/10 cells** — unchanged from R17.
- The round produced no candidate change, so (a) cannot be met; this is a valid negative result per protocol, not a regression.

**Tests:**
- `cargo test`: **87/87 pass** before and after removing the instrumentation. No new tests because no production logic changed; the measurement probe was temporary `#[cfg(test)]` and fully reverted.
- The final `git diff` contains only `docs/rounds/README.md`; `backend/src/engine/sim.rs` returned exactly to HEAD.

**Files changed:** `docs/rounds/README.md` (this section). Unchanged: `router.rs`, `congestion.rs`, `sim.rs`, `api.rs`, `main.rs`, `domain.rs`, `scenario.rs`, `stats.rs`, `learn.rs`, frontend, baselines, serving/FIFO, priority, downtime, learning, API/JSON.

**Verification steps:**
1. `cd backend && ~/.local/bin/cargo test` → 87/87 pass (baseline before instrumentation and after revert).
2. Baseline at HEAD: fresh `MEDIROUTE_DATA_DIR=/tmp/mediroute-r18-test`, separate server `PORT=8082` (8080 untouched), live `{strategy:optimized, speed:5000, seed:99}` → 408.1451 minutes, 52/52, 188 visits, metrics above; compare 42/7/123/5/2024 matches R17 number-for-number.
3. Temporary instrumentation in `sim.rs` (probe `wait_at` / `demand@ETA` / R17 / max-cap vs actual wait by patient-room pair), run 5 seeds with the stats snapshot from live seed 99; collect the bias tables above; remove instrumentation, `git diff` empty.
4. Final verify on the reverted tree: fresh `/tmp/mediroute-r18-test`, `PORT=8082`, live seed 99 → bit-identical to R17; compare 5 seeds → table above, baselines also bit-identical to R17.
5. Port 8080 untouched; test server 8082 stopped; **no git commit/push**.

**Decision:** **DISCARD — no code change.** The secondary-peak under-pricing hypothesis is refuted: positive bias systematically on 5/5 seeds and 3/3 sub-windows; max-cap makes calibration worse. The working tree keeps only this docs section for the coordinator to cross-check; there is no candidate to commit besides docs.

**Lessons learned:** (1) A model-vs-model mechanism can be real (demand@ETA > wait_at in 46% of decisions) yet still create no real under-prediction — always check against **actual wait**, not just compare two estimators. (2) Walk-time drain makes `wait_at` over-predict almost everywhere, including in the secondary rush; therefore any cap that increases price (max-cap) is the wrong direction. (3) R17's min-cap is the right intervention but has a limited scope; extending it to `[150,210)` is a different hypothesis (min-cap, not rising-demand max-cap), with supporting calibration evidence but needing a separate round to benchmark before concluding.

## Round 19 — Secondary-peak min-cap extension

**Status:** ❌ DISCARD — the hypothesis of extending min-cap into the secondary rush was refuted at the outcome level: **41 configurations were run (control + 40 variants), none improved mean p90 over 5 seeds** (benchmark 129.9813); most worsened the tail, especially s42 (+10.26) and s5 (+16.98). No routing code changed; the working tree only adds this docs section.

**Proposed aspect:**
- R17 committed the min-cap `first_step_wait = min(wait_at, demand@ETA)` in `[100,150)` and proved that the "only decrease, never inflate" mechanism is correct post-peak. R18 left an unexploited calibration signal: in the decision group `now ≥ 150` (secondary rush), `wait_at` has bias +7.22 / MAE 7.47, while `demand@ETA` is +3.59 / MAE 4.34 — nearly twice as well calibrated; in `[180,210)` it is 8.90 vs 3.97.
- Hypothesis: if `demand@ETA` is better calibrated in the secondary rush and both estimators over-predict, extending min-cap to the decision window `now ∈ [150,210)` (or another window/formula variant) will trim wait_at's over-prediction without ever increasing price, exactly the mechanism that won in R17.

**R18 numbers reused (not re-measured):**

| Decision group | n | `wait_at` bias / MAE | `demand@ETA` bias / MAE |
|---|---|---|---|
| decided `now < 150` (R17 active) | 47 | +9.94 / 10.34 | +1.61 / 3.82 |
| decided `now ≥ 150` (secondary rush) | 216 | +7.22 / 7.47 | +3.59 / 4.34 |
| ETA ∈ `[150,180)` | 95 | +5.06 / 5.63 | +2.91 / 4.21 |
| ETA ∈ `[180,210)` | 127 | +8.76 / 8.90 | +3.73 / 3.97 |

- Important note from R18: `demand@ETA < actual` in only 13/263 decisions (4.9%), `wait_at < actual` in only 5/263 (1.9%) — both still over-predict, so min-cap may only choose the smaller estimate, never inflate.

**Sweep design (temporary, fully reverted):**
- Temporary override in `router.rs`: extended window `[start,end)`, anchored by `now` or by `ETA = now + walk`, keeping/disabling R17; correction modes: `min` (R17-style), `margin:m` → `min(wait_at, demand@ETA + m)`, `minwait:t` (apply only when `wait_at ≥ t`), `blend:w` → `min(wait_at, (1−w)·wait_at + w·demand@ETA)`, `walkfloor` → `min(wait_at, max(demand@ETA, wait_at − walk))`, `walkcap` → `wait_at − walk` (diagnostic).
- Temporary `#[ignore]` harness in `sim.rs` reproduces the server protocol exactly: fresh learner → live Optimized seed 99 → stats snapshot → compare 5 seeds × 3 strategies. The `[100,150)` control reproduces HEAD **bit-identically** (live t=408.145081, 83.4505/8.0682/157.1367/11.1977; mean 79.9008 / 129.9813) — the harness is valid. All min-cap variants keep live seed 99 bit-identical; only the `walkcap` diagnostic changes live (t=408.1118), so the compare results of the min-cap variants are apples-to-apples.
- 41 configurations in total (control + 40 variants; some behaviorally equivalent because their windows are contiguous with R17), each with all 5 seeds. Baselines are invariant across the entire batch (FixedOrder/ShortestQueue match HEAD number-for-number).

**Variant sweep — mean 5 seeds (Optimized), Δ vs HEAD:**

| Variant | Mean avg | Δ avg | Mean p90 | Δ p90 |
|---|---|---|---|---|
| **R17 control `[100,150)`** | **79.9008** | — | **129.9813** | — |
| `[100,210)` now | 80.6445 | +0.93% | 135.4290 | +5.4477 |
| `[150,210)` now (≡ `[100,210)` because contiguous with R17) | 80.6445 | +0.93% | 135.4290 | +5.4477 |
| `[135,210)` now (≡ `[100,210)` for the same reason) | 80.6445 | +0.93% | 135.4290 | +5.4477 |
| `[150,180)` now | 80.4651 | +0.71% | 132.0339 | +2.0526 |
| `[150,165)` now | 80.4893 | +0.74% | 132.0893 | +2.1080 |
| `[150,160)` now | 80.0814 | +0.23% | 129.9813 | 0 |
| `[150,210)` ETA (≡ now-based) | 80.6445 | +0.93% | 135.4290 | +5.4477 |
| ETA ∈ `[150,165)` | 80.0814 | +0.23% | 129.9813 | 0 |
| ETA ∈ `[150,180)` | 80.5657 | +0.83% | 132.1753 | +2.1940 |
| ETA ∈ `[180,210)` | 80.2931 | +0.49% | 136.0288 | +6.0475 |
| ETA ∈ `[190,210)` | 80.1007 | +0.25% | 134.4560 | +4.4747 |
| `[150,180)` margin:4 | 79.7263 | −0.22% | 130.0022 | +0.0209 |
| `[150,180)` margin:6 | 79.5247 | −0.47% | 130.0022 | +0.0209 |
| `[150,210)` margin:6 | 79.9201 | +0.02% | 132.4440 | +2.4627 |
| `[150,210)` minwait:10 | 80.5995 | +0.87% | 134.4383 | +4.4570 |
| `[150,210)` walkfloor | 79.2719 | −0.79% | 134.0685 | +4.0872 |
| `[150,210)` blend:0.75 | 80.0107 | +0.14% | 131.7401 | +1.7588 |
| `[150,180)` walkcap (diagnostic) | 79.0924 | −1.01% | 136.5459 | +6.5646 |
| Secondary rush only `[150,210)`, R17 off | 79.6392 | −0.33% | 135.4290 | +5.4477 |
| ETA-overall `[100,210)`, replace R17 | 80.7689 | +1.09% | 135.0757 | +5.0944 |

- **No configuration has mean p90 < 129.9813.** The best is a tie (very narrow windows `[150,160)` / ETA `[150,165)`); the rest are worse by +1.76…+6.56.
- The full window `[150,210)` raises p90 on **s42 78.41→88.67 (+10.26)** and **s5 211.63→228.61 (+16.98 — exactly the pre-R17 R16 level)**; the other two seeds are unchanged. That is, extending min-cap not only adds no gain but erases exactly the tail gain R17 achieved on s5.
- Narrower windows: `[150,180)` only raises s42 (+10.26) and mean p90 +2.05; `[150,165)` raises s42 (+10.54); `[150,160)` leaves every seed's p90 unchanged but mean avg +0.23% → no benefit.
- Damping (margin/walkfloor/blend/minwait) can improve mean avg (walkcap `[150,180)` −1.01%, walkfloor −0.79%, margin `[150,180)` −0.22…−0.47%) but **never improves p90**; margin `[150,180)` is still +0.0209 (not better than the benchmark).
- Cross-ETA is not an escape: with R17 kept, `[150,210)` ETA behaves identically to `[150,210)` now because every decision with `now ≥ 150` has ETA ≥ 150, while `now < 150` is already inside R17. The other narrow ETA windows (`[150,165)`, `[150,180)`, `[180,210)`, `[190,210)`) also do not improve p90.

**Interpretation — why better calibration does not win the outcome:**
- Min-cap improves the wait forecast per decision but at the same time **changes the room selection order** in the secondary rush. `lookahead_cost` (R8) and `wait_sigma` (R16) still use `wait_at` per the round constraint, so the first-step price and the lookahead/risk price are no longer consistent; long-tail cases (p90) are sensitive to a few room changes and worsen.
- Both estimators still over-predict actual (R18: only 1.9%/4.9% of decisions under-predict), so min-cap cannot fix the "baseline offset", only amplify differences between rooms. R17 cuts exactly the walk that is double-counted post-peak; in the secondary rush, the cut is larger and does not correspond to an actual total improvement.
- The window boundary is part of the mechanism, not a detail: extending to 165 is already enough to worsen s42; extending to 210 makes s5 lose the entire R17 gain. This is why no "approximately right" variant meets all three criteria.

**5-seed compare on the final tree** (HEAD, no code change — bit-identical R17/R18):

| Seed | Strategy | avg_total | avg_wait | p90_total | avg_walk |
|---|---|---|---|---|---|
| 42 | **Optimized** | **65.0999** | **3.1666** | **78.4098** | **11.6167** |
| 42 | Fixed Order | 77.0249 | 5.9134 | 130.2068 | 13.4523 |
| 42 | Shortest Queue | 71.3185 | 4.3273 | 103.8948 | 13.5720 |
| 7 | **Optimized** | **73.6461** | **4.8477** | **128.1961** | **11.6399** |
| 7 | Fixed Order | 84.3898 | 7.0846 | 138.8932 | 13.8233 |
| 7 | Shortest Queue | 77.8194 | 5.5577 | 139.7541 | 13.0962 |
| 123 | **Optimized** | **73.2556** | **6.8929** | **116.3665** | **10.7541** |
| 123 | Fixed Order | 86.4409 | 10.1679 | 145.2265 | 12.7288 |
| 123 | Shortest Queue | 81.0868 | 8.6726 | 126.7505 | 12.4931 |
| 5 | **Optimized** | **117.5001** | **16.9063** | **211.6310** | **11.5661** |
| 5 | Fixed Order | 124.1452 | 18.3369 | 240.2734 | 12.9288 |
| 5 | Shortest Queue | 119.3817 | 16.9931 | 246.6130 | 13.1272 |
| 2024 | **Optimized** | **70.0024** | **4.0933** | **115.3030** | **11.6839** |
| 2024 | Fixed Order | 79.4512 | 6.0142 | 144.2094 | 13.8921 |
| 2024 | Shortest Queue | 74.2294 | 4.8259 | 124.3680 | 13.1494 |

- Mean 5 seeds (Optimized): avg_total **79.9008**, avg_wait 7.1813, p90_total **129.9813**, avg_walk 11.4521; FixedOrder 90.2904 / 9.5034 / 159.7619 / 13.3650; ShortestQueue 84.7672 / 8.0753 / 148.2761 / 13.0876. Baselines match R17/R18 number-for-number.
- Live seed 99: finished at `t = 408.145081`, 52/52, 188 visits, 83.4505 / 8.0682 / 157.1367 / 20.5701 / 11.1977 — bit-identical to R17/R18.

**Assessment by criterion:**
- **(a) Mean p90_total over 5 seeds better than 129.9813: ❌ NOT MET** — no configuration is better; the best is a tie at 129.9813 (`[150,160)` now / ETA `[150,165)`), and the full window is +5.4477.
- **(b) Mean avg_total not worse by more than 1%: ✅ MET** for the control and many damping variants (walkcap −1.01%, walkfloor −0.79%), but those variants all fail (a).
- **(c) Optimized #1 on every seed and main metric: ✅ MET 10/10 cells** on the final tree; the mild candidates (`[150,160)`, margin `[150,180)`) also keep 10/10, but do not meet (a).
- **No candidate meets all 3 criteria → DISCARD per protocol.** This is a valid negative result, not a regression: the final tree is unchanged from HEAD.

**Tests:**
- `cargo test`: **87/87 pass** before instrumentation and after revert. No production tests added because no logic change is kept (R18 precedent).
- The temporary override in `router.rs` and the `#[ignore]` harness in `sim.rs` were fully reverted; the final `git diff` contains only `docs/rounds/README.md`.

**Files changed:** `docs/rounds/README.md` (this section). Unchanged: `router.rs`, `congestion.rs`, `sim.rs`, `api.rs`, `main.rs`, `domain.rs`, `scenario.rs`, `stats.rs`, `learn.rs`, frontend, baselines, serving/FIFO, priority, downtime, learning, API/JSON, tie-band, lookahead, σ term.

**Verification steps:**
1. `cd backend && ~/.local/bin/cargo test` → 87/87 pass (HEAD and after revert).
2. Baseline/final: fresh `MEDIROUTE_DATA_DIR=/tmp/mediroute-r19-test`, separate server `PORT=8082` (8080 untouched), live `{strategy:optimized, speed:5000, seed:99}` → 408.145081 minutes, 52/52, 188 visits, metrics above; compare 42/7/123/5/2024 matches R17/R18 number-for-number, Optimized #1 10/10 cells.
3. Sweep: temporary harness in `sim.rs` (fresh learner → live seed 99 → snapshot → 5 seeds × 3 strategies) + temporary override in `router.rs`; 41 configurations; each with all 5 seeds; baselines invariant; control bit-identical to HEAD.
4. Revert instrumentation; `git status` clean (only docs); port 8080 untouched; server 8082 stopped; **no git commit/push**.

**Decision:** **DISCARD — no code change.** The hypothesis of extending min-cap into the secondary rush is refuted at the outcome level: 41 configurations, zero improve mean p90; the full window even erases R17's exact tail gain on s5. The working tree keeps only this docs section for the coordinator to cross-check.

**Lessons learned:** (1) Calibration (bias/MAE) is necessary but not sufficient: a better per-decision estimator can still make routing worse because it changes room selection order, especially when lookahead/σ still use the old estimator — benchmark the outcome, do not infer from calibration. (2) R17's min-cap works within a narrow scope; extending it to the secondary rush not only adds no gain but erases the old gain (s5 +16.98, exactly back to R16), so the window boundary is part of the design. (3) When a hypothesis fails, a sufficiently broad sweep (window/anchor/formula/damping) and an honest DISCARD are the correct result — do not force a "nearly passing" variant into a commit.

## Round 20 — Estimator consistency (first-step ↔ lookahead/σ)

**Status:** ❌ DISCARD — the estimator-consistency hypothesis was refuted at the outcome level: **9 configurations (control + 8 variants) ran the full 5-seed protocol, none improved mean p90 129.9813**. A1/C keep p90 unchanged but raise mean avg +0.076%; A2 (min-cap on the second hop) raises mean p90 +2.5601 (+1.97%), with s42 +12.80; B (σ horizon) changes σ but not a single decision. No routing code changed; the working tree only adds this docs section.

**Proposed aspect:**
- Diagnosis from R19: after R17, first-step wait in `[100,150)` uses `min(wait_at, demand_wait@ETA)`, but two other places in the same cost function still use raw `wait_at`:
  - `lookahead_cost` computes `finish_in_min = walk + wait_at + service`, so the second hop's ETA is pushed later by the over-prediction;
  - `wait_sigma` uses `horizon = walk + wait_at` (the horizon only affects arrival variance and the likely-arrivals filter).
- Hypothesis: if R17 is right because it cuts walk-double-count/over-prediction, the other cost components suffer the same bias and should be synchronized within the **exact R17 window** `[100,150)`: `lookahead_cost` uses min-capped wait at ETA (first hop: ETA; second hop: 2-step ETA) and/or `wait_sigma` uses min-capped wait in the horizon, keeping the "decrease only, never inflate" mechanism. Do not extend the window (R19 already refuted that direction).

**Sweep design (temporary, fully reverted):**
- `#[cfg(test)] mod sweep` in `router.rs`: bit `LOOKAHEAD_FINISH` (A1: `finish_in_min` uses `first_step_wait`), `LOOKAHEAD_CAP` (A2: `wait_next = min(future_wait@ETA2, wait_at)` inside the window), `SIGMA_HORIZON` (B: horizon uses `first_step_wait`), `ETA_ANCHOR` (gate A1/B by `now + walk ∈ [100,150)` instead of `now`). Two temporary helpers `lookahead_first_wait` / `sigma_wait` replace `wait_at` with `first_step_wait` only when the bit is on; non-test builds keep HEAD unchanged.
- `#[ignore]` harness in `sim.rs` reproduces the server protocol: fresh learner → live Optimized seed 99 **under each variant** → stats snapshot → compare 5 seeds × 3 strategies.
- Control reproduces **HEAD bit-identically**: live `t = 408.145081`, 83.4505 / 8.0682 / 157.1367 / 20.5701 / 11.1977; mean 79.9008 / 129.9813. Baselines invariant across **all 9 configurations** (FixedOrder/ShortestQueue match HEAD number-for-number in 10/10 rows = 5 seeds × 2 strategies, all 4 metrics).
- 9 configurations: control; A1; A2; A1+A2; B; C=A1+B; C+A2; A1-ETA; C-ETA.

**Variant sweep — Optimized, mean 5 seeds (each configuration on the full protocol):**

| # | Variant | Mean avg | Δ avg | Mean p90 | Δ p90 | Note |
|---|---|---|---|---|---|---|
| 0 | control | **79.9008** | — | **129.9813** | — | bit-identical HEAD |
| 1 | A1 lookahead-finish | 79.9615 | +0.0607 (+0.076%) | 129.9813 | 0 | only s5 avg +0.3033 |
| 2 | A2 second-hop-cap | 80.1947 | +0.2939 (+0.368%) | 132.5414 | +2.5601 (+1.97%) | s42 p90 +12.8007 |
| 3 | A1+A2 | 80.2553 | +0.3545 (+0.444%) | 132.5414 | +2.5601 | = A2 + A1 on s5 |
| 4 | B σ-horizon | **79.9008** | 0 | **129.9813** | 0 | 0 decisions changed |
| 5 | C=A1+B | 79.9615 | +0.0607 (+0.076%) | 129.9813 | 0 | ≡ A1 |
| 7 | C+A2 | 80.2553 | +0.3545 (+0.444%) | 132.5414 | +2.5601 | ≡ A1+A2 |
| 9 | A1 ETA-anchor | 79.9615 | +0.0607 (+0.076%) | 129.9813 | 0 | ≡ A1 |
| 13 | C ETA-anchor | 79.9615 | +0.0607 (+0.076%) | 129.9813 | 0 | ≡ A1 |

- A1/C/A1-ETA/C-ETA change only seed 5: avg 117.5001 → 117.8034 (+0.3033), p90 stays 211.6310. ETA-anchor does not change the result compared with now-anchor.
- A2 makes s42 p90 78.4098 → 91.2105 (+12.8007), avg 65.0999 → 65.6783; s7 avg +0.4217; s123 avg +0.1839; s5 avg +0.2853; s2024 unchanged. A1+A2/C+A2 add +0.3033 to s5 avg (mean avg 80.2553, +0.444%) but p90 is exactly A2.
- Live seed 99 **bit-identical in all 9 configurations** → the stats snapshot is the same, compare differences are pure routing, no learning confound.

**Diagnostics (counters accumulated across live + 5 seeds × 3 strategies):**

| Variant | Mechanism | Count | Magnitude | Consequence |
|---|---|---|---|---|
| A1 | first-step cut used for ETA2 | 2.970 calls | total 2.860,36 minutes; avg 0,963 minutes/call | only s5 changes path; avg +0.30 |
| A1 ETA | as above, gated by first-hop ETA | 523 calls | total 2.726,74 minutes; avg 5,21 minutes/call | result ≡ A1 |
| A2 | cap second-hop wait | 1.333 checks, 220 binds (16,5%) | total 1.200,16 minutes; avg 5,46 minutes/bind | s42 p90 +12.80 |
| B | horizon = walk + first_step_wait | 2.970 calls | wait cut 2.825,00 minutes; \|Δσ\| avg 0,0165, max 1,326 | 0 decisions changed |

**Interpretation — why consistency does not win:**
- **A1 is the "correct" fix but works against R17.** When ETA2 is computed from `first_step_wait` (earlier), `future_wait(next, ETA2)` increases because the room has not had time to drain; the 0.5-weighted lookahead therefore gives back most of the first-step amount just cut. On seed 5 — the only seed where R17's min-cap actually changes the path — this recovers +0.3033 avg without improving p90. In other words, the old inconsistency (late ETA2) was unintentionally preserving the first-step benefit of R17.
- **A2 shows that lookahead conservatism is functional.** Capping `wait_next` to `wait_at` binds mostly on hot follow-ups (avg cut 5,46 minutes per bind), making the second step look cheaper than reality and luring the s42 path into a long-tail tour: p90 +12.80. This is exactly the warning in the hypothesis — min-cap at the 2-step ETA over-cuts because lookahead estimates farther out and needs to be more conservative, not the same kind of bias as first-step.
- **B performs the synchronization correctly, but σ has no leverage in the window.** `wait_sigma_min` uses the horizon only for arrival variance and the likely-arrivals filter; queue variance (the dominant part) depends on the number of jobs ahead, not the horizon. During the lull `[120,150)` λ = 0 so arrival variance = 0; at the peak edge the horizon is short so the change is small. Result: |Δσ| averages 0,0165 minutes, max 1,326; with κ = 0.5, the maximum cost impact is 0,663 minutes — not enough to change any choice on 5 seeds. Hence C ≡ A1.
- **No variant improves p90**, even when tightening the anchor by ETA or combining all three components. The hypothesis is arithmetically correct (the estimators really do disagree) but behaviorally wrong: each component serves a different role (first-step = mean estimate; lookahead = far evaluation, needs conservatism; σ = spread around the number of jobs ahead), so "same bias" does not imply "same fix".

**5-seed compare on the final tree** (HEAD, no code change — bit-identical R17/R18/R19):

| Seed | Strategy | avg_total | avg_wait | p90_total | avg_walk |
|---|---|---|---|---|---|
| 42 | **Optimized** | **65.0999** | **3.1666** | **78.4098** | **11.6167** |
| 42 | Fixed Order | 77.0249 | 5.9134 | 130.2068 | 13.4523 |
| 42 | Shortest Queue | 71.3185 | 4.3273 | 103.8948 | 13.5720 |
| 7 | **Optimized** | **73.6461** | **4.8477** | **128.1961** | **11.6399** |
| 7 | Fixed Order | 84.3898 | 7.0846 | 138.8932 | 13.8233 |
| 7 | Shortest Queue | 77.8194 | 5.5577 | 139.7541 | 13.0962 |
| 123 | **Optimized** | **73.2556** | **6.8929** | **116.3665** | **10.7541** |
| 123 | Fixed Order | 86.4409 | 10.1679 | 145.2265 | 12.7288 |
| 123 | Shortest Queue | 81.0868 | 8.6726 | 126.7505 | 12.4931 |
| 5 | **Optimized** | **117.5001** | **16.9063** | **211.6310** | **11.5661** |
| 5 | Fixed Order | 124.1452 | 18.3369 | 240.2734 | 12.9288 |
| 5 | Shortest Queue | 119.3817 | 16.9931 | 246.6130 | 13.1272 |
| 2024 | **Optimized** | **70.0024** | **4.0933** | **115.3030** | **11.6839** |
| 2024 | Fixed Order | 79.4512 | 6.0142 | 144.2094 | 13.8921 |
| 2024 | Shortest Queue | 74.2294 | 4.8259 | 124.3680 | 13.1494 |

- Mean 5 seeds (Optimized): avg_total **79.9008**, avg_wait 7.1813, p90_total **129.9813**, avg_walk 11.4521; FixedOrder 90.2904 / 9.5034 / 159.7619 / 13.3650; ShortestQueue 84.7672 / 8.0753 / 148.2761 / 13.0876.
- Live seed 99: finished at `t = 408.145081`, 52/52, 188 visits, 83.4505 / 8.0682 / 157.1367 / 20.5701 / 11.1977 — bit-identical to R17/R18/R19.

**Assessment by criterion:**
- **(a) Mean p90_total over 5 seeds better than 129.9813: ❌ NOT MET** — no configuration is better; the best is a tie at 129.9813 (control/B), A2 +2.5601, A1/C +0.
- **(b) Mean avg_total not worse by more than 1%: ✅ MET** — every variant within +0.444%; A1/C +0.076%, A2 +0.368%.
- **(c) Optimized #1 on every seed and main metric: ✅ MET 10/10 cells in every variant** (A2 still #1: s42 65.68 < 71.32, p90 91.21 < 103.89; s5 117.79/118.09 < 119.38).
- **No candidate meets all 3 criteria → DISCARD per protocol.** This is a valid negative result; the final tree is unchanged from HEAD.

**Tests:**
- `cargo test`: **87/87 pass** before instrumentation and after revert. No production tests added because no logic change is kept (R18/R19 precedent).
- `#[cfg(test)] mod sweep` in `router.rs`, the two temporary helpers, and the `#[ignore]` harness in `sim.rs` were fully reverted; the final `git diff` contains only `docs/rounds/README.md`.

**Files changed:** `docs/rounds/README.md` (this section). Unchanged: `router.rs`, `congestion.rs`, `sim.rs`, `api.rs`, `main.rs`, `domain.rs`, `scenario.rs`, `stats.rs`, `learn.rs`, frontend, baselines, serving/FIFO, priority, downtime, learning, API/JSON, tie-band, lookahead, σ term.

**Verification steps:**
1. `cd backend && ~/.local/bin/cargo test` → **87/87 pass** (HEAD and after revert).
2. Baseline at HEAD: fresh `MEDIROUTE_DATA_DIR=/tmp/mediroute-r20-test`, separate server `PORT=8082` (8080 untouched), live `{strategy:optimized, speed:5000, seed:99}` → `t = 408.145081`, 52/52, 188 visits, metrics above; compare 42/7/123/5/2024 matches R17/R18/R19 number-for-number.
3. Sweep: `#[cfg(test)]` config + `#[ignore]` harness; 9 configurations, each fresh learner → live seed 99 under the variant → stats snapshot → 5 seeds × 3 strategies; control bit-identical HEAD; baselines invariant across the batch.
4. Final verify on the reverted tree: fresh `/tmp/mediroute-r20-final`, `PORT=8082`, live seed 99 → bit-identical HEAD; compare 5 seeds → table above.
5. Revert instrumentation; `git status` clean (only docs); port 8080 untouched; server 8082 stopped; **no git commit/push**.

**Decision:** **DISCARD — no code change.** The estimator-consistency hypothesis is refuted at the outcome level: 9 configurations, zero improve mean p90; A1/C are neutral to slightly worse (avg +0.076%), A2 worsens the s42 tail clearly (+12.80), B has no routing leverage. The working tree keeps only this docs section for the coordinator to cross-check.

**Lessons learned:** (1) Consistency is a property of the model, not automatically a property of the outcome: a real inconsistency can be fixed correctly (A1/B) while the result stays the same or worsens, because each cost component serves a different role. (2) Lookahead conservatism is load-bearing for the tail: capping the second hop (A2) does not "synchronize" anything, it only hides congestion at the next step, paying with p90 +12.80 on s42; conversely σ horizon has almost no leverage because queue variance does not depend on the horizon and the lull has λ = 0. (3) After 3 consecutive rounds unable to beat R17 by extending/synchronizing min-cap, the honest conclusion is that R17 has reached a good local equilibrium for the current cost model; improving p90 requires changing another axis (serving/priority/scenario), not further tuning estimators inside the same window.

## Synthesis of 20 rounds

- **20 rounds: 7 commit + 13 discard.** Commits: R1 (predictive routing + rush/lull/downtime/priority), R3 (distance-aware walk matrix), R4 (2-step lookahead), R8 (time-aware future congestion), R10 (downtime pricing), R16 (mean-variance σ), R17 (post-peak min-cap). Discards: R2, R5, R6, R7, R9, R11, R12, R13, R14, R15, R18, R19, R20.
- **Main metrics journey:** baseline commit `9ecc85b` seed 42: 101.1 / 13.4 / 146.9 (avg_total / avg_wait / p90_total; old scenario, arrivals spread evenly). After the scenario moved to two rushes + lull (R1) and the core improvements R3/R4/R8/R10/R16/R17: seed 42 at HEAD `65.0999 / 3.1666 / 78.4098`. Since the 5-seed protocol was standardized: R10-era mean `79.1642 / 135.1871` → R16 `79.1771 / 133.3764` → **R17 `79.9008 / 129.9813`** (mean avg / mean p90). Live seed 99 at HEAD: `83.4505 / 8.0682 / 157.1367`, walk `11.1977`, 188 visits. The absolute baseline → HEAD comparison includes the scenario change (R1), so the fair signal is the gap to the 2 baselines on the same arrivals: Optimized is now #1 on both avg and p90 on 5/5 seeds (10/10 cells).
- **3 biggest lessons:**
  1. **Structural change beats micro-tuning.** The biggest jump came from a realistic scenario + predictive routing + walk matrix + lookahead/future-congestion (R1/R3/R4/R8); the later pricing rounds (R10/R16/R17) only improve p90 partially and are very protocol-sensitive. 13/20 rounds being discarded is a reasonable cost for keeping the system from regressing.
  2. **The outcome benchmark is the arbiter, not calibration or theory.** R18/R19/R20 all had mechanisms that were "right" by calibration or internal consistency but did not win p90; conversely R17 won thanks to a narrow min-cap in exactly the window where bias was measured. A conclusion requires running all 5 seeds + live seed 99 + bit-identical baselines; a "nearly passing" result is not enough to commit.
  3. **Respect the closed loop and system constraints.** Changing routing changes learned stats (R16 caveat); changing serving policy can worsen p90 or help baselines asymmetrically, failing the p90/#1 criteria (R13/R14); and estimators within the same cost function have different roles (R20). Therefore every change should touch only Optimized, keep physics/FIFO/priority/downtime/learning/API intact, and verify on a fresh data dir with baselines matching number-for-number.
