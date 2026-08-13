// frontend/src/components/LivePulse.tsx
// v2: SSE stream of single verified opportunities (one per message).
// Shows the full dossier per opportunity: path, net/gross yield, fee breakdown,
// USD profit, capacity, time-open, fill score, staleness, confidence, maker plan,
// slippage, and per-leg entry/fill prices.

import { useState, useEffect, useRef } from 'react';
import { useSSE } from '../hooks/useSSE';
import { Opportunity } from '../lib/api';

export default function LivePulse() {
  const { data: gaps, isConnected, error } = useSSE<Opportunity[]>({
    endpoint: '/api/live-pulse',
    initialData: [],
  });

  const [lastUpdate, setLastUpdate] = useState<Date>(new Date());
  const [expanded, setExpanded] = useState<string | null>(null);
  const lastRef = useRef<number>(0);

  useEffect(() => {
    if (gaps && gaps.length > 0) {
      setLastUpdate(new Date());
      lastRef.current = Date.now();
    }
  }, [gaps]);

  // Flash effect on new arrival
  const isFresh = Date.now() - lastRef.current < 800;

  const formatAge = (ms: number): string => {
    if (ms < 1000) return `${ms}ms`;
    const seconds = Math.floor(ms / 1000);
    return `${seconds}s`;
  };

  const fmtNum = (n: number, digits = 2) => n.toFixed(digits);

  const getFillScoreColor = (score: string) => {
    switch (score.charAt(0)) {
      case 'A': return 'badge-A';
      case 'B': return 'badge-B';
      case 'C': return 'badge-C';
      case 'D': return 'badge-D';
      default: return 'badge-F';
    }
  };

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between flex-wrap gap-3">
        <div>
          <h2 className="text-3xl font-semibold tracking-tight">Live Pulse</h2>
          <p className="text-[var(--secondary-text)] mt-1">
            Verified USDT→COIN→COIN→USDT loops • SSE • nothing trades on numbers older than one tick
          </p>
        </div>

        <div className="flex items-center gap-3 text-sm">
          <div className={`px-3 py-1 rounded-full text-xs font-medium border ${isConnected ? 'border-emerald-500 text-emerald-500' : 'border-orange-500 text-orange-500'}`}>
            {isConnected ? '● LIVE SSE' : '● RECONNECTING'}
          </div>
          {error && <div className="text-red-500 text-xs">{error}</div>}
          <div className="text-[var(--secondary-text)]">
            Last gap: {lastUpdate.toLocaleTimeString()}
          </div>
        </div>
      </div>

      <div className="surface rounded-2xl overflow-hidden border border-[var(--accent-border)]">
        <table className="w-full">
          <thead>
            <tr className="border-b border-[var(--accent-border)] bg-[var(--surface)]">
              <th className="px-6 py-4 text-left text-xs font-medium text-[var(--secondary-text)]">PATH</th>
              <th className="px-6 py-4 text-right text-xs font-medium text-[var(--secondary-text)]">NET YIELD</th>
              <th className="px-6 py-4 text-right text-xs font-medium text-[var(--secondary-text)]">PROFIT / CAPACITY</th>
              <th className="px-6 py-4 text-right text-xs font-medium text-[var(--secondary-text)]">FEE COST</th>
              <th className="px-6 py-4 text-right text-xs font-medium text-[var(--secondary-text)]">TICKS</th>
              <th className="px-6 py-4 text-center text-xs font-medium text-[var(--secondary-text)]">FILL</th>
              <th className="px-6 py-4 text-center text-xs font-medium text-[var(--secondary-text)]">CONF</th>
              <th className="px-6 py-4 text-right text-xs font-medium text-[var(--secondary-text)]">AGE</th>
            </tr>
          </thead>
          <tbody className="divide-y divide-[var(--accent-border)]">
            {gaps && gaps.length > 0 ? (
              gaps.map((g, idx) => (
                <tr key={`${g.id}-${idx}`}
                  className={`gap-row hover:bg-[rgba(16,185,129,0.05)] cursor-pointer transition ${idx === 0 && isFresh ? 'bg-[rgba(16,185,129,0.12)]' : ''}`}
                  onClick={() => setExpanded(expanded === g.id ? null : g.id)}>
                  <td className="px-6 py-5 font-mono text-sm font-medium">
                    {g.path}
                    {g.maker_plan_yield_percent > 0 && (
                      <div className="text-[10px] text-[var(--secondary-text)] font-sans font-normal mt-0.5">
                        maker floor: +{fmtNum(g.maker_plan_yield_percent * 100, 3)}%
                      </div>
                    )}
                  </td>
                  <td className="px-6 py-5 text-right">
                    <span className="text-lg font-semibold text-success number-update">
                      +{fmtNum(g.net_yield_percent * 100)}%
                    </span>
                  </td>
                  <td className="px-6 py-5 text-right text-sm">
                    <div className="font-semibold text-success">${g.estimated_profit_usd.toFixed(2)}</div>
                    <div className="text-[var(--secondary-text)] text-xs">up to ${g.capacity_usd.toFixed(0)}</div>
                  </td>
                  <td className="px-6 py-5 text-right text-sm text-[var(--secondary-text)] font-mono">
                    {fmtNum(g.fee_cost_percent * 100, 3)}%
                  </td>
                  <td className="px-6 py-5 text-right text-sm font-medium">{g.ticks_survived}/{3}</td>
                  <td className="px-6 py-5 text-center">
                    <span className={`inline-block px-3 py-0.5 text-xs font-bold rounded-full ${getFillScoreColor(g.fill_score)}`}>
                      {g.fill_score}
                    </span>
                  </td>
                  <td className="px-6 py-5 text-center text-sm font-medium">
                    {fmtNum(g.confidence * 100, 0)}%
                  </td>
                  <td className="px-6 py-5 text-right text-sm text-[var(--secondary-text)] font-mono">
                    {formatAge(g.gap_age_ms)}
                    {g.staleness_ms < 2000 && <span className="text-emerald-500 ml-1">●</span>}
                  </td>
                </tr>
              ))
            ) : (
              <tr>
                <td colSpan={8} className="px-6 py-16 text-center text-[var(--secondary-text)]">
                  No verified gaps yet.<br />
                  Loops must pass weighted-fill simulation across all three legs and survive 3 consecutive fresh ticks.
                </td>
              </tr>
            )}
          </tbody>
        </table>

        {/* Expanded dossier */}
        {expanded && gaps && gaps.map((g, idx) =>
          expanded === g.id ? (
            <tr key={`detail-${g.id}-${idx}`}>
              <td colSpan={8} className="px-6 py-4 bg-[var(--surface)] border-t border-[var(--accent-border)]">
                <div className="grid grid-cols-2 md:grid-cols-4 gap-4 text-sm">
                  <Detail label="Gross gap" value={`+${fmtNum(g.gross_gap_percent * 100, 4)}%`} />
                  <Detail label="Fee cost (3 legs)" value={`${fmtNum(g.fee_cost_percent * 100, 3)}%`} />
                  <Detail label="Net yield" value={`+${fmtNum(g.net_yield_percent * 100)}%`} />
                  <Detail label="Est. profit (target size)" value={`$${g.estimated_profit_usd.toFixed(2)}`} />
                  <Detail label="True capacity (min depth)" value={`$${g.capacity_usd.toFixed(0)}`} />
                  <Detail label="Slippage @ target size" value={`${fmtNum(g.slippage_percent * 100, 4)}%`} />
                  <Detail label="Gap age at emission" value={formatAge(g.gap_age_ms)} />
                  <Detail label="Data freshness" value={`${g.staleness_ms}ms`} />
                  <Detail label="Confidence (loop history)" value={`${fmtNum(g.confidence * 100, 0)}%`} />
                  <Detail label="Maker-plan floor" value={g.maker_plan_yield_percent > 0 ? `+${fmtNum(g.maker_plan_yield_percent * 100, 3)}%` : 'n/a'} />
                  <Detail label="Fill score" value={g.fill_score} />
                  <Detail label="Ticks survived" value={`${g.ticks_survived}/3`} />
                </div>
                <div className="mt-4 pt-4 border-t border-[var(--accent-border)] grid grid-cols-1 md:grid-cols-3 gap-4 font-mono text-xs">
                  <LegDetail n={1} symbol={g.leg1_symbol} entry={g.leg1_entry_price} fill={g.leg1_fill_price} />
                  <LegDetail n={2} symbol={g.leg2_symbol} entry={g.leg2_entry_price} fill={g.leg2_fill_price} />
                  <LegDetail n={3} symbol={g.leg3_symbol} entry={g.leg3_entry_price} fill={g.leg3_fill_price} />
                </div>
              </td>
            </tr>
          ) : null
        )}
      </div>

      <div className="text-xs text-[var(--secondary-text)] text-center">
        Click any row for the full trade dossier • Fees configurable in Settings (default 0.05% taker × 3 legs) •
        Verified = weighted-fill simulation passed + 3-tick persistence + 2s staleness gate
      </div>
    </div>
  );
}

function Detail({ label, value }: { label: string; value: string }) {
  return (
    <div>
      <div className="text-[11px] uppercase tracking-wide text-[var(--secondary-text)]">{label}</div>
      <div className="font-medium mt-0.5">{value}</div>
    </div>
  );
}

function LegDetail({ n, symbol, entry, fill }: { n: number; symbol: string; entry: number; fill: number }) {
  return (
    <div className="border border-[var(--accent-border)] rounded-lg px-3 py-2">
      <div className="text-[11px] uppercase tracking-wide text-[var(--secondary-text)] mb-1">Leg {n} • {symbol}</div>
      <div>Entry: {entry.toFixed(8)}</div>
      <div>Fill: {fill.toFixed(8)}</div>
    </div>
  );
}
