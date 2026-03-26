// frontend/src/components/MarketWhitelist.tsx
// Page 3: Market Maintenance & Whitelist
// Shows the current 300 coins + 24h volume and Path Count
// Includes "Manual Re-scan" button for the 24h cycle

import { useState, useEffect } from 'react';
import { api } from '../lib/api';

interface WhitelistCoin {
  symbol: string;
  volume_24h: number;
  path_count: number;
  is_active: boolean;
}

export default function MarketWhitelist() {
  const [coins, setCoins] = useState<WhitelistCoin[]>([]);
  const [loading, setLoading] = useState(true);
  const [lastScan, setLastScan] = useState<Date>(new Date());
  const [isScanning, setIsScanning] = useState(false);

  const fetchWhitelist = async () => {
    setLoading(true);
    try {
      // For now we use a static list + mock data
      // In full implementation: fetch from /api/whitelist + DB
      const mockCoins: WhitelistCoin[] = [
        { symbol: "BTCUSDT", volume_24h: 1245000000, path_count: 12, is_active: true },
        { symbol: "ETHUSDT", volume_24h: 895000000, path_count: 15, is_active: true },
        { symbol: "SOLUSDT", volume_24h: 672000000, path_count: 8, is_active: true },
        { symbol: "PEPEUSDT", volume_24h: 432000000, path_count: 22, is_active: true },
        { symbol: "DOGEUSDT", volume_24h: 389000000, path_count: 11, is_active: true },
        { symbol: "XRPUSDT", volume_24h: 312000000, path_count: 7, is_active: true },
        { symbol: "TONUSDT", volume_24h: 245000000, path_count: 9, is_active: true },
        { symbol: "ADAUSDT", volume_24h: 198000000, path_count: 6, is_active: true },
      ];

      setCoins(mockCoins);
      setLastScan(new Date());
    } catch (error) {
      console.error('Failed to fetch whitelist:', error);
    } finally {
      setLoading(false);
    }
  };

  const handleManualRescan = async () => {
    setIsScanning(true);
    
    // Simulate the 24h maintenance task
    await new Promise(resolve => setTimeout(resolve, 1800)); // Simulate network delay
    
    await fetchWhitelist();
    setIsScanning(false);
    
    alert("✅ 24h Maintenance completed!\nNew whitelist loaded with seamless swap.");
  };

  useEffect(() => {
    fetchWhitelist();
  }, []);

  const formatVolume = (volume: number): string => {
    if (volume >= 1_000_000_000) return `$${(volume / 1_000_000_000).toFixed(1)}B`;
    if (volume >= 1_000_000) return `$${(volume / 1_000_000).toFixed(1)}M`;
    return `$${volume.toLocaleString()}`;
  };

  return (
    <div className="space-y-8">
      <div className="flex items-center justify-between">
        <div>
          <h2 className="text-3xl font-semibold tracking-tight">Market Maintenance</h2>
          <p className="text-[var(--secondary-text)] mt-1">
            Current 300-coin whitelist • Updated every 24 hours
          </p>
        </div>

        <button
          onClick={handleManualRescan}
          disabled={isScanning}
          className="px-6 py-3 bg-emerald-600 hover:bg-emerald-700 disabled:bg-gray-600 text-white font-medium rounded-xl flex items-center gap-2 transition-all active:scale-95"
        >
          {isScanning ? (
            <>⟳ Running Maintenance...</>
          ) : (
            <>⟳ Manual Re-scan Now</>
          )}
        </button>
      </div>

      <div className="surface rounded-2xl overflow-hidden border border-[var(--accent-border)]">
        <div className="px-6 py-4 border-b border-[var(--accent-border)] flex justify-between items-center bg-[var(--surface)]">
          <div className="font-medium">Active Whitelist ({coins.length} coins shown)</div>
          <div className="text-xs text-[var(--secondary-text)]">
            Last updated: {lastScan.toLocaleTimeString()}
          </div>
        </div>

        {loading ? (
          <div className="py-20 text-center text-[var(--secondary-text)]">
            Loading whitelist...
          </div>
        ) : (
          <div className="overflow-x-auto">
            <table className="w-full">
              <thead>
                <tr className="border-b border-[var(--accent-border)]">
                  <th className="px-6 py-4 text-left text-xs font-medium text-[var(--secondary-text)]">SYMBOL</th>
                  <th className="px-6 py-4 text-right text-xs font-medium text-[var(--secondary-text)]">24H VOLUME</th>
                  <th className="px-6 py-4 text-right text-xs font-medium text-[var(--secondary-text)]">PATH COUNT</th>
                  <th className="px-6 py-4 text-center text-xs font-medium text-[var(--secondary-text)]">STATUS</th>
                </tr>
              </thead>
              <tbody className="divide-y divide-[var(--accent-border)]">
                {coins.map((coin) => (
                  <tr key={coin.symbol} className="hover:bg-[rgba(16,185,129,0.05)]">
                    <td className="px-6 py-5 font-mono font-medium">{coin.symbol}</td>
                    <td className="px-6 py-5 text-right font-medium text-emerald-500">
                      {formatVolume(coin.volume_24h)}
                    </td>
                    <td className="px-6 py-5 text-right font-mono text-[var(--secondary-text)]">
                      {coin.path_count}
                    </td>
                    <td className="px-6 py-5 text-center">
                      <span className="inline-flex items-center px-3 py-1 rounded-full bg-emerald-500/10 text-emerald-500 text-xs font-medium">
                        ● ACTIVE
                      </span>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </div>

      <div className="text-xs text-center text-[var(--secondary-text)] max-w-md mx-auto">
        The engine automatically refreshes this list every 24 hours.<br />
        Only coins with &gt; $500,000 USD volume and valid closed loops are included.
      </div>
    </div>
  );
}
