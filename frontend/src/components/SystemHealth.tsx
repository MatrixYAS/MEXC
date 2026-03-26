// frontend/src/components/SystemHealth.tsx
// Page 4: System Health & Hardware
// Real-time gauges for CPU, RAM, WebSocket Latency, Math Loop time
// Settings for API Keys and Paper Mode

import { useState, useEffect } from 'react';
import { api, TelemetryData } from '../lib/api';

export default function SystemHealth() {
  const [telemetry, setTelemetry] = useState<TelemetryData | null>(null);
  const [loading, setLoading] = useState(true);
  const [paperMode, setPaperMode] = useState(true); // Default to Paper Mode

  const fetchTelemetry = async () => {
    try {
      const data = await api.telemetry();
      setTelemetry(data);
    } catch (error) {
      console.error('Failed to fetch telemetry:', error);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    fetchTelemetry();
    const interval = setInterval(fetchTelemetry, 5000); // Update every 5 seconds
    return () => clearInterval(interval);
  }, []);

  const cpuColor = (usage: number) => {
    if (usage > 80) return 'text-red-500';
    if (usage > 60) return 'text-orange-500';
    return 'text-emerald-500';
  };

  const getHealthStatus = (loopTime: number) => {
    if (loopTime > 10) return { status: 'WARNING', color: 'text-orange-500' };
    return { status: 'HEALTHY', color: 'text-emerald-500' };
  };

  return (
    <div className="space-y-8">
      <div>
        <h2 className="text-3xl font-semibold tracking-tight">System Health</h2>
        <p className="text-[var(--secondary-text)] mt-1">
          Real-time monitoring • Zero Degradation Dashboard
        </p>
      </div>

      {/* Status Gauges */}
      <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-6">
        {/* CPU Usage */}
        <div className="surface rounded-3xl p-8 border border-[var(--accent-border)]">
          <div className="flex justify-between items-start mb-6">
            <div>
              <div className="text-sm text-[var(--secondary-text)]">CPU Usage</div>
              <div className={`text-5xl font-semibold mt-3 ${telemetry ? cpuColor(telemetry.cpu_usage) : ''}`}>
                {telemetry ? telemetry.cpu_usage.toFixed(1) : '--'}<span className="text-2xl">%</span>
              </div>
            </div>
            <div className="text-4xl">💻</div>
          </div>
          <div className="h-2 bg-[var(--accent-border)] rounded-full overflow-hidden">
            <div 
              className="h-full bg-emerald-500 transition-all duration-300"
              style={{ width: `${Math.min(telemetry?.cpu_usage || 0, 100)}%` }}
            />
          </div>
        </div>

        {/* RAM Usage */}
        <div className="surface rounded-3xl p-8 border border-[var(--accent-border)]">
          <div className="flex justify-between items-start mb-6">
            <div>
              <div className="text-sm text-[var(--secondary-text)]">RAM Usage</div>
              <div className="text-5xl font-semibold mt-3">
                {telemetry ? telemetry.ram_usage_mb : '--'}<span className="text-2xl">MB</span>
              </div>
            </div>
            <div className="text-4xl">🧠</div>
          </div>
          <div className="text-xs text-[var(--secondary-text)]">
            2-core VPS optimized
          </div>
        </div>

        {/* Math Loop Time */}
        <div className="surface rounded-3xl p-8 border border-[var(--accent-border)]">
          <div className="flex justify-between items-start mb-6">
            <div>
              <div className="text-sm text-[var(--secondary-text)]">Math Loop</div>
              <div className={`text-5xl font-semibold mt-3 ${telemetry ? getHealthStatus(telemetry.math_loop_time_ms).color : ''}`}>
                {telemetry ? telemetry.math_loop_time_ms.toFixed(1) : '--'}<span className="text-2xl">ms</span>
              </div>
            </div>
            <div className="text-4xl">⚡</div>
          </div>
          <div className={`text-sm font-medium ${telemetry ? getHealthStatus(telemetry.math_loop_time_ms).color : ''}`}>
            {telemetry ? getHealthStatus(telemetry.math_loop_time_ms).status : 'LOADING'}
          </div>
          <div className="text-xs text-[var(--secondary-text)] mt-1">Target &lt; 10ms</div>
        </div>

        {/* Active Triangles */}
        <div className="surface rounded-3xl p-8 border border-[var(--accent-border)]">
          <div className="flex justify-between items-start mb-6">
            <div>
              <div className="text-sm text-[var(--secondary-text)]">Active Triangles</div>
              <div className="text-5xl font-semibold mt-3 text-emerald-500">
                {telemetry ? telemetry.active_triangles : '--'}
              </div>
            </div>
            <div className="text-4xl">🔍</div>
          </div>
          <div className="text-xs text-[var(--secondary-text)]">
            Passing 3-tick persistence
          </div>
        </div>
      </div>

      {/* Settings Panel */}
      <div className="surface rounded-3xl p-8 border border-[var(--accent-border)]">
        <h3 className="text-xl font-semibold mb-6">Settings</h3>
        
        <div className="space-y-8">
          {/* Paper Mode Toggle */}
          <div className="flex items-center justify-between">
            <div>
              <div className="font-medium">Paper Mode</div>
              <div className="text-sm text-[var(--secondary-text)]">Finding only • No live trading</div>
            </div>
            <button
              onClick={() => setPaperMode(!paperMode)}
              className={`px-5 py-2 rounded-xl font-medium transition-all ${
                paperMode 
                  ? 'bg-emerald-500 text-white' 
                  : 'bg-gray-200 dark:bg-zinc-700 text-[var(--primary-text)]'
              }`}
            >
              {paperMode ? 'ENABLED' : 'DISABLED'}
            </button>
          </div>

          {/* API Keys (Encrypted fields placeholder) */}
          <div>
            <label className="block text-sm font-medium mb-2">MEXC API Key (Encrypted)</label>
            <input 
              type="password" 
              className="surface w-full px-4 py-3 rounded-xl border border-[var(--accent-border)] focus:outline-none focus:border-emerald-500"
              placeholder="••••••••••••••••"
              disabled 
            />
            <p className="text-xs text-[var(--secondary-text)] mt-1">Stored securely in backend only</p>
          </div>

          <div>
            <label className="block text-sm font-medium mb-2">MEXC API Secret</label>
            <input 
              type="password" 
              className="surface w-full px-4 py-3 rounded-xl border border-[var(--accent-border)] focus:outline-none focus:border-emerald-500"
              placeholder="••••••••••••••••"
              disabled 
            />
          </div>
        </div>

        <div className="mt-10 pt-6 border-t border-[var(--accent-border)] text-xs text-[var(--secondary-text)]">
          Running on 2-core VPS • Optimized for zero allocation in hot path • WAL Mode SQLite
        </div>
      </div>
    </div>
  );
}
