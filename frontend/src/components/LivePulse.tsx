// frontend/src/components/LivePulse.tsx
// Updated per guide 2.1: Now uses real SSE via useSSE hook (no more polling fallback)

import { useState, useEffect } from 'react';
import { useSSE } from '../hooks/useSSE';

interface Opportunity {
  id: string;
  path: string;
  net_yield_percent: number;
  capacity_usd: number;
  gap_age_ms: number;
  fill_score: string;
  detected_at: string;
}

export default function LivePulse() {
  const { data: topGaps, isConnected, error } = useSSE<Opportunity[]>({
    endpoint: '/api/live-pulse',
    initialData: [],
  });

  const [lastUpdate, setLastUpdate] = useState<Date>(new Date());

  // Update timestamp whenever new data arrives
  useEffect(() => {
    if (topGaps && topGaps.length > 0) {
      setLastUpdate(new Date());
    }
  }, [topGaps]);

  const formatAge = (ms: number): string => {
    if (ms < 1000) return `${ms}ms`;
    const seconds = Math.floor(ms / 1000);
    return `${seconds}s`;
  };

  const getFillScoreColor = (score: string) => {
    switch (score) {
      case 'A': return 'badge-A';
      case 'B': return 'badge-B';
      case 'C': return 'badge-C';
      case 'D': return 'badge-D';
      case 'F': return 'badge-F';
      default: return 'badge-C';
    }
  };

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <div>
          <h2 className="text-3xl font-semibold tracking-tight">Live Pulse</h2>
          <p className="text-[var(--secondary-text)] mt-1">
            Real-time triangle arbitrage opportunities • Powered by SSE
          </p>
        </div>

        <div className="flex items-center gap-3 text-sm">
          <div className={`px-3 py-1 rounded-full text-xs font-medium border ${isConnected ? 'border-emerald-500 text-emerald-500' : 'border-orange-500 text-orange-500'}`}>
            {isConnected ? '● LIVE SSE' : '● RECONNECTING'}
          </div>
          {error && <div className="text-red-500 text-xs">{error}</div>}
          <div className="text-[var(--secondary-text)]">
            Last update: {lastUpdate.toLocaleTimeString()}
          </div>
        </div>
      </div>

      <div className="surface rounded-2xl overflow-hidden border border-[var(--accent-border)]">
        <table className="w-full">
          <thead>
            <tr className="border-b border-[var(--accent-border)] bg-[var(--surface)]">
              <th className="px-6 py-4 text-left text-xs font-medium text-[var(--secondary-text)]">PATH</th>
              <th className="px-6 py-4 text-right text-xs font-medium text-[var(--secondary-text)]">NET %</th>
              <th className="px-6 py-4 text-right text-xs font-medium text-[var(--secondary-text)]">CAPACITY</th>
              <th className="px-6 py-4 text-right text-xs font-medium text-[var(--secondary-text)]">AGE</th>
              <th className="px-6 py-4 text-center text-xs font-medium text-[var(--secondary-text)]">FILL SCORE</th>
            </tr>
          </thead>
          <tbody className="divide-y divide-[var(--accent-border)]">
            {topGaps && topGaps.length > 0 ? (
              topGaps.map((gap) => (
                <tr key={gap.id} className="gap-row hover:bg-[rgba(16,185,129,0.05)]">
                  <td className="px-6 py-5 font-mono text-sm font-medium">
                    {gap.path}
                  </td>
                  <td className="px-6 py-5 text-right">
                    <span className={`text-lg font-semibold text-success number-update`}>
                      +{gap.net_yield_percent.toFixed(2)}%
                    </span>
                  </td>
                  <td className="px-6 py-5 text-right font-medium text-[var(--primary-text)]">
                    ${gap.capacity_usd.toFixed(0)}
                  </td>
                  <td className="px-6 py-5 text-right text-sm text-[var(--secondary-text)] font-mono">
                    {formatAge(gap.gap_age_ms)}
                  </td>
                  <td className="px-6 py-5 text-center">
                    <span className={`inline-block px-3 py-0.5 text-xs font-bold rounded-full ${getFillScoreColor(gap.fill_score)}`}>
                      {gap.fill_score}
                    </span>
                  </td>
                </tr>
              ))
            ) : (
              <tr>
                <td colSpan={5} className="px-6 py-16 text-center text-[var(--secondary-text)]">
                  No profitable gaps detected yet.<br />
                  The engine is scanning...
                </td>
              </tr>
            )}
          </tbody>
        </table>
      </div>

      <div className="text-xs text-[var(--secondary-text)] text-center">
        Only gaps that survived $1,000 fill simulation + 3-tick persistence are shown • 0.1% taker fee applied
      </div>
    </div>
  );
}
