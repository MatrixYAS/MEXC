// frontend/src/lib/api.ts
// Updated per guide: Added API Key management + Today Stats + Test Connection

const API_BASE = import.meta.env.DEV 
  ? 'http://localhost:7860' 
  : '';  

export interface TriangleOpportunity {
  id: string;
  path: string;
  net_yield_percent: number;
  capacity_usd: number;
  gap_age_ms: number;
  fill_score: string;
  detected_at: string;
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

export interface ApiKeyRequest {
  api_key: string;
  secret_key: string;
}

export interface TodayStats {
  gaps_found: number;
  avg_yield: number;
  total_potential: number;
}

// Generic GET helper
async function get<T>(endpoint: string): Promise<T> {
  const response = await fetch(`${API_BASE}${endpoint}`);
  
  if (!response.ok) {
    throw new Error(`API Error: ${response.status} ${response.statusText}`);
  }
  
  return response.json();
}

// Generic POST helper
async function post<T>(endpoint: string, body: any): Promise<T> {
  const response = await fetch(`${API_BASE}${endpoint}`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(body),
  });

  if (!response.ok) {
    throw new Error(`API Error: ${response.status} ${response.statusText}`);
  }

  return response.json();
}

// Main API functions
export const api = {
  health: async (): Promise<HealthResponse> => {
    return get<HealthResponse>('/api/health');
  },

  telemetry: async (): Promise<TelemetryData> => {
    return get<TelemetryData>('/api/telemetry');
  },

  recentOpportunities: async (limit: number = 50): Promise<TriangleOpportunity[]> => {
    return get<TriangleOpportunity[]>(`/api/opportunities?limit=${limit}`);
  },

  whitelist: async (): Promise<string[]> => {
    return get<string[]>('/api/whitelist');
  },

  // NEW: API Key management (guide 2.2)
  saveApiKeys: async (payload: ApiKeyRequest): Promise<any> => {
    return post('/api/keys', payload);
  },

  // NEW: Test MEXC connection
  testMexcConnection: async (payload: ApiKeyRequest): Promise<any> => {
    return post('/api/test-mexc-connection', payload);
  },

  // NEW: Today stats for Verified Executions page (guide 2.4)
  todayStats: async (): Promise<TodayStats> => {
    return get<TodayStats>('/api/today-stats');
  },
};

// SSE helper (kept for future use)
export function createSSEConnection(endpoint: string, onMessage: (data: any) => void) {
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
    console.error('SSE connection error:', error);
  };

  return eventSource;
}
