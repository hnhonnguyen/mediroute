import type {
  CompareResponse,
  HistoryEntry,
  PatientView,
  RoomView,
  ScenarioConfig,
  SimStatus,
  StartSimRequest,
} from './types';

const BASE = '/api';

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const response = await fetch(`${BASE}${path}`, {
    ...init,
    headers: {
      'Content-Type': 'application/json',
      ...init?.headers,
    },
  });
  if (!response.ok) {
    let message = `Request failed (${response.status})`;
    try {
      const body: unknown = await response.json();
      if (
        body &&
        typeof body === 'object' &&
        'error' in body &&
        typeof body.error === 'string'
      ) {
        message = body.error;
      }
    } catch {
      // Keep the status-based message when the body is not JSON.
    }
    throw new Error(message);
  }
  return (await response.json()) as T;
}

export const api = {
  getConfig: () => request<ScenarioConfig>('/config'),
  getStatus: () => request<SimStatus>('/sim/status'),
  getRooms: () => request<RoomView[]>('/rooms'),
  getPatients: () => request<PatientView[]>('/patients'),
  getHistory: () => request<HistoryEntry[]>('/history'),
  startSim: (payload: StartSimRequest) =>
    request<SimStatus>('/sim/start', {
      method: 'POST',
      body: JSON.stringify(payload),
    }),
  pauseSim: () => request<SimStatus>('/sim/pause', { method: 'POST' }),
  resumeSim: () => request<SimStatus>('/sim/resume', { method: 'POST' }),
  resetSim: () => request<SimStatus>('/sim/reset', { method: 'POST' }),
  setSpeed: (speed: number) =>
    request<SimStatus>('/sim/speed', {
      method: 'POST',
      body: JSON.stringify({ speed }),
    }),
  compare: () =>
    request<CompareResponse>('/sim/compare', {
      method: 'POST',
      body: JSON.stringify({}),
    }),
};
