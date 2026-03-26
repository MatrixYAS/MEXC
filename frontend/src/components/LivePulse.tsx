// frontend/src/components/LivePulse.tsx
// Page 1: The "Live Pulse" (Real-Time Scanner)
// Shows only the top 10 most profitable paths with real-time updates

import { useState, useEffect } from 'react';
import { api, TriangleOpportunity } from '../lib/api';
import { useSSE } from '../hooks/useSSE';

export default function LivePulse() {
  const [topGaps, setTopGaps] = useState<TriangleOpportunity[]>([]);
  const [lastUpdate, setLastUpdate] = useState<Date>(new Date());

  // Use SSE for real-time updates (as per PRD)
  const { data: liveData, isConnected } = useSSE<TriangleOpportunity[]>({
    endpoint: '/api/live-pulse',           // TODO: Implement SSE endpoint in Rust later
    initialData: [],
  });

  // Fallback: Poll every 2 seconds if SSE not yet implemented
  useEffect(() => {
    const fetchTopGaps = async () => {
      try {
        // For now we use recent opportunities as placeholder
        const opportunities = await api.recentOpportunities(20);
        
        // Sort by net yield descending and take top 10
        const sorted = [...opportunities]
          .sort((a, b) => b.net_yield_percent - a.net_yield_percent)
          .slice(0, 10);
        
        setTopGaps(sorted);
        setLastUpdate(new Date());
      } catch (error) {
        console.error('Failed to fetch live gaps:', error);
      }
    };

    fetchTopGaps();
    const interval = setInterval(fetchTopGaps, 2000); // Update every 2s

    return () => clearInterval(interval);
  }, []);

  // Update from SSE when available
  useEffect(() => {
    if (liveData && liveData.length > 0) {
      const sorted = [...liveData]
        .sort((a, b) => b.net_yield_percent - a.net_yield_percent)
        .slice(0, 10);
      setTopGaps(sorted);
      setLastUpdate(new Date());
    }
  }, [liveData]);

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
            Real-time triangle arbitrage opportunities • Updated every 2s
          </p>
        </div>
        
        <div className="flex items-center gap-3 text-sm">
          <div className={`px-3 py-1 rounded-full text-xs font-medium border ${isConnected ? 'border-emerald-500 text-emerald-500' : 'border-orange-500 text-orange-500'}`}>
            {isConnected ? '● LIVE SSE' : '● POLLING'}
          </div>
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
            {topGaps.length > 0 ? (
              topGaps.map((gap, index) => (
                <tr key={gap.id} className="gap-row hover:bg-[rgba(16,185,129,0.05)]">
                  <td className="px-6 py-5 font-mono text-sm font-medium">
                    {gap.path}
                  </td>
                  <td className="px-6 py-5 text-right">
                    <span className={`text-lg font-semibold text-success number-update ${gap.net_yield_percent > 0 ? 'text-emerald-500' : ''}`}>
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
