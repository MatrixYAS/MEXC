// frontend/src/lib/api.ts
// API client for communicating with the Rust backend
// Supports both REST and future SSE streaming

const API_BASE = import.meta.env.DEV 
  ? 'http://localhost:7860' 
  : '';  // In production (Docker/Hugging Face), same origin

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

// Generic GET helper
async function get<T>(endpoint: string): Promise<T> {
  const response = await fetch(`${API_BASE}${endpoint}`);
  
  if (!response.ok) {
    throw new Error(`API Error: ${response.status} ${response.statusText}`);
  }
  
  return response.json();
}

// Main API functions
export const api = {
  // Health check
  health: async (): Promise<HealthResponse> => {
    return get<HealthResponse>('/api/health');
  },

  // Real-time telemetry
  telemetry: async (): Promise<TelemetryData> => {
    return get<TelemetryData>('/api/telemetry');
  },

  // Recent verified opportunities (for "Verified Executions" page)
  recentOpportunities: async (limit: number = 50): Promise<TriangleOpportunity[]> => {
    return get<TriangleOpportunity[]>(`/api/opportunities?limit=${limit}`);
  },

  // Current whitelist (for Market Maintenance page)
  whitelist: async (): Promise<string[]> => {
    return get<string[]>('/api/whitelist');
  },
};

// Future SSE helper (will be used in useSSE hook)
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
    // Auto-reconnect logic can be added here
  };

  return eventSource;
}
