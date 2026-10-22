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

