// frontend/src/components/CoinsPage.tsx
// Every coin the scanner is currently using, with live-measured tradability.
// Criterion: a $test_usd taker order must fill BOTH directions on the top 20
// depth levels with slippage <= max_slip_pct. Volume is a proxy; depth is the truth.

import { useState, useEffect } from 'react';
import { api } from '../lib/api';

interface CoinInfo {
  coin: string;
  pair: string;
  vol24h: number;
  depth_usd: number;
  buy_slip_pct: number | null;
  sell_slip_pct: number | null;
  test_usd: number;
  max_slip_pct: number;
  tradeable: boolean;
}

const fmtUsd = (v: number): string => {
  if (v >= 1_000_000_000) return `$${(v / 1_000_000_000).toFixed(1)}B`;
  if (v >= 1_000_000) return `$${(v / 1_000_000).toFixed(1)}M`;
  if (v >= 1_000) return `$${Math.round(v).toLocaleString()}`;
  return v > 0 ? `$${v.toFixed(0)}` : '—';
};

export default function CoinsPage() {
  const [coins, setCoins] = useState<CoinInfo[]>([]);
  const [loading, setLoading] = useState(true);
  const [lastScan, setLastScan] = useState<Date>(new Date());
  const [error, setError] = useState<string | null>(null);

  const fetchCoins = async () => {
    setLoading(true);
    setError(null);
    try {
      const data = await api.coins();
      setCoins(Array.isArray(data) ? data : []);
      setLastScan(new Date());
    } catch (e) {
      setError('Failed to load coins — is the backend running?');
      console.error(e);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    fetchCoins();
    const t = setInterval(fetchCoins, 60_000);
    return () => clearInterval(t);
  }, []);

  const testUsd = coins[0]?.test_usd ?? 100;
  const maxSlip = coins[0]?.max_slip_pct ?? 0.1;
  const tradeableCount = coins.filter(c => c.tradeable).length;

  return (
    <div className="space-y-8">
      <div className="flex items-center justify-between">
        <div>
          <h2 className="text-3xl font-semibold tracking-tight">Coins</h2>
          <p className="text-[var(--secondary-text)] mt-1">
            Every coin the scanner is using right now — measured live from the MEXC order book.
          </p>
        </div>
        <button
          onClick={fetchCoins}
          disabled={loading}
          className="px-6 py-3 bg-emerald-600 hover:bg-emerald-700 disabled:bg-gray-600 text-white font-medium rounded-xl flex items-center gap-2 transition-all active:scale-95"
        >
          {loading ? '⟳ Measuring…' : '⟳ Re-measure Now'}
        </button>
      </div>

      <div className="flex flex-wrap gap-4">
        <div className="surface rounded-2xl border border-[var(--accent-border)] px-5 py-4">
          <div className="text-xs text-[var(--secondary-text)]">Coins in use</div>
          <div className="text-2xl font-semibold">{coins.length}</div>
        </div>
        <div className="surface rounded-2xl border border-[var(--accent-border)] px-5 py-4">
          <div className="text-xs text-[var(--secondary-text)]">Tradeable both ways</div>
          <div className="text-2xl font-semibold text-emerald-500">{tradeableCount}</div>
        </div>
        <div className="surface rounded-2xl border border-[var(--accent-border)] px-5 py-4">
          <div className="text-xs text-[var(--secondary-text)]">Selection criterion</div>
          <div className="text-2xl font-semibold">${testUsd} fills both sides ≤ {maxSlip.toFixed(3)}% slip</div>
        </div>
      </div>

      {error && (
        <div className="p-4 rounded-2xl text-sm bg-red-500/10 text-red-500">{error}</div>
      )}

      <div className="surface rounded-2xl overflow-hidden border border-[var(--accent-border)]">
        <div className="px-6 py-4 border-b border-[var(--accent-border)] flex justify-between items-center bg-[var(--surface)]">
          <div className="font-medium">Live tradability ({coins.length} coins)</div>
          <div className="text-xs text-[var(--secondary-text)]">
            Last measured: {lastScan.toLocaleTimeString()}
          </div>
        </div>

        {loading ? (
          <div className="py-20 text-center text-[var(--secondary-text)]">Measuring live books…</div>
        ) : (
          <div className="overflow-x-auto">
            <table className="w-full">
              <thead>
                <tr className="border-b border-[var(--accent-border)]">
                  <th className="px-6 py-4 text-left text-xs font-medium text-[var(--secondary-text)]">#</th>
                  <th className="px-6 py-4 text-left text-xs font-medium text-[var(--secondary-text)]">COIN</th>
                  <th className="px-6 py-4 text-left text-xs font-medium text-[var(--secondary-text)]">PAIR</th>
                  <th className="px-6 py-4 text-right text-xs font-medium text-[var(--secondary-text)]">24H VOLUME</th>
                  <th className="px-6 py-4 text-right text-xs font-medium text-[var(--secondary-text)]">DEPTH (TOP 20)</th>
                  <th className="px-6 py-4 text-right text-xs font-medium text-[var(--secondary-text)]">BUY ${testUsd} SLIP</th>
                  <th className="px-6 py-4 text-right text-xs font-medium text-[var(--secondary-text)]">SELL ${testUsd} SLIP</th>
                  <th className="px-6 py-4 text-center text-xs font-medium text-[var(--secondary-text)]">TRADEABLE</th>
                </tr>
              </thead>
              <tbody className="divide-y divide-[var(--accent-border)]">
                {coins.map((c, i) => (
                  <tr key={c.pair} className="hover:bg-[rgba(16,185,129,0.05)]">
                    <td className="px-6 py-5 text-right font-mono text-xs text-[var(--secondary-text)]">{i + 1}</td>
                    <td className="px-6 py-5 font-mono font-medium">{c.coin}</td>
                    <td className="px-6 py-5 font-mono text-[var(--secondary-text)]">{c.pair}</td>
                    <td className="px-6 py-5 text-right font-medium">{fmtUsd(c.vol24h)}</td>
                    <td className="px-6 py-5 text-right font-medium text-emerald-500">{fmtUsd(c.depth_usd)}</td>
                    <td className="px-6 py-5 text-right font-mono">
                      {c.buy_slip_pct !== null ? `${c.buy_slip_pct.toFixed(4)}%` : 'no fill'}
                    </td>
                    <td className="px-6 py-5 text-right font-mono">
                      {c.sell_slip_pct !== null ? `${c.sell_slip_pct.toFixed(4)}%` : 'no fill'}
                    </td>
                    <td className="px-6 py-5 text-center">
                      {c.tradeable ? (
                        <span className="inline-flex items-center px-3 py-1 rounded-full bg-emerald-500/10 text-emerald-500 text-xs font-medium">● YES</span>
                      ) : (
                        <span className="inline-flex items-center px-3 py-1 rounded-full bg-amber-500/10 text-amber-500 text-xs font-medium">no</span>
                      )}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}

        {coins.length === 0 && !loading && (
          <div className="py-20 text-center text-[var(--secondary-text)]">
            No coins measured yet.
          </div>
        )}
      </div>

      <div className="text-xs text-center text-[var(--secondary-text)] max-w-xl mx-auto">
        “Tradeable” means a ${testUsd} taker order fills in BOTH directions with slippage at or below {maxSlip.toFixed(3)}%.
        Coins that fail this test are excluded from loop building — this is how the universe is selected, not 24h volume alone.
      </div>
    </div>
  );
}
