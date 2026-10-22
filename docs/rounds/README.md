# MediRoute — Improvement Rounds Log

Mỗi vòng: đề xuất aspect → headless DeepSeek-Flash implement → verify (cargo test + compare seed 42) → commit nếu tốt hơn, discard nếu không.

**Baseline (commit `9ecc85b`, seed 42, comparison report):**

| Strategy | avg_total | avg_wait | p90_total |
|---|---|---|---|
| Optimized | 101.1 | 13.4 | 146.9 |
| ShortestQueue | 104.5 | 14.3 | 159.3 |
| FixedOrder | 108.0 | 15.3 | 175.7 |

Live run (seed tự sinh): 52/52 patients, avg_total 109.7, avg_wait 14.0, p90 199.1.

---

## Round 1 — Predictive routing + complex scenarios

**Status:** ✅ COMMITTED (verified independently)

**Aspect đề xuất:**
1. Smarter routing algorithm (Optimized only, baselines giữ nguyên):
   - Predictive idle-time: dự đoán thời điểm mỗi capacity slot rảnh (remaining service của in-service patients `ends_at - now`), cộng queue/pending/en-route load, downtime, weighted anti-herding arrivals; = 0 khi slot thực sự idle
   - Anti-herding lookahead: ước lượng competitors (patients eligible khác) sẽ tới cùng phòng
   - Critical-path tie-break: trong band 5% cost, ưu tiên room unblock nhiều dependents nhất, rồi dependency chain dài nhất
2. Complex scenarios:
   - Rush-hour arrivals: 65% patients trong peak chính 0–120 min, còn lại peak phụ 150–210 min, lull ở giữa
   - Room downtime: mỗi room 15% cơ hội offline 10–20 min (seeded); in-service finishes bình thường, queue giữ
   - Patient priorities: ~10% urgent; priority queue (urgent trước normal, FIFO trong class)
   - Metrics thêm: avg/p90 wait cho urgent và normal (additive, giữ field cũ)
   - UI: RoomGrid card sọc xám + countdown khi downtime, PatientTable badge URGENT, ReportView bảng "Wait by priority"

**Files changed:** backend: domain.rs, scenario.rs, engine/{congestion,router,sim,stats}.rs, main.rs (thêm PORT + MEDIROUTE_DATA_DIR env); frontend: types.ts, RoomGrid.tsx, PatientTable.tsx, ReportView.tsx; README.md

**Kết quả (seed 42, verify độc lập bằng /api/sim/compare):**

| Strategy | avg_total | avg_wait | p90_total |
|---|---|---|---|
| **Optimized (sau R1)** | **57.97** | **2.39** | **70.90** |
| ShortestQueue | 62.96 | 3.75 | 89.49 |
| FixedOrder | 67.52 | 4.99 | 114.44 |
| Optimized (baseline đầu) | 101.1* | 13.4* | 146.9* |

*Lưu ý: scenario cũng thay đổi (rush bursts 210 min thay vì arrivals trải đều), nên so sánh tuyệt đối với baseline đầu gồm cả 2 yếu tố; so sánh nội bộ 3 chiến lược trên cùng arrivals mới là tín hiệu công bằng — Optimized tốt nhất cả avg lẫn p90.

**Verify:** cargo test 41/41 pass (thêm tests: predictive-wait, anti-herding, critical-path, rush-hour, priority queue, downtime, priority metrics); npm run build OK; agent đã live-test port 8081 (isolated) tất cả endpoints.

**Decision:** COMMIT — tốt hơn rõ rệt.

---

## Round 2 — Global assignment coordination (batch assignment)

**Status:** ❌ DISCARDED (verify chuẩn hóa cho thấy avg_total regression)

**Aspect đề xuất:**
- Hiện tại router quyết định tuần tự từng patient (mỗi patient chọn phòng cost thấp nhất một mình). Khi nhiều patients cần routing trong cùng thời điểm (arrivals trùng burst, service-end trùng nhau), quyết định tuần tự gây herding cục bộ — patient sau thấy queue của patient trước vừa tăng.
- Batch assignment: nhóm tất cả patients cần routing trong cùng bước xử lý, sync-optimize phân bổ (ví dụ greedy theo cost tăng dần + mỗi assignment cập nhật room load ảo cho các assignment sau trong batch; hoặc brute-force nhỏ khi batch ≤ 6 patients, mọi hoán vị phòng eligible). Mục tiêu: giảm avg_wait và avg_total thêm khi contention cao (rush hours).

**Nguyên tắc:** chỉ cải thiện Optimized; FixedOrder/ShortestQueue giữ nguyên. Additive, không break API/UI/tests cũ.

**Thiết kế agent đã implement:** batch ≤ 6 patients & ≤ 4096 tổ hợp → exhaustive min tổng cost với spread tie-break; batch lớn hơn → greedy + virtual room-load overlay; vì event timestamps liên tục (batch-1 hầu như luôn), dùng coordination window 0.5 phút: patient hiện tại commit ngay, patients sắp routing trong 0.5 phút được plan/reserve trước, revalidate lúc commit.

**Verify chuẩn hóa (quy trình: fresh data dir → live run seed 99 → compare seed 42, lặp trên cả 2 phiên bản code):**

| Metric | Round 1 | Round 2 | Δ |
|---|---|---|---|
| avg_total | 58.42 | 59.04 | +0.62 tệ hơn |
| avg_wait | 2.51 | 2.68 | +0.17 tệ hơn |
| p90_total | 75.95 | 71.96 | −3.99 tốt hơn |
| p90_wait | 9.23 | 10.44 | +1.21 tệ hơn |

Kết quả nhất quán qua 2 mức learned stats (1 run, 2 runs). Chỉ p90_total cải thiện; avg_total/avg_wait/p90_wait đều tệ hơn nhẹ. Baselines không đổi (67.52/4.99/114.44 và 62.96/3.75/89.49).

cargo test 48/48 pass (code mới), 41/41 sau revert. Quy tắc vòng lặp: cần tốt hơn tổng thể → **DISCARD**. Code reverted (`git checkout engine/{router,congestion,sim}.rs README.md`); bản ghi này giữ lại làm tài liệu.

**Bài học:** coordination window 0.5 phút với reservations giúp tail (p90_total −4 phút) nhưng plan/recheck thêm chi phí routing làm phân tán patients khỏi lựa chọn cost-tối ưu cục bộ, tăng nhẹ avg. Nếu sau này thử lại hướng này, nên giới hạn coordination chỉ áp dụng khi contention thực sự cao (queue ≥ 2 ở ≥ 2 phòng eligible).

---

## Round 3 — Distance-aware routing (walk-time matrix)

**Status:** ✅ COMMITTED (verified độc lập)

**Aspect đề xuất:**
1. Walk-time matrix theo khoảng cách vật lý:
   - `walk_time(from, to) = 1.0 + 0.25 × euclidean(from, to)`, clamp `[1.0, 5.0]` phút
   - Mỗi room có `position` + `wing`; entrance có vị trí riêng `(0, 0)`
   - 3 wing thật: Lab (Blood Test, ECG), Imaging (X-Ray, Ultrasound), Consultation (Cardiology, Orthopedics, Endocrinology, ENT, Eye Exam)
2. Patient có `current_location` (entrance lúc arrival, cập nhật thành room vừa hoàn tất) và `total_walk_min`
3. SIM VẬT LÝ DÙNG CHUNG cho cả 3 strategies: `EnRoute` duration = `walk_time(current_location, room)` thay vì hằng số 2.0; predictive en-route load cũng dùng walk time thật
4. Chỉ Optimized đổi routing: `cost(r) = est_wait(r) + est_service(r) + walk_time(current_location, r)`; FixedOrder vẫn thứ tự package, ShortestQueue vẫn chọn queue ngắn nhất — cả hai không đọc distance/wing
5. Metrics additive: thêm `avg_walk_min` (tổng walk time trung bình mỗi patient hoàn tất), không bỏ field cũ
6. UI additive: RoomGrid hiện badge wing; bảng so sánh ReportView thêm cột Avg walk; `types.ts` thêm field

**Thiết kế & quyết định quan trọng:**
- Layout: entrance `(0,0)` đặt cạnh Imaging wing; Lab và Consultation xa hơn. Cụ thể: Blood Test `(14, 9)`, ECG `(15.5, 9)`; X-Ray `(2.5, 2.5)`, Ultrasound `(4, 2.5)`; Cardiology `(6.5, 16.5)`, Orthopedics `(8, 16)`, Endocrinology `(9.5, 16.5)`, ENT `(7, 18)`, Eye Exam `(9, 18)`.
  - Trong cùng wing: ~1.375–1.75 phút (`blood_test → ecg` = 1.375); khác wing: ~3.5–5.0 phút (`blood_test → cardiology` ≈ 4.5).
- Optimized giữ nguyên cost dự đoán congestion, nhưng thêm walk time vào cost và mở tie band lên **25%** (`COST_TIE_BAND`, trước là 5%) với tie-break **walk gần nhất trước**, rồi mới tới unblocks/critical-path. Lý do: khi wait/service chênh lệch nhỏ trong band, chọn phòng gần giúp tour gọn wing thay vì nhảy wing vì lợi ích congestion rất nhỏ.
  - Đã thử band 5% → 20% → 30% → 25%. Band 30% làm seed 123 thua ShortestQueue về p90_total (136.79 vs 126.75); 25% giữ Optimized tốt nhất avg_total + p90 trên cả 3 seed đồng thời hạ walk.
- Baselines: routing logic giữ nguyên tuyệt đối (không đọc position/wing). Walk physics áp dụng cho mọi strategy nên avg_walk của baselines phản ánh package/queue order chứ không phải tối ưu khoảng cách.

**Files changed:**
- Backend: `domain.rs`, `scenario.rs`, `engine/{router,congestion,sim,stats}.rs`, `engine/learn.rs` (chỉ test fixture)
- Frontend: `types.ts`, `components/RoomGrid.tsx`, `components/ReportView.tsx`, `App.tsx`
- Docs: `README.md` (mô tả cost/wing/tests), `docs/rounds/README.md`

**Kết quả compare (headless, cùng arrivals/packages/service samples):**

Seed 42 (verify chuẩn):

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

**Đánh giá tiêu chí verify:**
- (a) Optimized tốt nhất cả avg_total và p90_total trên cả 3 seed.
- (b) Optimized walk thấp nhất trên cả 3 seed: seed 42 −0.90/−1.02 phút, seed 7 −1.27/−0.55, seed 123 −1.29/−1.05 so với FixedOrder/ShortestQueue → router thực sự exploit distance.

Live run seed 99 (fresh data dir, speed clamp 60): 52/52 patients, sim_time 405.47 phút, avg_total 80.45, avg_wait 7.16, p90_total 160.14, p90_wait 17.94, **avg_walk 11.49**, 188 visits.

**Verify steps đã thực hiện:**
1. `cargo test`: **50/50 pass** (41 test cũ + 9 test mới: walk_time clamp/growth, wing distances, Optimized chọn phòng gần khi cost tương đương, cost tính từ current room, baselines bỏ qua distance, EnRoute dùng walk time thật, patient location tracking, avg_walk metric).
2. `npm run build`: OK (tsc + vite; chỉ còn warning chunk > 500 kB có từ trước).
3. Fresh `MEDIROUTE_DATA_DIR=/tmp/mediroute-r3-test`, chạy isolated **PORT=8082**; live sim `{strategy: optimized, speed: 5000, seed: 99}` → chờ finished.
4. `POST /api/sim/compare` seed 42, 7, 123; ghi đầy đủ avg_total/avg_wait/p90_total/avg_walk cho 3 strategies.
5. Kiểm tra `/api/rooms` trả `wing` + `position`, `/api/status` trả `avg_walk_min`, frontend serve trên 8082.
6. Không đụng port 8080 (process user pid 150786 vẫn nguyên); không git commit/push.


**Verify độc lập (coordinator):** lặp lại quy trình chuẩn trên server riêng — cargo test 50/50, npm run build OK, fresh data dir → live seed 99 (finished, sim_time 405.5) → compare seeds 42/7/123: khớp chính xác từng số với report (64.10/2.64/84.66/12.55; 72.26/4.25/128.48/12.55; 75.15/7.25/125.68/11.44). Optimized tốt nhất avg_total + p90_total + avg_walk trên cả 3 seeds; gap với baselines nới rộng so với scenario Round 1 (seed 42: 7.2 min vs 5.0 min). 

**Decision:** COMMIT — scenario vật lý thực hơn (walk 1–5 min theo khoảng cách thật giữa 3 khu) và router khai thác distance để nới rộng khoảng cách với cả 2 baselines trên mọi metric.

---

## Round 4 — 2-step lookahead routing (chỉ Optimized)

**Status:** ✅ COMMITTED (verified độc lập)

**Aspect đề xuất:**
- Optimized chỉ so cost của phòng kế tiếp. Một lựa chọn rẻ trước mắt có thể để lại bước sau đắt (khóa dependency, phòng xa, hoặc phòng còn đông khi patient quay lại).
- Thêm lookahead một bước: `cost_total(r) = walk(current, r) + est_wait(r) + est_service(r) + w × lookahead(r)`, với
  `lookahead(r) = min` over `r'` eligible **sau khi** hoàn tất `r` của `[walk(r, r') + est_wait_tại_lúc_đến(r') + est_service(r')]`.
  Nếu `r` là phòng cuối (hoặc không còn phòng eligible) → `lookahead = 0`.
- Chỉ Optimized đổi; FixedOrder/ShortestQueue tuyệt đối giữ nguyên; API/JSON không đổi.

**Thiết kế:**
- Eligibility của bước 2 được tính với `done ∪ {r}`, nên phòng bị chặn bởi dependency chỉ xuất hiện sau khi chọn đúng prerequisite (A mở khóa B → B trở thành ứng viên bước 2 của A, nhưng vẫn bị chặn nếu chọn X trước).
- Thời điểm đến phòng kế: `finish_r + walk(r, r')`, trong đó `finish_r = walk(current, r) + est_wait(r) + est_service(r)`.
- Est_wait tương lai dùng **decay**: `max(0, est_wait_hiện_tại(r') − arrive_in)`. Cách này tái dùng nguyên `congestion::est_wait_min` (không sửa congestion.rs) và phản ánh việc hàng đợi kịp trôi trong lúc patient đi/khám phòng đầu. Đã thử bản không decay: tệ hơn rõ (xem bảng chọn hệ số).
- Tie-break giữ nguyên: band 25% so trên `cost_total`, rồi walk gần → unblocks → downstream depth → en-route → queue → package position.
- `sim.rs` không phải sửa: `RouteContext` đã có `required`, `done`, `graph`; router tự thêm `r` vào tập done khi ước lượng bước 2.
- Hệ số `LOOKAHEAD_WEIGHT = 0.5` (`router.rs`). Từ w = 0.6, band 25% trên tổng cost đã nuốt khác biệt một-bước đủ rõ và làm test cũ `clear_cost_difference_outranks_critical_path_depth` lật kỳ vọng; w = 0.7 còn làm hỏng thêm test `router_uses_learned_service_time_not_config_seed`. w = 0.5 giữ nguyên toàn bộ hành vi cũ, đồng thời cho kết quả tốt nhất/dễ chấp nhận nhất trên 3 seed.

**Chọn hệ số lookahead** (cùng quy trình: fresh data dir → live seed 99 → compare 42/7/123; mỗi mức w có learned stats riêng từ live run của chính nó):

| w (decay) | seed 42 avg/p90 | seed 7 avg/p90 | seed 123 avg/p90 |
|---|---|---|---|
| 0.40 | 64.78 / 92.74 | 72.35 / 127.76 | 75.91 / 125.00 |
| 0.45 | 64.61 / 83.69 | 73.27 / 126.89 | 75.91 / 125.00 |
| **0.50** | **63.92 / 75.89** | **72.80 / 126.89** | **75.92 / 121.81** |
| 0.60 | 64.90 / 92.35 | 72.34 / 128.32 | 79.47 / 136.79 |
| 0.70 | 64.06 / 83.69 | 71.27 / 128.50 | 76.41 / 131.20 |
| 0.80 | 65.51 / 85.55 | 71.37 / 128.50 | 75.80 / 131.20 |
| 0.50 no-decay | 66.95 / 88.14 | 72.78 / 126.89 | 79.57 / 129.42 |

- **Chọn w = 0.5**: p90_total tốt nhất hoặc đồng hạng trên cả 3 seed (75.89 / 126.89 / 121.81), avg_total không seed nào xấu quá ~1% so với Round 3. Các mức 0.6–0.8 làm p90 seed 42/123 tệ hơn cả Round 3 do quyết định rời rạc nhảy sang nhánh khác; 0.4/0.45 kém ổn định ở seed 42. Bản không decay tệ hơn ở cả 3 seed. Đường cong không đơn điệu vì mỗi thay đổi nhỏ lật hẳn thứ tự routing.

**Kết quả compare (verify chuẩn, headless — cùng arrivals/packages/service samples):**

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

Baselines khớp **từng số** với Round 3 ở cả 3 seed × 4 metric (ví dụ seed 42: 77.0249/5.9134/130.2068/13.4523 và 71.3185/4.3273/103.8948/13.5720) → physics, seed, và routing của baselines không bị đụng.

**Đánh giá vs Round 3 (Optimized):**

| Seed | Δ avg_total | Δ avg_wait | Δ p90_total | Δ avg_walk |
|---|---|---|---|---|
| 42 | −0.18 (63.92 vs 64.10) | +0.09 (2.73 vs 2.64) | **−8.77** (75.89 vs 84.66) | −0.50 (12.05 vs 12.55) |
| 7 | +0.54 (72.80 vs 72.26) | +0.18 (4.43 vs 4.25) | **−1.58** (126.89 vs 128.48) | −0.16 (12.39 vs 12.55) |
| 123 | +0.77 (75.92 vs 75.15) | +0.32 (7.57 vs 7.25) | **−3.87** (121.81 vs 125.68) | −0.33 (11.10 vs 11.44) |

- Tiêu chí "tốt hơn avg_total **hoặc** p90_total mà không regression đáng kể": đạt qua **p90_total giảm trên cả 3 seed** (−1.6 đến −8.8 phút; −1.2% đến −10.4%), avg_walk giảm cả 3, avg_total gần như hòa (1 seed tốt hơn, 2 seed nhích +0.5–0.8 phút ≈ +0.75–1.0%), avg_wait nhích +0.09–0.32 phút (3–4%) — mức nhỏ so với lợi ích tail.
- Optimized vẫn thắng cả 2 baselines trên cả 4 metric ở cả 3 seed.
- Không có over-bias rõ: avg_walk tiếp tục giảm, nghĩa là lookahead không kéo tour lệch sang các wing xa.
- Lưu ý: đây là so sánh theo đúng quy trình Round 3 (compare dùng learned stats từ live seed 99 của cùng phiên bản code), nên phần chênh lệch bao gồm cả hiệu ứng feedback của stats mới; baselines độc lập với stats nên không bị ảnh hưởng.

Live seed 99 (fresh dir, speed 5000→clamp 60): finished t = 408.15 phút, 52/52, 188 visits; avg_total 80.99, avg_wait 7.29, p90_total 149.97, p90_wait 19.91, avg_walk 11.53. (Round 3: t = 405.47, 80.45/7.16/160.14/17.94/11.49 — p90 live cũng giảm.)

**Files changed:** `backend/src/engine/router.rs` (hằng số + `future_wait`/`lookahead_cost`/`total_cost` + 4 test mới). Không đổi `congestion.rs`, `sim.rs`, `domain.rs`, `scenario.rs`, API, frontend. Không commit.

**Verify steps:**
1. `cargo test`: **54/54 pass** (50 test cũ + 4 mới: lookahead = 0 khi phòng cuối; wait tương lai decay theo thời gian bước 1; dependency unlock + 2-step thắng greedy 1-step trong case dựng được; baselines không đổi lựa chọn).
2. `npm run build`: OK (chỉ warning chunk > 500 kB có từ trước).
3. Fresh `MEDIROUTE_DATA_DIR=/tmp/mediroute-r4-test`, server riêng `PORT=8082`; live `{strategy:optimized, speed:5000, seed:99}` → chờ finished.
4. `POST /api/sim/compare` seed 42 → 7 → 123; ghi đủ avg_total/avg_wait/p90_total/avg_walk cho 3 strategies.
5. Kiểm tra `/api/rooms` trả `wing` + `position`, `/api/status` trả `avg_walk_min` + metric priority, frontend serve HTTP 200 trên 8082.
6. Port 8080 không đụng; không git commit/push.

**Decision:** WIN có điều kiện — cải thiện rõ tail latency (p90_total) và walk trên cả 3 seed, giữ baselines tuyệt đối, không API/JSON change; avg_total/avg_wait nhích nhẹ ở 2 seed. Đáng giữ để verify độc lập trước khi quyết định commit.

**Verify độc lập (coordinator):** cargo test 54/54; lặp quy trình chuẩn (fresh dir → live seed 99 finished t=408.1 → compare 42/7/123): khớp chính xác từng số với report. So với Round 3: p90_total −8.77/−1.58/−3.87 phút trên cả 3 seeds; avg_total −0.18 (seed 42, tốt hơn), +0.54/+0.77 (~1%, trong ngưỡng noise); avg_walk giảm cả 3 seeds.

**Decision:** COMMIT — p90_total (metric mục tiêu) cải thiện rõ rệt trên mọi seed, seed 42 chuẩn cải thiện cả avg_total lẫn p90_total, các regression avg còn lại trong 1%.

---

## Round 5 — Queue re-routing (chỉ Optimized)

**Status:** ❌ DISCARDED (verify độc lập: p90 không đổi, avg trung tính — không đạt chuẩn 'tốt hơn')

**Aspect đề xuất:**
- Optimized chỉ chọn phòng lúc bắt đầu một visit. Sau khi vào queue, bệnh nhân không bao giờ đổi ý: phòng có thể downtime, hàng đợi có thể phình lên vì người khác nhảy vào, trong khi một phòng eligible khác lại trống.
- Cho phép Optimized đánh giá lại khi hàng đợi hiện tại "di chuyển" hoặc phòng hiện tại gặp sự cố, và chuyển sang phòng khác nếu tổng thời gian hoàn thành dự kiến tốt hơn rõ rệt.
- FixedOrder và ShortestQueue tuyệt đối giữ FIFO cứng, không re-evaluate, không switch; kết quả 3 seed phải khớp Round 4 từng số.

**Thiết kế:**
- **Trigger điểm** (chỉ Optimized, chỉ phòng bị ảnh hưởng): `QueueJoin` (sau khi enqueue + dispatch), `ServiceStart`, và `RoomDowntimeStart`. Queue tiến triển (ServiceEnd → dispatch → ServiceStart) cũng đi qua đúng các trigger này. `reevaluate_room_queue` duyệt các vị trí > 0; nếu một bệnh nhân rời queue thì index được giữ nguyên cho người kế tiếp.
- **Điều kiện:** chỉ khi đang `InQueue`, `queue_position > 0`, đúng phòng hiện tại, và chưa chạm cap. Bệnh nhân `InService` không bao giờ bị đổi; người đứng đầu queue (vị trí 0, sắp được phục vụ) cũng không đổi.
- **Chi phí ở lại:** `congestion::projected_completion_min` — station sớm nhất free tại `max(remaining service, downtime)`; chỉ `pending_starts + ahead_in_queue` job phía trước tiêu thụ station; trả về wait + service của chính bệnh nhân. Các arrival phía sau bị bỏ qua vì không thể vượt qua vị trí đã giữ.
- **So sánh full-tour:** `stay_total = tour_completion(phòng_hiện_tại, done ∪ {phòng_hiện_tại}, stay_completion)`; với mỗi phòng eligible `r'`, `target_total = walk(current_location, r') + future_wait(r') + est_service(r') + tour_completion(r', done ∪ {r'}, first_completion)`. `tour_completion` greedy toàn bộ phần tour còn lại (kể cả phòng vừa bỏ), chọn phòng eligible có finish time sớm nhất và tôn trọng dependency; nhờ vậy chi phí quay lại phòng cũ được tính đủ. Đã thử target chỉ một bước: seed 42 p90_total tăng lên 84.10 (không lookahead) và 91.86 (có lookahead); full-tour đưa p90 về đúng mức Round 4.
- Bản full-tour trừ 1 khỏi `queue_len` của phòng đang đứng khi tính target (bệnh nhân sẽ rời queue đó), tránh tính hai lần slot bị bỏ; nếu không, các ca downtime bị định giá sai và không switch.
- **Switch:** `target_total < stay_total − SWITCH_THRESHOLD_MIN`; chọn target có tổng nhỏ nhất. Khi switch: `VecDeque::remove` khỏi queue cũ (giữ nguyên FIFO của phần còn lại), tăng `switch_count` + `switches_this_visit`, đi bộ từ `current_location` (phòng hoàn tất gần nhất — không đổi khi switch) tới phòng mới, rồi enqueue vào cuối hàng (urgent vẫn được chèn trước normal như cũ). Walk đã đi về phòng cũ là sunk cost; ngưỡng threshold hấp thụ phần này.
- **Cap:** `MAX_SWITCHES_PER_VISIT = 2`, reset khi `ServiceEnd`; unit test dựng 3 downtime liên tiếp để xác nhận switch thứ 3 bị chặn.

**Chọn threshold** (learned stats cố định, cùng bộ stats cho cả 4 mức):

| Threshold | seed 42 avg/p90 | seed 7 avg/p90 | seed 123 avg/p90 | switches 42/7/123 |
|---|---|---|---|---|
| 2.0 | 64.02 / 75.89 | 72.01 / 126.89 | 75.92 / 121.81 | 1 / 2 / 1 |
| **3.0** | **64.02 / 75.89** | **72.01 / 126.89** | **75.92 / 121.81** | **1 / 2 / 1** |
| 4.0 | 64.02 / 75.89 | 72.61 / 126.89 | 75.92 / 121.81 | 1 / 1 / 0 |
| 5.0 | 64.02 / 75.89 | 72.61 / 126.89 | 75.92 / 121.81 | 1 / 1 / 0 |

- **Chọn 3.0 phút** (nằm trong khoảng gợi ý 3–5): mức 4–5 chặn mất một switch thực sự có lợi ở seed 7 (avg_total 72.61 thay vì 72.01); mức 2 cho quyết định giống hệt mức 3 nên không cần nới thêm. Trong 3 seed, không bệnh nhân nào switch quá 1 lần nên cap 2 chỉ là lưới an toàn.

**Kết quả compare (verify chuẩn, fresh dir → live seed 99 → compare 42/7/123):**

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

- Baselines khớp **từng số** với Round 4 ở cả 3 seed × 4 metric; `total_switches = 0` cho cả hai baseline.
- Optimized vs Round 4: seed 42 +0.10 avg_total (0.16%), +0.01 wait, p90 bằng, +0.06 walk; seed 7 **−0.79 avg_total** (−1.1%), **−0.26 wait**, p90 bằng, +0.18 walk; seed 123 0.00 avg_total, −0.01 wait, p90 bằng, +0.05 walk. p90_total không đổi ở cả 3 seed; avg_total tốt hơn rõ ở seed 7, các seed còn lại dao động ≤0.16%. Walk tăng nhẹ do các detour, đúng bản chất cơ chế.
- Switch stats: 4 switch / 156 bệnh nhân (0.026 switch/bệnh nhân; 4 bệnh nhân được switch, mỗi người 1 lần). Cơ chế hiếm vì chỉ tác động lên người đang chờ và không đứng đầu hàng; đây là lý do lợi ích tổng thể nhỏ nhưng không gây hại tail.

**Files changed:**
- `backend/src/engine/sim.rs`: trigger re-evaluate, `consider_switch`, `switch_patient`, cap + threshold, `switch_count`/`switches_this_visit`, trigger tests.
- `backend/src/engine/congestion.rs`: `projected_completion_min` + 2 test.
- `backend/src/engine/router.rs`: `tour_completion`, `switch_decision`, tách `lookahead_after`.
- `backend/src/domain.rs`: `Patient.switch_count`, `Patient.switches_this_visit`.
- `backend/src/engine/stats.rs`: `total_switches`, `patients_switched`, `avg_switches_per_patient`; `PatientView.switch_count` (additive JSON).
- `api.rs`, `engine/learn.rs`, `main.rs`, `scenario.rs`: chỉ reflow bởi `cargo fmt`, không đổi logic.
- Không đổi frontend, API routes, scenario, hay hành vi baselines.

**Verify steps:**
1. `cargo test`: **63/63 pass** (54 test cũ + 9 mới: 2 congestion cho projection, 7 sim cho switch downtime/below-threshold/head-of-queue/baselines/FIFO/urgent/cap).
2. `npm run build`: OK (chỉ warning chunk > 500 kB có từ trước).
3. Fresh `MEDIROUTE_DATA_DIR=/tmp/mediroute-r5-test`, server riêng `PORT=8082`; live `{strategy:optimized, speed:5000, seed:99}` → finished t = 408.15 phút, 52/52, 188 visits, 0 switch; metrics 80.99 / 7.29 / 149.97 / 19.91 / 11.53 (trùng khớp Round 4 vì seed 99 không có cơ hội switch).
4. `POST /api/sim/compare` seed 42 → 7 → 123; ghi đủ 4 metric + switch stats cho 3 strategies; xác nhận baselines khớp Round 4 từng số.
5. `/api/patients` trả `switch_count` cho cả 52 bệnh nhân; `/api/status` trả 3 metric switch; frontend serve HTTP 200 trên 8082.
6. Port 8080 không đụng; không git commit/push.

**Decision:** WIN có điều kiện — cơ chế hoạt động đúng (switch khi downtime/gain đủ lớn, giữ FIFO + priority, baselines tuyệt đối không đổi), seed 7 giảm avg_total 0.79 phút và avg_wait 0.26 phút, p90_total không đổi ở cả 3 seed. Seed 42 nhích +0.10 phút avg_total (0.16%) do một switch dự đoán có lợi nhưng thực tế không; seed 123 trung tính. Đây là mức đánh đổi nhỏ, chấp nhận được; không commit theo yêu cầu.

**Verify độc lập (coordinator):** cargo test 63/63; lặp quy trình chuẩn (fresh dir → live seed 99 finished → compare 42/7/123): khớp chính xác từng số với report. So với Round 4: p90_total KHÔNG đổi cả 3 seeds (75.89/126.89/121.81 — kỳ vọng cải thiện tail không xảy ra); avg_total +0.10/−0.79/0.00; chỉ 4 switches/156 patients.

**Decision:** DISCARD — quy tắc vòng lặp yêu cầu "tốt hơn" rõ ràng; ở đây metric mục tiêu p90 đứng yên, avg là hòa hoán nhỏ giữa các seed, và cơ chế gần như không kích hoạt (0.026 switch/patient) dù thêm ~300 dòng code. Code reverted, cargo test 54/54. Bản ghi giữ làm tài liệu.

**Bài học:** trong scenario hiện tại, patient kẹt queue hiếm khi do quyết định routing sai mà do contention tổng thể — re-routing chỉ giúp khi phòng downtime dài hoặc phép so sánh thay đổi lớn; cả hai hiếm. Nếu tăng downtime (15%→30%) hoặc thêm phòng hay hỏng, cơ chế này đáng thử lại.

---

## Round 6 — Remaining-tour rollout evaluation (chỉ Optimized)

**Status:** ❌ DISCARDED (negative result — không W nào tốt hơn Round 4; không commit)

**Aspect đề xuất:**
- Round 4 chỉ lookahead 1 bước. Tổng quát hóa thành greedy rollout toàn phần tour còn lại: với mỗi candidate `r`, giả định `r` đã xong (`position = r`, `done ∪ {r}`), lặp chọn phòng rẻ nhất tiếp theo (`walk + est_wait decay theo thời điểm tới + est_service`) cho đến khi hết required rooms.
- `rollout_cost(r)` = tổng cost tour còn lại **sau** `r` (không gồm cost tới `r`); `cost_total(r) = walk(current, r) + est_wait(r) + est_service(r) + W × rollout_cost(r)`, W sweep 0.3/0.4/0.5/0.6/0.7.
- Rollout không tính lại capacity contention của người khác (chỉ decay est_wait theo thời điểm tới), deterministic, tối đa O(rooms²) mỗi quyết định (≤ 9 phòng → ~81 phép tính/lần; ~17k phép tính/sim).
- Chỉ Optimized đổi; FixedOrder/ShortestQueue, `congestion.rs`, `sim.rs`, API/JSON, frontend giữ nguyên. Toàn bộ tie-break chain hiện có giữ nguyên.

**Thiết kế:**
- `rollout_plan` (`router.rs`): tạo `tour_done = done ∪ {r}`, `location = r`, `elapsed = walk_to(r) + wait_at(r) + est_service(r)`. Mỗi vòng lọc required chưa làm và eligible theo dependency, chọn min `walk(location, next) + future_wait(next, elapsed + walk) + est_service(next)`, cộng vào tổng, tăng `elapsed`, cập nhật `location`/`tour_done`. Trả `(total, path)` — path dùng cho test.
- `rollout_cost` = phần total; production `total_cost` dùng `ROLLOUT_WEIGHT`. Lookahead 1 bước cũ giữ dưới `#[cfg(test)]` (`lookahead_cost`/`one_step_total_cost`) làm fallback đối chiếu.
- Tie-band 25% và thứ tự walk gần → unblocks → downstream depth → en-route → queue → package position giữ nguyên, áp trên `cost_total` mới.
- Sweep W: mỗi mức có fresh data dir → live seed 99 → compare 42/7/123, nên learned stats của từng mức là riêng của chính nó (đúng quy trình Round 3/4).

**Bảng sweep W (Optimized, headless compare; TB = trung bình 3 seed):**

| W | seed 42 avg/p90 | seed 7 avg/p90 | seed 123 avg/p90 | TB avg/p90 | walk TB |
|---|---|---|---|---|---|
| R4 1-step (0.5) | 63.92 / 75.89 | 72.80 / 126.89 | 75.92 / 121.81 | 70.88 / 108.20 | 11.85 |
| **0.30** | **64.28 / 82.08** | **75.25 / 127.75** | **80.55 / 121.04** | **73.36 / 110.29** | **11.24** |
| 0.40 | 64.88 / 86.18 | 74.31 / 124.92 | 80.78 / 127.95 | 73.32 / 113.02 | 10.94 |
| 0.50 | 66.43 / 92.86 | 74.65 / 124.92 | 81.58 / 127.95 | 74.22 / 115.24 | 10.69 |
| 0.60 | 66.96 / 86.18 | 74.80 / 124.92 | 81.58 / 127.95 | 74.45 / 113.02 | 10.59 |
| 0.70 | 68.82 / 95.94 | 74.66 / 136.82 | 83.04 / 127.95 | 75.51 / 120.24 | 10.49 |

- Chọn **W = 0.3** để verify cuối: p90 trung bình tốt nhất trong sweep (110.29), avg gần như đồng hạng với 0.4 (73.36 vs 73.32), thắng 0.4 ở 2/3 seed trên cả avg lẫn p90, và là mức thấp nhất còn giữ toàn bộ test cũ xanh.
- Xu hướng rõ: W tăng → walk giảm đều (11.24 → 10.49) nhưng avg/p90 xấu đi. Rollout thực chất tối ưu hình học tour; càng tin vào nó càng bỏ qua congestion tương lai.
- Lưu ý test: tại W ≥ 0.5, test cũ `clear_cost_difference_outranks_critical_path_depth` lật kỳ vọng. Rollout toàn tour cộng thêm phần tour chung vào cả hai phương án, làm khoảng cách tương đối co vào band 25% (ví dụ 47.5 vs 37.5 → 21%), nên tie-break critical-path chọn `a`. W = 0.3/0.4 pass 59/59.

**Kết quả compare W=0.3 (verify chuẩn: fresh dir → live seed 99 → compare 42/7/123):**

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

Baselines khớp **từng số** với Round 4/3 ở cả 3 seed × 4 metric → physics, seed và routing của baselines không bị đụng.

**Đánh giá vs Round 4 (Optimized):**

| Seed | Δ avg_total | Δ avg_wait | Δ p90_total | Δ avg_walk |
|---|---|---|---|---|
| 42 | +0.36 (+0.6%) | +0.21 | +6.19 (+8.2%) | −0.43 |
| 7 | +2.45 (+3.4%) | +0.82 | +0.86 (+0.7%) | −0.68 |
| 123 | +4.63 (+6.1%) | +1.56 | −0.77 (−0.6%) | −0.71 |

- Không đạt tiêu chí "tốt hơn avg_total **hoặc** p90_total mà không regression đáng kể": avg_total tệ hơn cả 3 seed (seed 7/123 vượt ngưỡng 1–2%), p90 chỉ tốt hơn 0.77 phút ở seed 123 nhưng tệ hơn +6.19 ở seed 42 và +0.86 ở seed 7.
- avg_walk giảm đều 0.43–0.71 phút trên cả 3 seed: rollout thực sự kéo tour về gần nhau, nhưng đánh đổi bằng wait/tail. Nguyên nhân: rollout bỏ qua contention tương lai và decay est_wait theo thời điểm tới, nên các phòng đông "trong tương lai" bị định giá gần 0; W càng cao, quyết định càng bị kéo về tour ngắn thay vì né queue.
- Live seed 99 (fresh dir, speed 5000→clamp 60), W=0.3: finished 409.81 phút, 52/52, 188 visits; 82.33 / 7.99 / 156.66 / 23.02 / **10.38** (avg_total/avg_wait/p90_total/p90_wait/avg_walk) vs Round 4 80.99 / 7.29 / 149.97 / 19.91 / 11.53 → chỉ walk tốt hơn.

**Files changed:** `backend/src/engine/router.rs` (ROLLOUT_WEIGHT + `rollout_plan`/`rollout_cost`/`tour_total_cost`; lookahead cũ giữ cfg(test); 5 test mới), `docs/rounds/README.md`. Không đổi `congestion.rs`, `sim.rs`, `domain.rs`, `scenario.rs`, API, frontend. Không git commit/push; port 8080 không đụng.

**Verify steps:**
1. `cargo test`: **59/59 pass** (54 test cũ + 5 mới: rollout = 0 khi là phòng cuối và equivalence lookahead; rollout không chọn phòng chưa unlock; rollout ưu tiên phòng gần trong tour còn lại; equivalence khi chỉ còn 1 visit sau candidate; W=0 → immediate cost).
2. `npm run build`: OK (chỉ warning chunk > 500 kB có từ trước).
3. Fresh `MEDIROUTE_DATA_DIR=/tmp/mediroute-r6-test`, server riêng `PORT=8082`; live `{strategy:optimized, speed:5000, seed:99}` → finished 409.81 phút, 52/52, 188 visits.
4. `POST /api/sim/compare` seed 42 → 7 → 123; ghi đủ 4 metric cho 3 strategies; xác nhận baselines khớp Round 4 từng số.
5. Sweep W lặp lại đúng quy trình trên cho từng mức (fresh dir riêng `/tmp/mediroute-r6-w0*`).

**Decision:** DISCARD (negative result) — không W nào vượt Round 4 trên avg_total hoặc p90_total mà không regression đáng kể; đây là negative result trung thực, không phải lỗi triển khai (rollout deterministic, dependency-valid, baselines nguyên vẹn). W=0.3 là mức tốt nhất của sweep nhưng vẫn kém R4 ở avg cả 3 seed và p90 2/3 seed. Code giữ ở working tree với W=0.3 để coordinator verify độc lập; đề xuất revert nếu không có hướng điều chỉnh khác (ví dụ mô phỏng contention trong rollout, hoặc chỉ dùng rollout khi queue thấp).


**Verify độc lập (coordinator):** cargo test 59/59; lặp quy trình chuẩn (fresh dir → live seed 99 finished → compare seed 42): khớp chính xác (64.28/2.94/82.08/11.62). So với Round 4 seed 42: avg_total +0.36, p90_total +6.19 — regression rõ. Code reverted, cargo test 54/54.

**Bài học:** rollout toàn tour tối ưu hình học (walk giảm đều 0.4–0.7 phút) nhưng est_wait decay về ~0 trong tương lai xa khiến nó bỏ qua contention tương lai — đẩy wait và tail lên. Lookahead 1 bước (R4) là điểm cân bằng tốt: đủ xa để tránh dead-end, đủ gần để est_wait còn ý nghĩa. Hướng này nên dừng trừ khi mô hình được contention tương lai tốt hơn.

---

## Round 7 — Priority-aware wait estimation (chỉ Optimized)

**Status:** ❌ DISCARDED (negative result — avg_total và p90_total đều tệ hơn Round 4 ở cả 3 seed; không commit)

**Aspect đề xuất:**
- `est_wait` hiện coi mọi patient như nhau (gần-FIFO), nhưng:
  - (a) urgent đang quyết định sẽ nhảy trước mọi normal đang chờ → wait thật của họ chỉ gồm in-service remaining + urgent phía trước, không phải toàn bộ queue. Router ước lượng quá cao cho urgent nên họ có thể né phòng thực ra tốt cho mình.
  - (b) normal sẽ bị urgent arrivals (tương lai gần) nhảy trước → wait thật cao hơn `est_wait` hiện tại một chút; anti-herding đã đếm arrivals nhưng chưa tính hiệu ứng nhảy queue.
- Tách `est_wait` theo class của patient đang quyết định, dùng cho cả cost hiện tại lẫn lookahead 1 bước. Không đổi priority queue trong `sim.rs`, baselines, API/JSON, frontend, learning.

**Thiết kế:**
- `RoomLoad` thêm (additive, default 0): `urgent_queue_len`, `urgent_en_route`, `urgent_arrival_rate_per_min`. `sim.rs` đếm urgent trong queue/en-route và ghi timestamp mỗi lần urgent enqueue (`urgent_join_times`); cơ chế chèn priority không đổi.
- `congestion::est_wait_urgent_min`: `projected_wait` với jobs = `pending_starts` (đã được dequeue trước khi newcomer kịp join, nên cản mọi class) + `urgent_queue_len` + `urgent_en_route`. In-service remaining của mọi class vẫn cản vì visit không bị gián đoạn; downtime vẫn cộng. Normal queue/en-route bị bỏ qua; likely arrivals cũng bỏ qua (giữ đơn giản, có thể underestimate khi nhiều urgent cùng tới).
- `congestion::est_wait_normal_min`: lấy nguyên `est_wait_min` (FIFO + likely arrivals) làm base, cộng **expected urgent jumpers**. λ của từng phòng = số urgent enqueue trong 30 phút sim gần nhất / 30 (`URGENT_RATE_WINDOW_MIN`; đầu run λ tự nhiên thấp). `jumpers = ceil(λ × base_wait)`; cộng `jumpers` vào extra jobs của projected wait. Kết quả luôn ≥ generic (test xác nhận).
- `router.rs`: `next_room` nhận `PatientPriority`; `wait_at`/`future_wait`/`lookahead_cost`/`total_cost` dùng đúng class, kể cả phòng kế trong lookahead 1 bước. Baselines nhận tham số nhưng không đọc (tests xác nhận lựa chọn y hệt).
- `RoomView` thêm `est_wait_urgent_min`, `est_wait_normal_min` (additive JSON); không thêm metric mới, frontend không đổi.
- File changed: `backend/src/engine/congestion.rs`, `router.rs`, `sim.rs` (instrumentation load + RoomView); `docs/rounds/README.md`. Không đổi `stats.rs`, `api.rs`, `scenario.rs`, `domain.rs`, frontend.

**Tests:** `cargo test` **63/63 pass** (54 cũ + 9 mới: 6 congestion — urgent bỏ normal queue/en-route, urgent chỉ đếm urgent competitors, normal cộng jumpers, normal ≥ generic, idle/downtime; 2 router — urgent chọn phòng normal né, baselines bỏ qua priority; 1 sim — split queue/en-route + λ). `npm run build` OK (chỉ warning chunk > 500 kB có từ trước).

**Kết quả compare (verify chuẩn: fresh `/tmp/mediroute-r7-test` → live seed 99 speed 5000→clamp 60 → compare 42/7/123; 8 metric):**

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

Baselines khớp **từng số** với Round 4 ở cả 3 seed × 4 metric (77.02/5.91/130.21/13.45; 71.32/4.33/103.89/13.57; 84.39/7.08/138.89/13.82; 77.82/5.56/139.75/13.10; 86.44/10.17/145.23/12.73; 81.09/8.67/126.75/12.49) và cả split urgent/normal.

**Đo split Round 4 từ HEAD** (cùng quy trình, worktree sạch tại commit R4):

| Seed | avg_wait_urgent | p90_wait_urgent | avg_wait_normal | p90_wait_normal |
|---|---|---|---|---|
| 42 | 1.38 | 6.55 | 2.93 | 10.07 |
| 7 | 1.17 | 3.90 | 4.75 | 15.82 |
| 123 | 2.52 | 8.17 | 8.10 | 24.62 |

**Đánh giá vs Round 4 (Optimized):**

| Seed | Δ avg_total | Δ avg_wait | Δ p90_total | Δ avg_walk | Δ urg avg/p90 | Δ norm avg/p90 |
|---|---|---|---|---|---|---|
| 42 | +2.02 (+3.2%) | +0.46 | **+18.54 (+24.4%)** | +0.33 | −0.36 / **−2.39** | +0.59 / +1.46 |
| 7 | +0.64 (+0.9%) | +0.17 | +1.76 (+1.4%) | 0.00 | +0.42 / +0.37 | +0.15 / +1.45 |
| 123 | +3.35 (+4.4%) | +0.90 | **+18.44 (+15.1%)** | +0.26 | +0.48 / −0.38 | +0.95 / −0.14 |

- Không đạt tiêu chí "tốt hơn avg_total **hoặc** p90_total mà không regression đáng kể": cả 3 seed đều tệ hơn avg_total; p90_total tệ hơn ở cả 3 (2 seed tệ hơn ~15–24%). Chỉ urgent p90 seed 42 cải thiện rõ (−2.39) nhưng đánh đổi bằng normal và tổng.
- Live seed 99: finished 408.15 phút, 52/52, 188 visits; **81.03 / 7.35 / 143.03 / 19.91 / 11.37**, urgent 4.18/8.22, normal 7.59/20.78 (R4: 80.99/7.29/149.97/19.91/11.53; split đo lại: urgent 4.20/9.90, normal 7.52/20.78). Live p90_total giảm 6.94 nhưng compare 3 seed lại tệ hơn — seed 99 là ngoại lệ có lợi, không phải xu hướng.

**Chẩn đoán (các biến thể, mỗi mức fresh dir + live seed 99 + compare 42/7/123):**

| Variant | seed 42 avg/p90 | seed 7 avg/p90 | seed 123 avg/p90 |
|---|---|---|---|
| R4 (tham chiếu) | 63.92 / 75.89 | 72.80 / 126.89 | 75.92 / 121.81 |
| urgent-only (normal dùng generic) | 63.79 / 83.69 | 73.06 / 128.20 | 75.92 / 121.81 |
| normal-only (urgent dùng generic) | 66.32 / 94.43 | 73.18 / 128.40 | 79.27 / 140.25 |
| full + jumpers floor | 64.02 / 92.73 | 73.06 / 128.20 | 75.92 / 121.81 |
| full + jumpers ceil (final) | 65.94 / 94.43 | 73.44 / 128.65 | 79.27 / 140.25 |

- Chạy R7 router với **stats học được của R4** cho kết quả giống hệt R7 stats (65.94/3.19/94.43/12.38; 73.44/4.60/128.65/12.39; 79.27/8.47/140.25/11.36) → regression thuần do routing, không phải feedback của learning.
- Urgent-only (bỏ term jumpers) gần như hòa R4 về avg nhưng p90 vẫn tệ hơn 2 seed; normal-only một mình tái hiện gần đủ mức p90 tệ (94.43 / 140.25) → **term normal jumpers là nguồn regression lớn nhất**, còn class-aware urgent tự nó cũng không cải thiện tổng.
- Nguyên nhân: (a) khi urgent bỏ qua normal queue, urgent lao vào các phòng nhiều normal — đúng là urgent được phục vụ nhanh hơn nhưng normal bị đẩy chậm; ước lượng cũ (đếm cả normal queue) vô tình đóng vai trò san tải và có lợi hơn cho tổng. Tiền đề "urgent né phòng thực ra tốt cho họ" không đúng trong scenario này (urgent-only làm urgent p90 seed 42 tăng 1.71 vs 1.38). (b) term jumpers (ceil) phạt mọi phòng có urgent history thêm 1–2 full service (~10–30 phút), đủ để lật nhiều quyết định normal sang tour khác; seed 42 điển hình: 5 bệnh nhân General Checkup kết tour tại ENT với wait 20–30 phút (R4: 7–11) → p90_total 94.43. Floor làm normal p90 gần R4 hơn nhưng vẫn không cứu được tail.
- Không có over-bias rõ về walk: seed 42/7 walk tăng nhẹ (+0.33/0.00), seed 123 +0.26.

**Verify steps:**
1. `cargo test`: 63/63 pass; `npm run build`: OK.
2. Fresh `MEDIROUTE_DATA_DIR=/tmp/mediroute-r7-test`, server riêng `PORT=8082`; live `{strategy:optimized, speed:5000, seed:99}` → finished t = 408.15 phút, 52/52, 188 visits.
3. `POST /api/sim/compare` seed 42 → 7 → 123; ghi đủ 8 metric cho 3 strategies; baselines khớp R4 từng số; `/api/rooms` trả thêm `est_wait_urgent_min`/`est_wait_normal_min`.
4. Biến thể: lặp quy trình với fresh dir riêng cho urgent-only, normal-only, full-floor; đo R4 split bằng worktree sạch tại HEAD.
5. Port 8080 không đụng; không git commit/push.

**Decision:** DISCARD (negative result) — priority-aware wait estimation không cải thiện tổng thể: avg_total tệ hơn cả 3 seed, p90_total tệ hơn 1.4–24.4%, chỉ urgent p90 seed 42 tốt hơn. Code giữ ở working tree (uncommitted) để coordinator verify độc lập; đề xuất revert nếu không có hướng điều chỉnh khác (ví dụ discount một phần normal queue thay vì bỏ hẳn, hoặc chỉ áp dụng term jumpers khi contention thực sự cao).

**Bài học:** trong scenario này ưu tiên là cơ chế queue, không phải tín hiệu routing. Bỏ qua normal queue khiến urgent herding vào phòng đông và chuyển chi phí sang normal; ước lượng "sai" cũ lại là một dạng điều tiết tải có lợi. Muốn khai thác priority trong routing cần mô hình hóa **chi phí ngoại ứng lên normal** (không chỉ wait của urgents), thay vì chỉ sửa wait estimate của từng class.

**Verify độc lập (coordinator):** cargo test 63/63; lặp quy trình chuẩn (fresh dir → live seed 99 finished → compare seed 42): khớp chính xác (65.94/94.43 — regression vs R4 63.92/75.89). Code reverted, cargo test 54/54.

**Decision:** DISCARD — p90_total regression +18.54 (seed 42) / +18.44 (seed 123). Chẩn đoán của agent xác nhận: urgent-only estimate gần hòa nhưng normal-only tái hiện phần lớn regression; bỏ normal queue khỏi ước lượng urgent khiến urgent herding vào phòng đông, chuyển chi phí sang normal (5 patients General Checkup seed 42 kết tour tại ENT với wait 20–30 min).

**Bài học:** tách ước lượng wait theo class chỉ an toàn khi lượng urgent đủ nhỏ để effect bậc hai; ở đây ~10% urgent đủ để sai lệch ước lượng normal (jumpers) bẻ cong tour của 90% patients. Nếu thử lại: chỉ sửa est_wait_urgent (giữ normal = generic), bỏ term jumpers.

---

## Round 8 — Time-aware future congestion model (chỉ Optimized)

**Status:** ✅ COMMITTED (verified độc lập 5 seeds — mean tốt hơn Round 4 cả avg_total lẫn p90_total)

**Aspect đề xuất:**
- Chẩn đoán từ Round 6: future-wait dạng decay tuyến tính `max(0, est_wait − arrive_in)` mù với contention tương lai. Trong rush, wait không giảm mà có thể tăng vì arrivals mới vượt capacity drain; trong lull/end-of-day thì giảm nhanh.
- Thay decay bằng mô hình demand tĩnh: `future_wait(room, h)` dựng lại station backlog hiện tại, rồi đổ thêm expected arrivals từ arrival schedule của scenario trong khoảng `(now, now + h]`, trừ dần bằng capacity (kể cả downtime), và trả wait của một patient đến sau `h` phút.
- Chỉ dùng cho lookahead bước 2 của Optimized (giữ nguyên cấu trúc 1 bước của Round 4, không rollout toàn tour). `wait_at` hiện tại, baselines, priority queue, API/JSON và frontend không đổi.

**Thiết kế:**

*Demand model (`scenario.rs`, offline, không dynamic learning):*
- `ArrivalDemand::from_scenario(scenario, N)` với `N = patient_count`:
  - `λ_hosp(t) = 0.65·N / 120` cho `t ∈ [0, 120)`; `0` cho `[120, 150)`; `0.35·N / 60` cho `[150, 210)`; `0` sau `210` (đúng rush profile: 65% trong 0–120, 35% trong 150–210, lull 120–150).
  - Độ phổ biến phòng: `share(r) = Σ_{package p chứa r} weight(p) / Σ_p weight(p)`.
  - `λ_r(t) = share(r) · λ_hosp(t)`; expected arrivals trên `[a, b]` là tích phân từng khung: `E_r(a,b) = Σ_segments overlap(a,b,segment) · rate_segment · share(r)`. Tổng arrivals toàn viện qua 2 cửa sổ = `N` (test bảo toàn).
- Ví dụ demo: `share(blood_test) = 0.80`, `share(ent) = 0.45`, `share(ultrasound) = 0.30`.

*Future wait (`congestion.rs`):*
- Với phòng capacity `C`, service estimate `s`, downtime `d`:
  - Khởi tạo `C` station free-times: `q_i = max(in_service_remaining_i, d)`; station chưa dùng = `d`.
  - Đổ các job đã committed (`pending_starts + queue_len + en_route`) với ready = 0 theo list scheduling: luôn chọn station sớm nhất, `q = max(q, ready) + s`.
  - Expected arrivals trong `(now, now+h]` chia slice 1 phút: slice tại offset `k` có `e_k` arrivals → đổ `e_k` job (fractional) với `ready = k`, work `= e_k · s`. Anti-herding `likely_arrivals` (competitor sẽ rảnh tại `ready_in`) cũng được đổ tại đúng `ready_in ≤ h`, weight tương ứng — không double-count vì họ chưa nằm trong queue hiện tại.
  - `future_wait(room, h) = max(0, min_i q_i − h)`.
- Khi `λ = 0` (lull 120–150, sau 210, hoặc test không demand) model thoái hóa đúng về `max(0, wait_hiện_tại − h)` — tương thích hành vi Round 4.
- Trong rush, nếu `λ_r · s > C` thì backlog tương lai tăng tuyến tính; ví dụ Orthopedics: `share = 0.30`, `λ_peak = 0.65·52/120 ≈ 0.282/min` → `λ_r ≈ 0.085/min`, `× 16 min service = 1.35 > C = 1` nên phòng đang trống vẫn tích queue khi horizon dài ra.
- `future_wait` chỉ thay thế `future_wait` của lookahead; `wait_at` (chi phí phòng kế tiếp) giữ nguyên `congestion::est_wait_min`.

*Router (`router.rs`):*
- `RouteContext` thêm `demand: Option<&ArrivalDemand>` và `now_min: f64` (additive, test cũ dùng `None`/`0.0`).
- `lookahead(r) = min_{r' eligible sau r} [walk(r,r') + future_wait(r', arrive_in) + est_service(r')]`, `arrive_in = walk(current,r) + wait_at(r) + est_service(r) + walk(r,r')` — đúng thời điểm patient thực sự tới `r'`.
- `LOOKAHEAD_WEIGHT` giữ `0.5`; tie-band 25% và tie-break giữ nguyên.

**Chọn hệ số lookahead** (mỗi mức fresh data dir + live seed 99 + compare 42/7/123, learned stats riêng của chính nó; bảng là Optimized avg_total / p90_total):

| w | seed 42 | seed 7 | seed 123 |
|---|---|---|---|
| 0.40 | 64.83 / 82.62 | 72.80 / 126.47 | 76.78 / 130.16 |
| 0.45 | 66.19 / 87.86 | 73.13 / 128.20 | 76.91 / 129.34 |
| **0.50** | **64.07 / 77.79** | **73.13 / 128.20** | **72.29 / 116.37** |
| 0.55 | 65.26 / 80.53 | 73.70 / 128.20 | 80.72 / 122.27 |
| 0.60 | 64.38 / 83.14 | 73.31 / 126.89 | 80.13 / 134.62 |
| 0.50 bỏ likely arrivals | 62.86 / 75.66 | 72.70 / 128.30 | 78.56 / 142.34 |
| 0.50 max(decay cũ, model) | 64.07 / 77.79 | 73.13 / 128.20 | 72.29 / 116.37 |

- **Chọn w = 0.5**: tốt nhất ở seed 42/123 cả avg_total lẫn p90_total; seed 7 chỉ kém mức tốt nhất ~0.5% avg / ~1.4% p90. w = 0.55/0.60 kéo seed 123 tệ đi rõ (avg 80.7/80.1); w = 0.40/0.45 tốt seed 7 nhưng p90 seed 42/123 kém.
- Bỏ likely arrivals (chỉ demand λ) cải thiện seed 42/7 nhưng p90 seed 123 vọt lên 142.34 (+16.9% vs Round 4) → giữ likely arrivals tại đúng ready time.
- Bọc `max(decay cũ, model)` cho kết quả **bit-identical** với model thuần trên cả 3 seed: trong scenario này model đã ≥ decay tại các horizon dùng; regression seed 42 không phải do định giá quá cao so với decay mà do áp lực rush đổi thứ hạng bước 2. Giữ model thuần cho đơn giản.

**Kết quả compare (verify chuẩn, headless):**

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

Baselines khớp **từng số** với Round 4 ở cả 3 seed × 4 metric (77.0249/5.9134/130.2068/13.4523; 71.3185/4.3273/103.8948/13.5720; 84.3898/7.0846/138.8932/13.8233; 77.8194/5.5577/139.7541/13.0962; 86.4409/10.1679/145.2265/12.7288; 81.0868/8.6726/126.7505/12.4931) → physics, seed và routing của baselines không bị đụng.

**Đánh giá vs Round 4 (Optimized):**

| Seed | Δ avg_total | Δ avg_wait | Δ p90_total | Δ avg_walk |
|---|---|---|---|---|
| 42 | +0.15 (+0.23%) | +0.18 (+6.78%) | +1.90 (+2.50%) | −0.54 (−4.47%) |
| 7 | +0.33 (+0.46%) | +0.25 (+5.67%) | +1.31 (+1.03%) | −0.63 (−5.05%) |
| 123 | −3.63 (−4.78%) | −0.97 (−12.85%) | −5.44 (−4.47%) | −0.29 (−2.65%) |
| **Mean 3 seed** | **−1.05 (−1.48%)** | **−0.18 (−3.65%)** | **−0.75 (−0.69%)** | **−0.49 (−4.11%)** |

- Theo mean 3 seed, cả 4 metric đều tốt hơn Round 4; lợi ích tập trung ở seed 123 (avg_total −4.78%, p90_total −4.47%, avg_wait −12.85%), nơi rush contention mạnh nhất.
- Theo từng seed: seed 123 đạt rõ tiêu chí; seed 7 p90 +1.03% nằm trong ngưỡng 1–2% và avg +0.46% coi như hòa; seed 42 p90 +2.50% **hơi vượt ngưỡng 2%** và avg +0.23% (trong noise). Vì vậy không tuyên bố win tuyệt đối — đây là win tổng hợp có điều kiện.
- Optimized vẫn thắng cả 2 baselines trên cả 4 metric ở cả 3 seed.
- avg_wait tăng 5.7–6.8% ở seed 42/7 nhưng avg_total gần như không đổi và walk giảm 4.5–5.1%: routing đổi sang tour ngắn hơn, ít tail hơn; tail chỉ xấu nhẹ ở seed 42.
- Không có dấu hiệu over-bias: avg_walk giảm cả 3 seed.

Live seed 99 (fresh dir, speed 5000→clamp 60): finished t = 399.75 phút, 52/52, 188 visits; **78.01 / 6.61 / 146.60 / 16.47 / 11.05** (avg_total/avg_wait/p90_total/p90_wait/avg_walk). Round 4: 80.99 / 7.29 / 149.97 / 19.91 / 11.53 → live cũng tốt hơn ở cả 5 metric (avg_total −2.98, p90_total −3.37, p90_wait −3.44, walk −0.48).

**Files changed:** `backend/src/scenario.rs` (`ArrivalSegment`/`ArrivalDemand` + 2 test), `backend/src/engine/congestion.rs` (`future_wait_min` + `push_job` + 6 test), `backend/src/engine/router.rs` (`RouteContext.demand/now_min`, `future_wait` dùng model, 2 test mới; 17 test literal thêm field, không đổi hành vi), `backend/src/engine/sim.rs` (World dựng `ArrivalDemand` và truyền vào RouteContext), `docs/rounds/README.md`. Không đổi `wait_at`/predictive wait hiện tại, FixedOrder/ShortestQueue, priority queue, `domain.rs`, `stats.rs`, `learn.rs`, API/JSON, frontend.

**Verify steps:**
1. `cargo test`: **64/64 pass** (54 test cũ + 10 mới: 2 scenario — bảo toàn N + lull, room share theo package; 6 congestion — no-demand khớp decay cũ, tăng trong rush, về 0 lúc lull/sau 210, phản ánh room share, likely arrivals đúng ready time, downtime; 2 router — lookahead dùng đúng `arrive_in` + future_wait tăng theo horizon, baselines không đổi lựa chọn).
2. `npm run build`: OK (chỉ warning chunk > 500 kB có từ trước).
3. Fresh `MEDIROUTE_DATA_DIR=/tmp/mediroute-r8-test`, server riêng `PORT=8082`; live `{strategy:optimized, speed:5000, seed:99}` → finished 399.75 phút, 52/52, 188 visits.
4. `POST /api/sim/compare` seed 42 → 7 → 123; ghi đủ 4 metric cho 3 strategies; xác nhận baselines khớp Round 4 từng số.
5. Sweep w lặp quy trình với fresh dir riêng `/tmp/mediroute-r8-w0*`; biến thể bỏ likely arrivals và `max(decay, model)`.
6. Port 8080 không đụng; không git commit/push.

**Decision:** WIN TỔNG HỢP CÓ ĐIỀU KIỆN — không commit (theo yêu cầu round). Mean 3 seed cải thiện cả avg_total, avg_wait, p90_total và avg_walk; baselines tuyệt đối không đổi; API/JSON không đổi. Điểm trượt nhẹ nếu áp tiêu chí per-seed nghiêm ngặt: seed 42 p90_total +2.50% (>2%), trong khi seed 7 p90 +1.03% và seed 123 cải thiện mạnh. Code giữ ở working tree (uncommitted) để coordinator verify độc lập.

**Bài học:** xác nhận chẩn đoán Round 6 — decay tuyến tính bỏ qua áp lực arrivals là sai hệ thống trong rush; mô hình λ + station drain cho future-wait tăng/giảm đúng pha và mang lại lợi ích tail lớn ở seed contention cao (seed 123) mà không đổi predictive wait hiện tại. Lợi ích không đồng đều giữa các seed và khá nhạy với cách xử lý competitor (likely arrivals): bỏ chúng tốt cho seed 42 nhưng phá tail seed 123. Hướng tiếp theo nếu cần ổn định per-seed: threshold hóa model theo mức vượt capacity (`λ_r · s` so với `C`) thay vì luôn cộng expected arrivals, hoặc blend có trọng số với decay cũ — nhưng `max(decay, model)` đã thử và bit-identical, nên cần cơ chế khác chứ không chỉ clamp.

**Verify độc lập (coordinator) — mở rộng 5 seeds:** cargo test 64/64; lặp quy trình chuẩn trên cả R8 (working tree) và R4 (worktree HEAD): fresh dir → live seed 99 → compare seeds 42/7/123/5/2024. Kết quả: R8 thua nhỏ ở seeds nhàn (42: +0.15 avg/+1.90 p90; 7: +0.33/+1.31; 2024: +0.83 avg nhưng p90 −1.76) nhưng thắng lớn ở seeds congested (123: −3.63/−5.44; **5: −0.72/−37.00**). Mean 5 seeds: avg_total 79.95→79.34 (−0.61), p90_total 143.39→135.19 (−8.20). Loại seed 5, mean 4 seeds vẫn tốt hơn cả hai metric.

**Decision:** COMMIT — mean tốt hơn trên cả avg_total lẫn p90_total qua 5 seeds; cải thiện lớn nhất đúng vùng mục tiêu (high contention), regression chỉ ở seeds nhạn dưới 2.5% p90. Mô hình future congestion khắc phục đúng điểm mù được chẩn đoán ở Round 6 (wait decay về 0).

---

## Round 9 — Adaptive EWMA learning rate (chỉ learning, không đổi routing)

**Status:** ❌ DISCARDED (negative end-to-end — không commit)

**Aspect đề xuất:**
- Learning hiện tại (từ Round 1) dùng EWMA **α = 0.3 cố định** cho mọi room, mọi lúc. Hệ quả kép:
  - **Cold start chậm**: `est_service` khởi tạo từ seed config, lệch có chủ đích so với true mean (demo: config 10 phút, true mean 13 phút). Với α = 0.3, sau 5 mẫu estimate mới đạt ~12.50 (còn lệch 0.50); routing trong ~10–15 phút đầu dựa trên estimate sai.
  - **Steady state nhiễu**: service sample dao động ±20% quanh true mean; α = 0.3 giữ estimate dao động theo từng sample, làm chi phí `est_service` trong cost function nhấp nhô và tạo route churn.
- Đề xuất α thích ứng theo `visit_count` của từng room: **cao khi ít mẫu** (hội tụ nhanh từ cold start), **giảm dần khi nhiều mẫu** (giảm variance steady state):
  `α(n) = max(α_min, α_init / (1 + n/k))`.
- Chỉ đổi `learn.rs` (+ field diagnostic); **không đổi** router logic, congestion, sim physics, baselines, priority queue, API/JSON (chỉ thêm field additive vào room view) và frontend.

**Thiết kế:**

*`learn.rs`:*
- `EwmaConfig { alpha_init, alpha_min, decay_k }`, default **0.7 / 0.1 / 10**; `alpha(n) = clamp(alpha_init/(1+n/k), alpha_min, alpha_init)`.
  - Với default: `α(0)=0.70`, `α(1)=0.636`, `α(5)=0.467`, `α(10)=0.35`, `α(20)=0.233`, `α(36)=0.152`, `α(60)=0.10` (chạm floor), giữ `0.10` cho mọi `n ≥ 60`.
  - α được áp dụng đồng thời cho `est_service_min`, `est_wait_observed_avg` và diagnostic mới.
- `Learner` giữ `EwmaConfig`; `new()` dùng default, thêm `with_ewma()` (cho sweep/test) và `current_alpha(n)` (cho room view lúc idle).
- Diagnostic **`avg_abs_service_dev_min`** = EWMA của `|service_sample − est trước khi update|`, dùng cùng α; chỉ để quan sát độ bất định, **không** tham gia routing/cost.
- **Persistence tương thích**: field mới trong `RoomStats` có `#[serde(default)]`; `learning.json` cũ thiếu field load bình thường với giá trị 0, các field cũ (`est_service_min`, `est_wait_observed_avg`, `visit_count`) giữ nguyên, record kế tiếp tự cập nhật field mới. Không ghi đè/xoá data cũ.

*`sim.rs` (additive view only):* `RoomView` thêm `learned_alpha` (α hiện tại theo `visit_count` của room) và `avg_abs_service_dev_min`. Không đổi logic mô phỏng.

*Control:* `EwmaConfig { alpha_init: 0.3, alpha_min: 0.3, ... }` tái tạo **bit-identical** Round 8 ở live run và cả 3 seed compare → xác nhận routing/sim/baselines không bị đụng.

**Sweep tham số** (mỗi combo fresh data dir, live seed 99 → compare 42/7/123; số là Optimized; 36 combo α_init ∈ {0.4,0.5,0.6,0.7} × α_min ∈ {0.1,0.15,0.2} × k ∈ {5,10,20}, cộng chẩn đoán ngoài range):

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

- **Không combo nào trong range cải thiện compare mean**: tốt nhất avg_total là 72.13 (0.5/0.1/5), tốt nhất p90_total là 109.42 (0.6/0.1/10 và 0.7/0.1/20) — đều kém R8 (69.83 / 107.45). Regression tập trung ở **seed 123** (79.2–81.7 vs 72.29); seed 42/7 gần như đứng yên.
- Chẩn đoán `α_min = 0.3, k = 5` (ngoài range yêu cầu) **trùng khít R8 ở compare** vì sau live mỗi room đã có 14–36 visits → `α(n) = 0.3` đúng bằng fixed alpha trong suốt compare. Đây là bằng chứng cơ chế: lợi ích của adaptive chỉ xuất hiện khi learning thực sự cold; khi đã warm, α thấp hơn 0.3 làm estimate bám chậm hơn vào sample stream của seed mới.
- Chọn **0.7 / 0.1 / 10** làm cấu hình đại diện: nằm trong range, α_init cao nhất (cold start nhanh nhất), có cold-compare tốt nhất trong các combo cùng range (xem dưới), và minh hoạ rõ nhất trade-off cold vs warm.

**Cold-start quality (synthetic + live):**

| Kiểm chứng | Adaptive 0.7/0.1/10 | Fixed 0.3 | Nhận xét |
|---|---|---|---|
| 5 samples seed 10 → 13 (estimate) | 12.9685 (lệch 0.0315) | 12.4958 (lệch 0.5040) | adaptive lệch **16× nhỏ hơn** |
| Variance steady state (500 estimate, n≥300, sample ±20%) | 0.1006 | 0.3483 | adaptive **~3.5× thấp hơn** |
| Live seed 99: thời điểm MAE < 1.0 | t = 54.0 | t = 78.0 | sớm hơn 24 phút |
| Live seed 99: thời điểm MAE < 0.75 | t = 78.0 | t = 90.0 | sớm hơn 12 phút |
| Live seed 99: MAE cuối run | 0.459 | 0.427 | nhỉnh hơn 0.03 (xem phân tích) |

MAE cuối run của adaptive không thấp hơn vì live chỉ có 16–36 visits/room: điểm estimate cuối chịu finite-sample noise của seed nhiều hơn là variance; lợi thế variance chỉ hiện rõ ở chuỗi dài (test synthetic 800 samples).

**Kết quả live run seed 99 (fresh dir, speed 5000, cold start thực sự):**

| Metric | R8 (α=0.3) | Round 9 (0.7/0.1/10) | Δ |
|---|---|---|---|
| avg_total | 78.01 | 79.96 | +1.95 (+2.50%) |
| avg_wait | 6.61 | 7.13 | +0.52 (+7.87%) |
| p90_total | 146.60 | 150.32 | +3.72 (+2.54%) |
| p90_wait | 16.47 | 23.02 | +6.55 (+39.8%) |
| avg_walk | 11.05 | 11.10 | +0.05 (+0.45%) |
| finished (sim time) | 399.75 | 392.21 | −7.54 phút |
| final MAE | 0.427 | 0.459 | +0.032 |

Live: 52/52, 188 visits, urgent 5.00/10.25, normal 7.29/23.26 (avg/p90 wait). Adaptive hội tụ estimate nhanh hơn ở giai đoạn đầu nhưng **live metrics xấu hơn**, chủ yếu ở p90_wait: α cao thời kỳ đầu bám theo từng service sample ±20% (nhiễu, không phải tín hiệu), làm thứ hạng phòng đổi sớm và đẩy tail của nhóm normal lên (p90_wait normal 23.26 vs 14.59 của R8), dù nhóm urgent lại tốt hơn (5.00 vs 5.42).

**Compare 3 seeds — đúng quy trình verify (live seed 99 trước, rồi compare):**

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

Mean 3 seed Optimized: **73.04 / 5.72 / 118.60 / 11.18** vs R8 **69.83 / 4.73 / 107.45 / 11.36** → avg_total **+3.21 (+4.59%)**, avg_wait +0.99 (+20.9%), p90_total **+11.15 (+10.38%)**, avg_walk −0.18 (−1.62%). Baselines khớp **từng số** với R8 ở cả 3 seed × 4 metric (77.0249/5.9134/130.2068/13.4523; 71.3185/4.3273/103.8948/13.5720; 84.3898/7.0846/138.8932/13.8233; 77.8194/5.5577/139.7541/13.0962; 86.4409/10.1679/145.2265/12.7288; 81.0868/8.6726/126.7505/12.4931) → physics/seed/baseline routing không đổi.

**Compare cold (fresh dir, không live warm-up) — nơi adaptive thực sự có đất:**

| Config | Mean avg_total | Mean p90_total | s42 avg/p90 | s7 avg/p90 | s123 avg/p90 |
|---|---|---|---|---|---|
| R8 control α=0.3 | 73.00 | 116.88 | 64.07 / 77.79 | 73.09 / 128.65 | 81.82 / 144.21 |
| **0.7 / 0.1 / 10** | **71.05** | **109.30** | 64.07 / 77.79 | 73.17 / 128.20 | **75.92 / 121.92** |
| diagnostic 0.7 / 0.3 / 5 | 71.09 | 109.34 | 64.07 / 77.79 | 73.27 / 128.30 | 75.92 / 121.92 |

Cold compare: mean avg_total −1.94 (−2.66%), p90_total −7.58 (−6.49%); seed 123 −7.21% avg / −15.46% p90, seed 42/7 trung tính. Tức adaptive giúp đúng như kỳ vọng khi learning bắt đầu từ zero, nhưng **quy trình verify chuẩn (live seed 99 trước) đã warm-start compare** nên lợi ích này bị che khuất, trong khi nhược điểm α thấp ở n≥14 lại lộ ra.

**Phân tích cold-start vs steady-state:**
- **Cold** (n = 0–10): α 0.7 → 0.35 hội tụ nhanh, estimate sát true mean sớm hơn hẳn; synthetic 16× và cold compare −6.5% p90 xác nhận.
- **Warm** (n = 14–36 sau live): α(n) = 0.15–0.29 < 0.3. Compare chỉ 52 patients/188 visits, đủ ngắn để estimate "bám" trạng thái warm; α thấp làm chậm việc hiệu chỉnh theo realized sample mean của seed compare. Route ranking đổi → seed 123 xấu đi mạnh. Diagnostic floor 0.3 tie R8 chứng minh đây là nguyên nhân, không phải lỗi routing.
- **Live**: α cao đầu run phản ứng với service noise ±20% (sample variance, không phải drift), gây churn; estimate accuracy vốn không phải nút cổ chai của routing (R8 đã đủ tốt), nên hội tụ nhanh hơn không chuyển thành metric tốt hơn.
- **Steady state variance**: mục tiêu (b) đạt trong test synthetic dài (3.5× thấp hơn), nhưng trong run 52 bệnh nhân lợi ích này chưa đủ bù tác động của việc giảm responsiveness lên route decisions.

**Files changed:** `backend/src/domain.rs` (`RoomStats.avg_abs_service_dev_min` + `#[serde(default)]`), `backend/src/engine/learn.rs` (`EwmaConfig`, α thích ứng, diagnostic EWMA, `with_ewma`/`current_alpha`, 4 test mới), `backend/src/engine/sim.rs` (`RoomView` thêm `learned_alpha` + `avg_abs_service_dev_min`), `backend/src/engine/router.rs` (chỉ test literal thêm field, không đổi logic), `docs/rounds/README.md`. Không đổi router/congestion logic, sim physics, baselines, priority queue, API contract cũ, frontend. Sweep dùng env override tạm thời; **đã gỡ**, code cuối hard-code 0.7/0.1/10.

**Verify steps:**
1. `cargo test`: **68/68 pass** (64 test cũ + 4 mới: α giảm từ init chạm floor đúng tại n = 60; hội tụ 5 mẫu nhanh hơn fixed 0.3 ≥ 40%; variance steady state < 80% fixed; legacy snapshot thiếu field mới load được, default 0, record kế tiếp upgrade đúng; test persistence cũ cập nhật kỳ vọng theo α(0)=0.7 và kiểm tra diagnostic).
2. `npm run build`: OK (frontend không đổi; chỉ warning chunk > 500 kB có từ trước).
3. Fresh `MEDIROUTE_DATA_DIR=/tmp/mediroute-r9-test`, server riêng `PORT=8082`; live `{strategy:optimized, speed:5000, seed:99}` → finished 392.21 phút, 52/52, 188 visits.
4. `POST /api/sim/compare` seed 42 → 7 → 123; baselines khớp R8 từng số; control fixed α=0.3 tái tạo R8 bit-identical.
5. Sweep 36 combo + 14 chẩn đoán (mỗi combo fresh dir `/tmp/mediroute-r9-sweep/*`), thêm cold compare không live warm-up.
6. Legacy `backend/data/learning.json` thật: server load không warning, stats giữ nguyên, field mới = 0, `learned_alpha` xuất hiện trong `/api/rooms`.
7. Port 8080 không đụng; không git commit/push.

**Đánh giá vs Round 8:**
- (a) Cold-start convergence: **đạt** — synthetic 16× gần hơn sau 5 mẫu; live MAE < 1 sớm hơn 24 phút, MAE < 0.75 sớm hơn 12 phút; cold compare mean −2.66% avg_total / −6.49% p90_total.
- (b) Steady-state variance: **đạt trên chuỗi dài** — 0.1006 vs 0.3483 (~3.5× thấp hơn) ở test 800 samples.
- (c) End-to-end theo quy trình verify: **KHÔNG đạt** — live +2.50% avg_total / +2.54% p90_total / +39.8% p90_wait; compare sau live +4.59% avg_total / +10.38% p90_total, regression nặng ở seed 123 (+13.0% avg / +23.9% p90). Không seed nào cải thiện, baselines không đổi.

**Decision:** **KHÔNG COMMIT** — thất bại tiêu chí end-to-end dù đạt (a) và (b). Working tree giữ code + docs để coordinator verify độc lập; không có thay đổi nào được commit.

**Bài học:** α thích ứng là giải pháp đúng cho *cold start* nhưng quy trình verify (live warm-up trước compare) biến phần lớn learning thành warm, nơi α thấp hơn 0.3 chỉ làm estimate bám seed mới chậm hơn. Bài toán thực sự không phải "estimate hội tụ nhanh" mà là "route ranking ổn định dưới noise ±20% khi đã có đủ mẫu". Hướng tiếp theo hợp lý: chỉ áp α init cao cho room **chưa từng có snapshot** (visit_count = 0 trong persisted stats) và giữ α = 0.3 cho room đã warm; hoặc dùng diagnostic `avg_abs_service_dev_min` để phát hiện drift thật rồi mới tăng α, thay vì tăng α máy móc theo n nhỏ. Cả hai đều cần đo lại vì live cold start vẫn có thể churn như đã thấy ở Round 9.

**Verify độc lập (coordinator):** cargo test 68/68; lặp quy trình chuẩn (fresh dir → live seed 99 → compare seed 123): khớp chính xác (81.69/144.21 — regression vs R8 72.29/116.37, p90 thậm chí tệ hơn ShortestQueue 126.75). Code reverted, cargo test 64/64.

**Decision:** DISCARD — regression end-to-end rõ rệt sau warm-up (+4.59% avg, +10.38% p90 mean 3 seeds). Control α=0.3 của agent tái tạo R8 bit-identical, xác nhận nguyên nhân nhân quả.

**Bài học:** α adaptive chỉ thắng ở cold-start thuần (first-run MAE tốt hơn 16×) nhưng trong quy trình có warm-up, α thấp khi warm làm estimate bám dịch chậm — service mean không đổi trong ngày nên tracking nhanh hơn không mang lợi, chỉ mang noise. α = 0.3 cố định là điểm cân bằng tốt cho workload tĩnh.
