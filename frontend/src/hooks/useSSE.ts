// frontend/src/hooks/useSSE.ts
// Updated per guide 2.1: Real EventSource connection to /api/live-pulse SSE

import { useEffect, useState, useRef, useCallback } from 'react';

interface Opportunity {
  id: string;
  path: string;
  net_yield_percent: number;
  capacity_usd: number;
  gap_age_ms: number;
  fill_score: string;
  detected_at: string;
}

interface Telemetry {
  cpu_usage: number;
  ram_usage_mb: number;
  ws_latency_ms: number;
  math_loop_time_ms: number;
  active_triangles: number;
  timestamp: string;
}

interface UseSSEOptions<T> {
  endpoint: string;
  initialData?: T;
  enabled?: boolean;
  onSnapshot?: (t: Telemetry) => void;
}

export function useSSE<T = Opportunity[]>({
  endpoint,
  initialData = [] as T,
  enabled = true,
  onSnapshot,
}: UseSSEOptions<T>) {
  const [data, setData] = useState<T>(initialData);
  const [isConnected, setIsConnected] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const eventSourceRef = useRef<EventSource | null>(null);
  const reconnectTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const attempts = useRef(0);
  const onSnapshotRef = useRef(onSnapshot);
  onSnapshotRef.current = onSnapshot;

  const disconnect = useCallback(() => {
    if (reconnectTimer.current) {
      clearTimeout(reconnectTimer.current);
      reconnectTimer.current = null;
    }
    if (eventSourceRef.current) {
      eventSourceRef.current.close();
      eventSourceRef.current = null;
    }
    setIsConnected(false);
  }, []);

  const connect = useCallback(() => {
    if (!enabled) return;
    disconnect();

    const eventSource = new EventSource(endpoint);
    eventSourceRef.current = eventSource;

    eventSource.onopen = () => {
      setIsConnected(true);
      setError(null);
      attempts.current = 0;
      console.log(`✅ SSE connected to ${endpoint}`);
    };

    // Named event: a new verified gap (previously the only data we expected)
    eventSource.addEventListener('gap', (ev) => {
      try {
        const parsed: Opportunity = JSON.parse(ev.data);
        setData((prev: any) => {
          const updated = [parsed, ...prev].sort((a, b) => b.net_yield_percent - a.net_yield_percent).slice(0, 10);
          return updated as T;
        });
      } catch (err) {
        console.error('SSE gap parse error:', err);
      }
    });

    // Named event: initial + periodic telemetry snapshot (keeps the UI
    // dashboard numbers live even in quiet markets)
    eventSource.addEventListener('snapshot', (ev) => {
      try {
        const parsed: Telemetry = JSON.parse(ev.data);
        onSnapshotRef.current?.(parsed);
      } catch (err) {
        console.error('SSE snapshot parse error:', err);
      }
    });

    // Named event: heartbeat every 15s — proves the connection is alive.
    eventSource.addEventListener('heartbeat', () => {});

    eventSource.onerror = () => {
      // EventSource auto-reconnects internally; treat repeated failures as
      // a real outage and reconnect with exponential backoff (3s → 30s cap).
      setIsConnected(false);
      setError('Connection lost – reconnecting...');
      attempts.current += 1;
      const delay = Math.min(3000 * attempts.current, 30000);
      reconnectTimer.current = setTimeout(() => {
        reconnectTimer.current = null;
        connect();
      }, delay);
    };
    }, [endpoint, enabled, disconnect]);

  useEffect(() => {
    if (enabled) {
      connect();
    } else {
      disconnect();
    }

    return () => {
      disconnect();
    };
  }, [enabled, connect, disconnect]);

  return {
    data,
    isConnected,
    error,
    reconnect: connect,
  };
}
