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
  const [liveBooks, setLiveBooks] = useState<Record<string, any>>({});

  useEffect(() => {
    if (gaps && gaps.length > 0) {
      setLastUpdate(new Date());
      lastRef.current = Date.now();
    }
  }, [gaps]);

  // Poll live order books (engine books fed by the same WebSocket stream) for
  // any expanded opportunity's three legs, so the dossier shows live prices.
  useEffect(() => {
    const poll = async () => {
      if (!expanded || !gaps) return;
      const g = gaps.find((x) => x.id === expanded);
      if (!g) return;
      const syms = [g.leg1_symbol, g.leg2_symbol, g.leg3_symbol].join(',');
      try {
        const res = await fetch(`/api/live-books?symbols=${encodeURIComponent(syms)}`);
        if (res.ok) {
          const json = await res.json();
          const map: Record<string, any> = {};
          for (const b of json.books || []) map[b.symbol] = b;
          setLiveBooks(map);
        }
      } catch { /* background poll; ignore */ }
    };
    poll();
    const iv = setInterval(poll, 1500);
    return () => clearInterval(iv);
  }, [expanded, gaps]);

  const fmtPrice = (p: number): string => {
    if (p === 0) return '—';
    if (p >= 100) return p.toFixed(2);
    if (p >= 1) return p.toFixed(4);
    if (p >= 0.01) return p.toFixed(6);
    return p.toFixed(8);
  };

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
            LIVE market data (WebSocket 100ms + REST re-seed) • SIMULATED execution (no orders placed) • nothing trades on numbers older than one tick
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
                        maker floor: +{fmtNum(g.maker_plan_yield_percent, 3)}%
                      </div>
                    )}
                  </td>
                  <td className="px-6 py-5 text-right">
                    <span className="text-lg font-semibold text-success number-update">
                      +{fmtNum(g.net_yield_percent)}%
                    </span>
                  </td>
                  <td className="px-6 py-5 text-right text-sm">
                    <div className="font-semibold text-success">${g.estimated_profit_usd.toFixed(2)}</div>
                    <div className="text-[var(--secondary-text)] text-xs">up to ${g.capacity_usd.toFixed(0)}</div>
                  </td>
                  <td className="px-6 py-5 text-right text-sm text-[var(--secondary-text)] font-mono">
                    {fmtNum(g.fee_cost_percent, 3)}%
                  </td>
                  <td className="px-6 py-5 text-right text-sm font-medium">{g.ticks_survived}/3</td>
                  <td className="px-6 py-5 text-center">
                    <span className={`inline-block px-3 py-0.5 text-xs font-bold rounded-full ${getFillScoreColor(g.fill_score)}`}>
                      {g.fill_score}
                    </span>
                  </td>
                  <td className="px-6 py-5 text-center text-sm font-medium">
                    {fmtNum(g.confidence, 0)}%
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
                  <Detail label="Gross gap" value={`+${fmtNum(g.gross_gap_percent, 4)}%`} hint="Raw loop product vs 1.0 before any costs — includes the slippage you pay walking the book." />
                      <Detail label="Fee cost (3 legs)" value={`${fmtNum(g.fee_cost_percent, 3)}%`} hint="MEXC spot taker fee 0.05% per leg × 3 legs = 0.15%. Taker fee × 3 legs. Configurable in Settings." />
                  <Detail label="Net yield" value={`+${fmtNum(g.net_yield_percent)}%`} hint="Gross gap − fees − slippage buffer. This is the gap that actually survives costs." />
                  <Detail label="Optimal size" value={`$${g.optimal_size_usd.toFixed(0)} @ +${fmtNum(g.optimal_net_yield_percent, 2)}%`} highlight hint="Largest size whose net yield stays at/above the threshold given real book depth. Trade at this size or less." />
                  <Detail label="Est. profit" value={`$${g.estimated_profit_usd.toFixed(2)}`} hint="Optimal size × net yield at that size. Low profit at high yield = small size cap (depth), not a math error." />
                  <Detail label="True capacity (min depth)" value={`$${g.capacity_usd.toFixed(0)}`} hint="USD fillable across the shallowest leg — the hard ceiling." />
                  <Detail label="Slippage @ target size" value={`${fmtNum(g.slippage_percent, 4)}%`} hint="Average slippage of the 3 legs computed separately: each leg's weighted fill price vs its best price. Paid per leg, measured per leg, already inside the gross gap." />
                  <Detail label="Gap age at emission" value={formatAge(g.gap_age_ms)} hint="How long the gap was open and confirmed before emission." />
                  <Detail label="Data freshness" value={`${g.staleness_ms}ms`} hint="Age of the order-book snapshot used. >2s is rejected." />
                  <Detail label="Confidence (CONF %)" value={`${fmtNum(g.confidence, 0)}%`} hint="Weighted mix: data freshness + weighted-fill grade + tick persistence." />
                  <Detail label="Ticks survived (TICKS)" value={`${g.ticks_survived}`} hint="Consecutive 100ms WebSocket updates the gap persisted on fresh books. Required = 3 (Settings). Every tick re-validates the live fill simulation — ghosts die here." />
                  <Detail label="Maker-plan floor" value={g.maker_plan_yield_percent > 0 ? `+${fmtNum(g.maker_plan_yield_percent, 3)}%` : 'n/a'} hint="What the gap would be with 0% maker (limit-order) fees — upside if you trade as maker." />
                  <Detail label="Fill score" value={g.fill_score} hint="Liquidity grade A–F of the top 5 depth levels averaged across the 3 legs." />
                </div>
                {/* Size / yield curve */}
                <SizeCurveChart curveJson={g.size_curve_json} />
                <div className="mt-4 pt-4 border-t border-[var(--accent-border)]">
                  <div className="text-[11px] uppercase tracking-wide text-[var(--secondary-text)] mb-2">Live prices — same WebSocket feed, polled every 1.5s</div>
                  <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
                    <LegLive n={1} symbol={g.leg1_symbol} entry={g.leg1_entry_price} fill={g.leg1_fill_price} book={liveBooks[g.leg1_symbol]} fmtPrice={fmtPrice} />
                    <LegLive n={2} symbol={g.leg2_symbol} entry={g.leg2_entry_price} fill={g.leg2_fill_price} book={liveBooks[g.leg2_symbol]} fmtPrice={fmtPrice} />
                    <LegLive n={3} symbol={g.leg3_symbol} entry={g.leg3_entry_price} fill={g.leg3_fill_price} book={liveBooks[g.leg3_symbol]} fmtPrice={fmtPrice} />
                  </div>
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

function Detail({ label, value, hint, highlight }: { label: string; value: string; hint?: string; highlight?: boolean }) {
  return (
    <div>
      <div className="text-[11px] uppercase tracking-wide text-[var(--secondary-text)]" title={hint}>{label}</div>
      <div className={`font-medium mt-0.5 ${highlight ? 'text-emerald-400' : ''}`} title={hint}>{value}</div>
      {hint && <div className="text-[10px] text-[var(--secondary-text)] opacity-60 mt-0.5">{hint}</div>}
    </div>
  );
}

/// Size / yield curve: how net yield decays as trade size eats book depth.
function SizeCurveChart({ curveJson }: { curveJson: string }) {
  let points: { size_usd: number; net_yield: number }[] = [];
  try {
    points = JSON.parse(curveJson || '[]');
  } catch { points = []; }
  if (points.length === 0) {
    return (
      <div className="mt-4 pt-3 border-t border-[var(--accent-border)] text-[11px] text-[var(--secondary-text)]">
        Size curve: unavailable (books changed before sizing).
      </div>
    );
  }
  const maxBar = Math.max(...points.map((p) => Math.abs(p.net_yield)), 0.0001);
  return (
    <div className="mt-4 pt-3 border-t border-[var(--accent-border)]">
      <div className="text-[11px] uppercase tracking-wide text-[var(--secondary-text)] mb-2">Yield vs size — slippage cost at each size (green = above threshold)</div>
      <div className="flex items-end gap-2 h-24">
        {points.map((p, i) => {
          const pct = p.net_yield.toFixed(3);
          const h = Math.max((Math.abs(p.net_yield) / maxBar) * 100, 2);
          const above = p.net_yield >= 0;
          return (
            <div key={i} className="flex-1 flex flex-col items-center justify-end h-full">
              <div className="text-[9px] text-[var(--secondary-text)] mb-1" title={`$${p.size_usd.toFixed(0)}`}>${p.size_usd.toFixed(0)}</div>
              <div
                className="w-full rounded-t"
                style={{ height: `${h}%`, background: above ? 'rgb(16,185,129)' : 'rgb(239,68,68)', opacity: 0.85 }}
                title={`$${p.size_usd.toFixed(0)} → ${pct}% net`}
              />
              <div className="text-[9px] font-mono mt-1" style={{ color: above ? 'rgb(16,185,129)' : 'rgb(239,68,68)' }}>{pct}%</div>
            </div>
          );
        })}
      </div>
      <div className="text-[10px] text-[var(--secondary-text)] mt-1">Higher size = deeper book walk = bigger slippage. Red bars turn the gap into a loss at that size.</div>
    </div>
  );
}

/// Live per-leg price chip: best bid/ask from the engine books (same WS feed),
/// plus the entry/fill prices at detection time for comparison.
function LegLive({ n, symbol, entry, fill, book, fmtPrice }: { n: number; symbol: string; entry: number; fill: number; book?: any; fmtPrice: (p: number) => string }) {
  const stale = typeof book?.stale_ms === 'number' && book.stale_ms > 2000;
  const drift = book && entry > 0 ? ((book.last - entry) / entry) * 100 : null;
  return (
    <div className="border border-[var(--accent-border)] rounded-lg px-3 py-2 font-mono text-xs">
      <div className="flex items-center justify-between text-[11px] uppercase tracking-wide text-[var(--secondary-text)] mb-1">
        <span>Leg {n} • {symbol}</span>
        <span className={`font-sans ${stale ? 'text-red-400' : 'text-emerald-500'}`}>{stale ? '● stale' : '● live'}</span>
      </div>
      <div className="grid grid-cols-2 gap-x-3 gap-y-0.5">
        <div>Best bid: <b>{fmtPrice(book?.best_bid ?? 0)}</b></div>
        <div>Best ask: <b>{fmtPrice(book?.best_ask ?? 0)}</b></div>
        <div>Det. entry: {fmtPrice(entry)}</div>
        <div>Det. fill: {fmtPrice(fill)}</div>
        {drift !== null && (
          <div className={`col-span-2 ${drift > 0 ? 'text-emerald-400' : drift < 0 ? 'text-red-400' : 'text-[var(--secondary-text)]'}`}>
            Since detection: {drift > 0 ? '+' : ''}{drift.toFixed(3)}%
          </div>
        )}
        {book?.depth && (
          <div className="col-span-2 pt-1 text-[10px] text-[var(--secondary-text)]">
            Depth top: bid ${(book.depth.bids[0] ? book.depth.bids[0][1] * book.depth.bids[0][0] : 0).toFixed(0)} | ask ${(book.depth.asks[0] ? book.depth.asks[0][1] * book.depth.asks[0][0] : 0).toFixed(0)}
          </div>
        )}
      </div>
    </div>
  );
}
