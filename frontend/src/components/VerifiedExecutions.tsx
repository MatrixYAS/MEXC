// frontend/src/components/VerifiedExecutions.tsx
// Updated per guide 2.4: Now uses real /api/today-stats endpoint

import { useState, useEffect } from 'react';
import { api, TriangleOpportunity, TodayStats } from '../lib/api';
import { format } from 'date-fns';

export default function VerifiedExecutions() {
  const [opportunities, setOpportunities] = useState<TriangleOpportunity[]>([]);
  const [todayStats, setTodayStats] = useState<TodayStats>({
    gaps_found: 0,
    avg_yield: 0,
    total_potential: 0,
  });
  const [loading, setLoading] = useState(true);

  const fetchData = async () => {
    setLoading(true);
    try {
      // Fetch recent opportunities
      const ops = await api.recentOpportunities(100);
      setOpportunities(ops);

      // Fetch real today stats from backend (new endpoint)
      const stats = await api.todayStats();
      setTodayStats(stats);
    } catch (error) {
      console.error('Failed to fetch verified executions:', error);
      // Fallback if backend endpoint not ready yet
      setTodayStats({
        gaps_found: ops.length,
        avg_yield: ops.length > 0 
          ? ops.reduce((sum, item) => sum + item.net_yield_percent, 0) / ops.length 
          : 0,
        total_potential: ops.reduce((sum, item) => sum + item.net_yield_percent, 0),
      });
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    fetchData();
    
    // Refresh every 10 seconds
    const interval = setInterval(fetchData, 10000);
    return () => clearInterval(interval);
  }, []);

  const formatAge = (ms: number): string => {
    if (ms < 60000) return `${Math.floor(ms / 1000)}s ago`;
    if (ms < 3600000) return `${Math.floor(ms / 60000)}m ago`;
    return `${Math.floor(ms / 3600000)}h ago`;
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
    <div className="space-y-8">
      <div>
        <h2 className="text-3xl font-semibold tracking-tight">Verified Executions</h2>
        <p className="text-[var(--secondary-text)] mt-1">
          Real-world executable gaps • Headless logging from the Rust engine
        </p>
      </div>

      {/* Analytics Header - Now uses real backend data */}
      <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
        <div className="surface rounded-2xl p-6 border border-[var(--accent-border)]">
          <div className="text-sm text-[var(--secondary-text)]">Gaps Found Today</div>
          <div className="text-4xl font-semibold mt-2 text-success">{todayStats.gaps_found}</div>
        </div>
        
        <div className="surface rounded-2xl p-6 border border-[var(--accent-border)]">
          <div className="text-sm text-[var(--secondary-text)]">Average Yield</div>
          <div className="text-4xl font-semibold mt-2 text-success">
            +{todayStats.avg_yield.toFixed(2)}%
          </div>
        </div>
        
        <div className="surface rounded-2xl p-6 border border-[var(--accent-border)]">
          <div className="text-sm text-[var(--secondary-text)]">Total Potential Yield</div>
          <div className="text-4xl font-semibold mt-2 text-success">
            +{todayStats.total_potential.toFixed(1)}%
          </div>
        </div>
      </div>

      {/* History Table */}
      <div className="surface rounded-2xl overflow-hidden border border-[var(--accent-border)]">
        <div className="px-6 py-4 border-b border-[var(--accent-border)] bg-[var(--surface)] flex justify-between items-center">
          <h3 className="font-medium">Recent Verified Opportunities</h3>
          <button 
            onClick={fetchData}
            className="text-xs px-4 py-1.5 bg-emerald-500 hover:bg-emerald-600 text-white rounded-lg transition-colors"
          >
            Refresh
          </button>
        </div>

        {loading && opportunities.length === 0 ? (
          <div className="py-20 text-center text-[var(--secondary-text)]">
            Loading verified gaps from SQLite...
          </div>
        ) : opportunities.length === 0 ? (
          <div className="py-20 text-center text-[var(--secondary-text)]">
            No verified opportunities yet.<br />
            The engine is scanning for real-world executable triangles.
          </div>
        ) : (
          <div className="overflow-x-auto">
            <table className="w-full">
              <thead>
                <tr className="border-b border-[var(--accent-border)]">
                  <th className="px-6 py-4 text-left text-xs font-medium text-[var(--secondary-text)]">DETECTED</th>
                  <th className="px-6 py-4 text-left text-xs font-medium text-[var(--secondary-text)]">PATH</th>
                  <th className="px-6 py-4 text-right text-xs font-medium text-[var(--secondary-text)]">NET YIELD</th>
                  <th className="px-6 py-4 text-right text-xs font-medium text-[var(--secondary-text)]">CAPACITY</th>
                  <th className="px-6 py-4 text-right text-xs font-medium text-[var(--secondary-text)]">AGE</th>
                  <th className="px-6 py-4 text-center text-xs font-medium text-[var(--secondary-text)]">SCORE</th>
                </tr>
              </thead>
              <tbody className="divide-y divide-[var(--accent-border)]">
                {opportunities.map((opp) => (
                  <tr key={opp.id} className="hover:bg-[rgba(16,185,129,0.05)]">
                    <td className="px-6 py-5 text-sm text-[var(--secondary-text)] font-mono">
                      {format(new Date(opp.detected_at), 'HH:mm:ss')}
                    </td>
                    <td className="px-6 py-5 font-mono text-sm">
                      {opp.path}
                    </td>
                    <td className="px-6 py-5 text-right">
                      <span className="text-lg font-semibold text-success">
                        +{opp.net_yield_percent.toFixed(2)}%
                      </span>
                    </td>
                    <td className="px-6 py-5 text-right font-medium">
                      ${opp.capacity_usd.toFixed(0)}
                    </td>
                    <td className="px-6 py-5 text-right text-sm text-[var(--secondary-text)]">
                      {formatAge(opp.gap_age_ms)}
                    </td>
                    <td className="px-6 py-5 text-center">
                      <span className={`inline-block px-3 py-0.5 text-xs font-bold rounded-full ${getFillScoreColor(opp.fill_score)}`}>
                        {opp.fill_score}
                      </span>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </div>

      <div className="text-xs text-center text-[var(--secondary-text)]">
        All entries passed $1,000 weighted fill simulation + 3 consecutive ticks • Auto-pruned after 7 days
      </div>
    </div>
  );
}
