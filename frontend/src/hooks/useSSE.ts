// frontend/src/hooks/useSSE.ts
// Custom hook for Server-Sent Events (SSE)
// As specified in the PRD: "Use Server-Sent Events (SSE)" for real-time data streaming
// Lighter on battery/RAM than WebSockets for a read-only dashboard

import { useEffect, useState, useRef, useCallback } from 'react';

interface UseSSEOptions<T> {
  endpoint: string;
  initialData?: T;
  onMessage?: (data: T) => void;
  enabled?: boolean;
}

export function useSSE<T = any>({
  endpoint,
  initialData,
  onMessage,
  enabled = true,
}: UseSSEOptions<T>) {
  const [data, setData] = useState<T | null>(initialData || null);
  const [isConnected, setIsConnected] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const eventSourceRef = useRef<EventSource | null>(null);

  const connect = useCallback(() => {
    if (!enabled || eventSourceRef.current) return;

    try {
      const eventSource = new EventSource(endpoint);
      eventSourceRef.current = eventSource;

      eventSource.onopen = () => {
        setIsConnected(true);
        setError(null);
        console.log(`SSE connected to ${endpoint}`);
      };

      eventSource.onmessage = (event) => {
        try {
          const parsedData: T = JSON.parse(event.data);
          setData(parsedData);
          
          // Optional callback for additional side effects
          if (onMessage) {
            onMessage(parsedData);
          }
        } catch (err) {
          console.error('Failed to parse SSE message:', err);
        }
      };

      eventSource.onerror = (err) => {
        console.error('SSE error:', err);
        setIsConnected(false);
        setError('Connection lost. Reconnecting...');
        
        // Auto-reconnect after delay
        setTimeout(() => {
          if (eventSourceRef.current) {
            eventSourceRef.current.close();
            eventSourceRef.current = null;
          }
          connect();
        }, 3000);
      };
    } catch (err) {
      setError('Failed to establish SSE connection');
      console.error('SSE setup error:', err);
    }
  }, [endpoint, onMessage, enabled]);

  const disconnect = useCallback(() => {
    if (eventSourceRef.current) {
      eventSourceRef.current.close();
      eventSourceRef.current = null;
    }
    setIsConnected(false);
  }, []);

  // Auto-connect when enabled changes
  useEffect(() => {
    if (enabled) {
      connect();
    } else {
      disconnect();
    }

    // Cleanup on unmount
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
