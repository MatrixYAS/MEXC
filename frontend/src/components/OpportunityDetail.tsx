// frontend/src/components/OpportunityDetail.tsx
// Full-page dossier for a single verified opportunity (route /opportunity/:id).
// Shows detection timestamp, complete loop math, the optimal trade size,
// size/yield decay curve, and live per-leg prices polled every 1.5s.

import { useState, useEffect } from 'react';
import { useParams, Link } from 'react-router-dom';
import { Opportunity, api } from '../lib/api';

export default function OpportunityDetail() {
  const { id } = useParams<{ id: string }>();
  const [opp, setOpp] = useState<Opportunity | null>(null);
  const [liveBooks, setLiveBooks] = useState<Record<string, any>>({});
  const [tick, setTick] = useState(0);

  useEffect(() => {
    let alive = true;
    (async () => {
      try {
        const all = await api.allOpportunities();
        const found = all.find((o) => o.id === id);
        if (alive) setOpp(found ?? null);
      } catch {
        if (alive) setOpp(null);
      }
    })();
    return () => { alive = false; };
  }, [id]);

  // Live prices for the three legs — same engine WebSocket feed, polled.
  useEffect(() => {
    const poll = async () => {
      if (!opp) return;
      const syms = [opp.leg1_symbol, opp.leg2_symbol, opp.leg3_symbol].join(',');
      try {
        const res = await fetch(`/api/live-books?symbols=${encodeURIComponent(syms)}`);
        if (res.ok) {
          const json = await res.json();
          const map: Record<string, any> = {};
          for (const b of json.books || []) map[b.symbol] = b;
          setLiveBooks(map);
        }
      } catch { /* background */ }
    };
    poll();
    const iv = setInterval(poll, 1500);
    return () => clearInterval(iv);
  }, [opp, tick]);

  // Keep live prices fresh on tab focus as well.
  useEffect(() => {
    const onVis = () => { if (document.visibilityState === 'visible') setTick((t) => t + 1); };
    window.addEventListener('visibilitychange', onVis);
    return () => window.removeEventListener('visibilitychange', onVis);
  }, []);

  const fmtPrice = (p: number): string => {
    if (p === 0) return '—';
    if (p >= 100) return p.toFixed(2);
    if (p >= 1) return p.toFixed(4);
    if (p >= 0.01) return p.toFixed(6);
    return p.toFixed(8);
  };
  const fmtNum = (n: number, digits = 2) => n.toFixed(digits);

  if (!opp) {
    return (
      <div className="space-y-4">
        <Link to="/" className="text-sm text-[var(--secondary-text)] hover:text-[var(--primary-text)]">&larr; Back to Live Pulse</Link>
        <div className="surface rounded-2xl p-10 text-center text-[var(--secondary-text)]">
          Opportunity not found (it may have aged out of the recent window).
        </div>
      </div>
    );
  }

  const curve = JSON.parse(opp.size_curve_json || '[]') as { size_usd: number; net_yield: number }[];

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between flex-wrap gap-3">
        <Link to="/" className="text-sm text-[var(--secondary-text)] hover:text-[var(--primary-text)]">&larr; Back to Live Pulse</Link>
        <div className="text-sm text-[var(--secondary-text)] font-mono">
          Detected at {opp.detected_at ? new Date(opp.detected_at).toLocaleString() : '—'}
        </div>
      </div>

      <div className="surface rounded-2xl border border-[var(--accent-border)] p-6">
        <div className="text-[11px] uppercase tracking-wide text-[var(--secondary-text)] mb-1">Trade loop</div>
        <div className="font-mono text-lg font-semibold text-[var(--primary-text)] break-words">{opp.path}</div>
        <div className="grid grid-cols-2 md:grid-cols-5 gap-4 mt-6 text-sm">
          <KPI label="Net yield" value={`+${fmtNum(opp.net_yield_percent)}%`} color="text-success" />
          <KPI label="Gross gap" value={`+${fmtNum(opp.gross_gap_percent, 4)}%`} />
          <KPI label="Fee cost (3 legs)" value={`${fmtNum(opp.fee_cost_percent, 3)}%`} />
          <KPI label="Slippage (avg/leg)" value={`${fmtNum(opp.slippage_percent, 4)}%`} />
          <KPI label="Maker-plan floor" value={opp.maker_plan_yield_percent > 0 ? `+${fmtNum(opp.maker_plan_yield_percent, 3)}%` : 'n/a'} />
        </div>
      </div>

      <div className="surface rounded-2xl border border-[var(--accent-border)] p-6">
        <div className="text-[11px] uppercase tracking-wide text-[var(--secondary-text)] mb-3">Optimal trade — the number that matters</div>
        <div className="grid grid-cols-2 md:grid-cols-4 gap-4 text-sm">
          <KPI label="Optimal trade size" value={`$${opp.optimal_size_usd.toFixed(0)}`} highlight />
          <KPI label="Yield at optimal size" value={`+${fmtNum(opp.optimal_net_yield_percent, 2)}%`} highlight />
          <KPI label="Estimated profit" value={`$${opp.estimated_profit_usd.toFixed(2)}`} highlight />
          <KPI label="WIN %" value={`${fmtNum(opp.confidence, 0)}%`} />
          <KPI label="Ticks survived" value={`${opp.ticks_survived} / 3`} hint="Consecutive fresh-tick validations (required: 3)" />
          <KPI label="Fill score" value={opp.fill_score} hint="Liquidity grade A–F across the 3 legs" />
          <KPI label="Gap age at emission" value={`${Math.floor(opp.gap_age_ms / 1000)}s`} hint="How long the gap held before emission" />
          <KPI label="Book freshness" value={`${opp.staleness_ms}ms`} hint="Snapshot age at emission — >2s is rejected" />
        </div>
        <p className="text-[11px] text-[var(--secondary-text)] opacity-70 mt-4">
          Optimal size is binary-searched from real order-book depth: it is the largest USD size where all three legs still fill ≥95% while net yield stays at/above your threshold.
          Profit = optimal size × net yield at that exact size — profit can never exceed size × yield. If the gap looks huge but profit is small, the books can only absorb a small trade before slippage eats the gap.
        </p>
      </div>

      <SizeCurveDetail curve={curve} threshold={opp.optimal_net_yield_percent} />

      <div className="surface rounded-2xl border border-[var(--accent-border)] p-6">
        <div className="text-[11px] uppercase tracking-wide text-[var(--secondary-text)] mb-3">Live prices — same WebSocket feed, polled every 1.5s</div>
        <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
          <LegCard n={1} symbol={opp.leg1_symbol} entry={opp.leg1_entry_price} fill={opp.leg1_fill_price} book={liveBooks[opp.leg1_symbol]} fmtPrice={fmtPrice} />
          <LegCard n={2} symbol={opp.leg2_symbol} entry={opp.leg2_entry_price} fill={opp.leg2_fill_price} book={liveBooks[opp.leg2_symbol]} fmtPrice={fmtPrice} />
          <LegCard n={3} symbol={opp.leg3_symbol} entry={opp.leg3_entry_price} fill={opp.leg3_fill_price} book={liveBooks[opp.leg3_symbol]} fmtPrice={fmtPrice} />
        </div>
      </div>
    </div>
  );
}

function KPI({ label, value, color, highlight, hint }: { label: string; value: string; color?: string; highlight?: boolean; hint?: string }) {
  return (
    <div title={hint}>
      <div className="text-[11px] uppercase tracking-wide text-[var(--secondary-text)]">{label}</div>
      <div className={`font-semibold mt-0.5 ${highlight ? 'text-emerald-400' : color ?? ''}`} title={hint}>{value}</div>
    </div>
  );
}

function LegCard({ n, symbol, entry, fill, book, fmtPrice }: { n: number; symbol: string; entry: number; fill: number; book: any; fmtPrice: (p: number) => string }) {
  const best = book ? (book.asks?.[0]?.[0] ?? book.bids?.[0]?.[0] ?? 0) : 0;
  const stale = book ? Date.now() - (book.ts_ms ?? Date.now()) : null;
  return (
    <div className="rounded-xl border border-[var(--accent-border)] bg-[var(--surface)] p-4">
      <div className="text-[11px] uppercase tracking-wide text-[var(--secondary-text)]">Leg {n} — {symbol}</div>
      <div className="text-xl font-mono font-semibold mt-1">{fmtPrice(best || fill || entry)}</div>
      <div className="text-[11px] text-[var(--secondary-text)] mt-1">
        entry {fmtPrice(entry)} • fill {fmtPrice(fill)}
      </div>
      <div className="text-[10px] text-[var(--secondary-text)] mt-1">
        book {book ? `asks ${fmtPrice(book.asks?.[0]?.[0] ?? 0)} / bids ${fmtPrice(book.bids?.[0]?.[0] ?? 0)}` : '—'}
        {stale !== null ? ` • ${stale}ms old` : ''}
      </div>
    </div>
  );
}

function SizeCurveDetail({ curve, threshold }: { curve: { size_usd: number; net_yield: number }[]; threshold: number }) {
  if (curve.length === 0) {
    return (
      <div className="surface rounded-2xl border border-[var(--accent-border)] p-6 text-sm text-[var(--secondary-text)]">
        No size/yield curve data for this opportunity.
      </div>
    );
  }
  const w = 700, h = 240, pad = 44;
  const maxS = Math.max(...curve.map((p) => p.size_usd));
  const yields = curve.map((p) => p.net_yield * 100);
  const maxY = Math.max(...yields, threshold * 100);
  const x = (i: number) => pad + (i / (curve.length - 1)) * (w - 2 * pad);
  const y = (v: number) => h - pad - ((v / maxY) * (h - 2 * pad));
  const pathD = curve.map((p, i) => `${i === 0 ? 'M' : 'L'} ${x(i)} ${y(p.net_yield * 100)}`).join(' ');
  return (
    <div className="surface rounded-2xl border border-[var(--accent-border)] p-6">
      <div className="text-[11px] uppercase tracking-wide text-[var(--secondary-text)] mb-2">Size / yield decay — honest slippage reality</div>
      <svg viewBox={`0 0 ${w} ${h}`} className="w-full h-60">
        {/* threshold line */}
        <line x1={pad} y1={y(threshold * 100)} x2={w - pad} y2={y(threshold * 100)} stroke="rgb(251 191 36 / 0.5)" strokeWidth="1" strokeDasharray="6 4" />
        <text x={w - pad} y={y(threshold * 100) - 4} textAnchor="end" fontSize="10" fill="rgb(251 191 36)">threshold {threshold.toFixed(2)}%</text>
        {curve.map((p, i) => (
          <g key={i}>
            <circle cx={x(i)} cy={y(p.net_yield * 100)} r="4" fill={p.net_yield * 100 >= threshold * 100 ? '#10b981' : '#ef4444'} />
            <text x={x(i)} y={h - 14} textAnchor="middle" fontSize="10" fill="var(--secondary-text)">${p.size_usd >= 1000 ? `${(p.size_usd / 1000).toFixed(1)}k` : p.size_usd.toFixed(0)}</text>
            <text x={x(i)} y={y(p.net_yield * 100) - 8} textAnchor="middle" fontSize="10" fill="var(--secondary-text)">{(p.net_yield * 100).toFixed(2)}%</text>
          </g>
        ))}
        <path d={pathD} fill="none" stroke="#10b981" strokeWidth="1.5" opacity="0.6" />
      </svg>
      <p className="text-[11px] text-[var(--secondary-text)] mt-1">
        Green = still profitable above your threshold at that size. Red = slippage ate the gap. The optimal trade size is the largest green point.
      </p>
    </div>
  );
}
