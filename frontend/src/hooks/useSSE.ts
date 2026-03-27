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

interface UseSSEOptions<T> {
  endpoint: string;
  initialData?: T;
  enabled?: boolean;
}

export function useSSE<T = Opportunity[]>({
  endpoint,
  initialData = [] as T,
  enabled = true,
}: UseSSEOptions<T>) {
  const [data, setData] = useState<T>(initialData);
  const [isConnected, setIsConnected] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const eventSourceRef = useRef<EventSource | null>(null);

  const connect = useCallback(() => {
    if (!enabled || eventSourceRef.current) return;

    const eventSource = new EventSource(endpoint);
    eventSourceRef.current = eventSource;

    eventSource.onopen = () => {
      setIsConnected(true);
      setError(null);
      console.log(`✅ SSE connected to ${endpoint}`);
    };

    eventSource.onmessage = (event) => {
      try {
        const parsed: Opportunity = JSON.parse(event.data);
        setData((prev: any) => {
          // Keep only top 10 most profitable (Live Pulse requirement)
          const updated = [parsed, ...prev].sort((a, b) => b.net_yield_percent - a.net_yield_percent).slice(0, 10);
          return updated as T;
        });
      } catch (err) {
        console.error('SSE parse error:', err);
      }
    };

    eventSource.onerror = (err) => {
      console.error('SSE error:', err);
      setIsConnected(false);
      setError('Connection lost – reconnecting...');
      
      // Auto-reconnect
      setTimeout(() => {
        if (eventSourceRef.current) {
          eventSourceRef.current.close();
          eventSourceRef.current = null;
        }
        connect();
      }, 3000);
    };
  }, [endpoint, enabled]);

  const disconnect = useCallback(() => {
    if (eventSourceRef.current) {
      eventSourceRef.current.close();
      eventSourceRef.current = null;
    }
    setIsConnected(false);
  }, []);

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
