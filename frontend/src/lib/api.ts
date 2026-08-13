// frontend/src/lib/api.ts
// v2: JWT Bearer auth for admin endpoints, full 15-field opportunity model,
// settings & credential-verification endpoints.

export interface Opportunity {
  id: string;
  triangle_id: string;
  path: string;
  net_yield_percent: number;
  gross_gap_percent: number;
  fee_cost_percent: number;
  estimated_profit_usd: number;
  capacity_usd: number;
  gap_age_ms: number;
  ticks_survived: number;
  fill_score: string;
  staleness_ms: number;
  confidence: number;
  maker_plan_yield_percent: number;
  slippage_percent: number;
  optimal_size_usd: number;
  optimal_net_yield_percent: number;
  size_curve_json: string;
  leg1_symbol: string;
  leg1_entry_price: number;
  leg1_fill_price: number;
  leg2_symbol: string;
  leg2_entry_price: number;
  leg2_fill_price: number;
  leg3_symbol: string;
  leg3_entry_price: number;
  leg3_fill_price: number;
  detected_at: string;
  is_executed: boolean;
}

export interface SettingsSnapshot {
  min_profit_threshold: number;
  taker_fee: number;
  slippage_buffer: number;
  target_volume_usd: number;
  tick_interval_ms: number;
  required_ticks: number;
  max_whitelist: number;
  min_24h_volume_usd: number;
  retention_days: number;
  scan_paused: boolean;
  updated_at: string;
}

export interface KeyTestResult {
  checked_at: string;
  test_name: string;
  passed: boolean;
  detail: string;
}

export interface TelemetryData {
  cpu_usage: number;
  ram_usage_mb: number;
  ws_latency_ms: number;
  math_loop_time_ms: number;
  active_triangles: number;
  timestamp: string;
}

export interface HealthResponse {
  status: string;
  uptime_ms: number;
  telemetry: TelemetryData;
}

const API_BASE = import.meta.env.DEV ? 'http://localhost:7860' : '';

function authHeaders(): HeadersInit {
  const token = localStorage.getItem('authToken');
  return token ? { Authorization: `Bearer ${token}` } : {};
}

// Authenticated GET — forces re-login on 401
async function getAuth<T>(endpoint: string): Promise<T> {
  const res = await fetch(`${API_BASE}${endpoint}`, { headers: authHeaders() });
  if (res.status === 401) {
    localStorage.removeItem('authToken');
    window.location.reload();
    throw new Error('Session expired');
  }
  if (!res.ok) throw new Error(`API Error: ${res.status} ${res.statusText}`);
  return res.json();
}

// Authenticated POST
async function postAuth<T>(endpoint: string, body: any): Promise<T> {
  const res = await fetch(`${API_BASE}${endpoint}`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json', ...authHeaders() },
    body: JSON.stringify(body),
  });
  if (res.status === 401) {
    localStorage.removeItem('authToken');
    window.location.reload();
    throw new Error('Session expired');
  }
  if (!res.ok) {
    const msg = await res.text().catch(() => res.statusText);
    throw new Error(`API Error: ${res.status} ${msg}`);
  }
  return res.json();
}

// Public (unauthenticated) endpoints
async function get<T>(endpoint: string): Promise<T> {
  const res = await fetch(`${API_BASE}${endpoint}`);
  if (!res.ok) throw new Error(`API Error: ${res.status} ${res.statusText}`);
  return res.json();
}

export const api = {
  health: async (): Promise<HealthResponse> => get('/api/health'),
  telemetry: async (): Promise<TelemetryData> => get('/api/telemetry'),
  whitelist: async (): Promise<string[]> => get('/api/whitelist'),
  todayStats: async (): Promise<{ gaps_found: number; avg_yield_pct: number; total_estimated_profit_usd: number }> => get('/api/today-stats'),

  // Admin endpoints
  recentOpportunities: async (): Promise<Opportunity[]> => getAuth('/api/recent-opportunities'),
  allOpportunities: async (): Promise<Opportunity[]> => getAuth('/api/all-opportunities'),
  settings: async (): Promise<SettingsSnapshot> => getAuth('/api/settings'),
  saveSettings: async (payload: Partial<SettingsSnapshot>): Promise<string> => postAuth('/api/settings', payload),
  verifyKeys: async (payload: { api_key: string; secret_key: string }): Promise<{ all_passed: boolean; results: KeyTestResult[]; save_recommended: boolean }> =>
    postAuth('/api/verify-keys', payload),
  saveApiKeys: async (payload: { api_key: string; secret_key: string }): Promise<string> => postAuth('/api/keys', payload),
  hasApiKeys: async (): Promise<boolean> => getAuth<boolean>('/api/keys'),
  deleteApiKeys: async (): Promise<string> => postAuth('/api/keys/delete', {}),
  keyTests: async (): Promise<KeyTestResult[]> => getAuth('/api/key-tests'),

  exportCsv: async (): Promise<Blob> => {
    const res = await fetch(`${API_BASE}/api/export-csv`, { headers: authHeaders() });
    if (res.status === 401) {
      localStorage.removeItem('authToken');
      window.location.reload();
    }
    if (!res.ok) throw new Error(`Export failed: ${res.status}`);
    return res.blob();
  },
};

// SSE for live pulse (EventSource can't send custom headers; endpoint is public,
// verified opportunities are the only data it emits).
export function createSSEConnection(endpoint: string, onMessage: (data: Opportunity) => void): EventSource {
  const eventSource = new EventSource(`${API_BASE}${endpoint}`);
  eventSource.onmessage = (event) => {
    try {
      const data = JSON.parse(event.data);
      onMessage(data);
    } catch (error) {
      console.error('SSE parse error:', error);
    }
  };
  eventSource.onerror = (error) => {
    console.warn('SSE connection error (reconnecting...):', error);
  };
  return eventSource;
}
