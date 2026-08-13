// frontend/src/components/VerifiedExecutions.tsx
// v2: full filterable table of every verified trade the bot found, with all
// dossier fields, a per-day stats strip, and a one-click CSV export.

import { useEffect, useState } from 'react';
import { api, Opportunity } from '../lib/api';

export default function VerifiedExecutions() {
  const [rows, setRows] = useState<Opportunity[]>([]);
  const [loading, setLoading] = useState(true);
  const [stats, setStats] = useState({ gaps_found: 0, avg_yield_pct: 0, total_estimated_profit_usd: 0 });
  const [minYield, setMinYield] = useState(0);
  const [exporting, setExporting] = useState(false);

  useEffect(() => {
    load();
  }, []);

  const load = async () => {
    setLoading(true);
    try {
      const [r, s] = await Promise.all([api.allOpportunities(), api.todayStats()]);
      setRows(r);
      setStats(s);
    } catch (e: any) {
      console.error(e);
    } finally {
      setLoading(false);
    }
  };

  const handleExport = async () => {
    setExporting(true);
    try {
      const blob = await api.exportCsv();
      const url = URL.createObjectURL(blob);
      const a = document.createElement('a');
      a.href = url;
      a.download = `mexc_trades_${new Date().toISOString().slice(0, 10)}.csv`;
      a.click();
      URL.revokeObjectURL(url);
    } catch (e: any) {
      console.error('Export failed:', e);
    } finally {
      setExporting(false);
    }
  };

  const filtered = rows.filter((r) => r.net_yield_percent * 100 >= minYield);

  const fmtAge = (ms: number) => (ms < 1000 ? `${ms}ms` : `${(ms / 1000).toFixed(1)}s`);

  return (
    <div className="space-y-6">
      <div className="flex items-end justify-between flex-wrap gap-4">
        <div>
          <h2 className="text-3xl font-semibold tracking-tight">Verified Executions</h2>
          <p className="text-[var(--secondary-text)] mt-1">
            Every opportunity the bot verified (3-tick persistence + weighted-fill simulation)
          </p>
        </div>
        <button
          onClick={handleExport}
          disabled={exporting || rows.length === 0}
          className="px-6 py-3 rounded-xl font-medium border border-emerald-500/50 text-emerald-400 hover:bg-emerald-500/10 disabled:opacity-40 transition"
        >
          {exporting ? 'Exporting…' : '⬇ Export CSV'}
        </button>
      </div>

      {/* Today's stats strip */}
      <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
        <StatCard label="Gaps found today" value={String(stats.gaps_found)} />
        <StatCard label="Avg net yield" value={`+${stats.avg_yield_pct.toFixed(3)}%`} />
        <StatCard label="Total estimated profit" value={`$${stats.total_estimated_profit_usd.toFixed(2)}`} />
      </div>

      <div className="flex items-center gap-3">
        <label className="text-sm text-[var(--secondary-text)]">Min net yield %:</label>
        <input
          type="number"
          step="0.01"
          min="0"
          value={minYield}
          onChange={(e) => setMinYield(parseFloat(e.target.value) || 0)}
          className="w-28 px-3 py-2 rounded-lg border border-[var(--accent-border)] bg-transparent focus:outline-none focus:border-emerald-500 font-mono text-sm"
        />
      </div>

      <div className="surface rounded-2xl overflow-x-auto border border-[var(--accent-border)]">
        <table className="w-full text-sm min-w-[1100px]">
          <thead>
            <tr className="border-b border-[var(--accent-border)]">
              <Th>TIME</Th><Th>PATH</Th><Th>NET %</Th><Th>GROSS %</Th><Th>FEES %</Th>
              <Th>PROFIT $</Th><Th>CAPACITY $</Th><Th>TICKS</Th><Th>FILL</Th>
              <Th>SLIP %</Th><Th>CONF %</Th><Th>AGE</Th><Th>STALE ms</Th>
            </tr>
          </thead>
          <tbody className="divide-y divide-[var(--accent-border)]">
            {loading ? (
              <tr><td colSpan={13} className="px-6 py-16 text-center text-[var(--secondary-text)]">Loading…</td></tr>
            ) : filtered.length === 0 ? (
              <tr><td colSpan={13} className="px-6 py-16 text-center text-[var(--secondary-text)]">
                No verified trades yet. The table fills as the bot confirms real loops.
              </td></tr>
            ) : (
              filtered.map((r) => (
                <tr key={r.id} className="hover:bg-[rgba(16,185,129,0.05)]">
                  <Td className="text-[var(--secondary-text)] font-mono">{new Date(r.detected_at).toLocaleTimeString()}</Td>
                  <Td className="font-mono font-medium">{r.path}</Td>
                  <Td className="font-semibold text-success">+{(r.net_yield_percent * 100).toFixed(3)}%</Td>
                  <Td className="text-[var(--secondary-text)]">+{(r.gross_gap_percent * 100).toFixed(4)}%</Td>
                  <Td className="text-[var(--secondary-text)]">{(r.fee_cost_percent * 100).toFixed(3)}%</Td>
                  <Td className="font-medium text-success">${r.estimated_profit_usd.toFixed(2)}</Td>
                  <Td>${r.capacity_usd.toFixed(0)}</Td>
                  <Td>{r.ticks_survived}/3</Td>
                  <Td><span className="badge-C">{r.fill_score}</span></Td>
                  <Td>{(r.slippage_percent * 100).toFixed(4)}%</Td>
                  <Td>{(r.confidence * 100).toFixed(0)}%</Td>
                  <Td className="font-mono">{fmtAge(r.gap_age_ms)}</Td>
                  <Td className="font-mono text-[var(--secondary-text)]">{r.staleness_ms}</Td>
                </tr>
              ))
            )}
          </tbody>
        </table>
      </div>

      <div className="text-xs text-[var(--secondary-text)]">
        {rows.length} total rows in database (retention setting controls auto-pruning).
        Profit figures are estimates from weighted-fill simulation at the configured target volume.
      </div>
    </div>
  );
}

function StatCard({ label, value }: { label: string; value: string }) {
  return (
    <div className="surface rounded-2xl border border-[var(--accent-border)] px-6 py-5">
      <div className="text-xs uppercase tracking-wide text-[var(--secondary-text)]">{label}</div>
      <div className="text-2xl font-semibold mt-1">{value}</div>
    </div>
  );
}

function Th({ children }: { children: React.ReactNode }) {
  return <th className="px-4 py-3 text-left text-[11px] font-medium text-[var(--secondary-text)] uppercase tracking-wide">{children}</th>;
}

function Td({ children, className = '' }: { children: React.ReactNode; className?: string }) {
  return <td className={`px-4 py-3 ${className}`}>{children}</td>;
}
