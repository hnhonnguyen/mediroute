export type Strategy = 'optimized' | 'fixed_order' | 'shortest_queue';

export type PatientStatus =
  | 'arrived'
  | 'en_route'
  | 'in_queue'
  | 'in_service'
  | 'departed';

export type PatientPriority = 'normal' | 'urgent';

export interface RoomConfig {
  id: string;
  name: string;
  initial_avg_service_min: number;
  capacity: number;
  wing: string;
  position: [number, number];
}

export interface DependencyEdge {
  from: string;
  to: string;
}

export interface PackageConfig {
  id: string;
  name: string;
  rooms: string[];
  weight: number;
}

export interface ScenarioConfig {
  rooms: RoomConfig[];
  dependencies: DependencyEdge[];
  packages: PackageConfig[];
  walk_time_min: number;
}

export interface RoomUtilization {
  room_id: string;
  room_name: string;
  capacity: number;
  utilization_pct: number;
  visits: number;
  avg_service_min: number;
  total_busy_min: number;
}

export interface Metrics {
  total_patients: number;
  arrived: number;
  completed: number;
  in_hospital: number;
  avg_total_min: number;
  avg_wait_min: number;
  p90_total_min: number;
  p90_wait_min: number;
  avg_wait_urgent_min?: number;
  p90_wait_urgent_min?: number;
  avg_wait_normal_min?: number;
  p90_wait_normal_min?: number;
  avg_walk_min?: number;
  total_visits: number;
  room_utilization: RoomUtilization[];
}

export interface RoomView {
  id: string;
  name: string;
  capacity: number;
  wing?: string;
  position?: [number, number];
  initial_avg_service_min: number;
  est_service_min: number;
  est_wait_observed_avg: number;
  visit_count: number;
  queue_len: number;
  in_service: number;
  en_route: number;
  est_wait_min: number;
  utilization_pct: number;
  downtime_active?: boolean;
  downtime_until?: number | null;
  downtime_remaining_min?: number;
}

export interface PatientVisitView {
  room_id: string;
  room_name: string;
  wait_min: number;
  service_min: number;
  service_end: number;
}

export interface PatientView {
  id: number;
  name: string;
  package_id: string;
  package_name: string;
  arrival_min: number;
  priority?: PatientPriority;
  status: PatientStatus;
  current_room: string | null;
  current_room_name: string | null;
  rooms_done: string[];
  rooms_done_names: string[];
  rooms_remaining: string[];
  rooms_remaining_names: string[];
  rooms_total: number;
  elapsed_min: number | null;
  total_time_min: number | null;
  visits: PatientVisitView[];
}

export interface HistoryEntry {
  patient_id: number;
  patient_name: string;
  room_id: string;
  room_name: string;
  arrival_at_room: number;
  service_start: number;
  service_end: number;
  wait_min: number;
  service_min: number;
}

export interface SimStatus {
  started: boolean;
  running: boolean;
  paused: boolean;
  finished: boolean;
  strategy: Strategy | null;
  speed: number;
  seed: number | null;
  sim_time_min: number;
  metrics: Metrics;
}

export interface StrategyResult {
  strategy: Strategy;
  label: string;
  metrics: Metrics;
  makespan_min: number;
}

export interface CompareResponse {
  seed: number;
  patient_count: number;
  results: StrategyResult[];
}

export interface StartSimRequest {
  strategy?: Strategy;
  speed?: number;
  seed?: number;
  patient_count?: number;
}
