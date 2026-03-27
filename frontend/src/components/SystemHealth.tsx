// frontend/src/components/SystemHealth.tsx
// Updated per guide 2.2: Full API Key management + Test Connection + Save Keys

import { useState, useEffect } from 'react';
import { api, TelemetryData } from '../lib/api';

export default function SystemHealth() {
  const [telemetry, setTelemetry] = useState<TelemetryData | null>(null);
  const [apiKey, setApiKey] = useState('');
  const [secretKey, setSecretKey] = useState('');
  const [paperMode, setPaperMode] = useState(true);
  const [statusMessage, setStatusMessage] = useState('');
  const [isSaving, setIsSaving] = useState(false);
  const [loading, setLoading] = useState(true);

  // Fetch telemetry
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
    const interval = setInterval(fetchTelemetry, 5000);
    return () => clearInterval(interval);
  }, []);

  // Save API Keys
  const handleSaveKeys = async () => {
    if (!apiKey || !secretKey) {
      setStatusMessage('❌ Please enter both API Key and Secret');
      return;
    }

    setIsSaving(true);
    setStatusMessage('');

    try {
      // Call backend endpoint (we'll add this in main.rs later if missing)
      const response = await fetch('/api/keys', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ api_key: apiKey, secret_key: secretKey }),
      });

      if (response.ok) {
        setStatusMessage('✅ API Keys saved successfully (encrypted)');
        setApiKey('');
        setSecretKey('');
      } else {
        setStatusMessage('❌ Failed to save keys');
      }
    } catch (error) {
      setStatusMessage('❌ Connection error while saving keys');
      console.error(error);
    } finally {
      setIsSaving(false);
    }
  };

  // Test Connection (placeholder - calls MEXC via backend)
  const handleTestConnection = async () => {
    if (!apiKey || !secretKey) {
      setStatusMessage('❌ Enter keys first');
      return;
    }

    setStatusMessage('🔄 Testing connection to MEXC...');

    try {
      const response = await fetch('/api/test-mexc-connection', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ api_key: apiKey, secret_key: secretKey }),
      });

      if (response.ok) {
        setStatusMessage('✅ MEXC Connection successful!');
      } else {
        setStatusMessage('❌ MEXC Connection failed. Check your keys.');
      }
    } catch (error) {
      setStatusMessage('❌ Failed to reach backend for test');
    }
  };

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
        </div>

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
        </div>

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
        </div>
      </div>

      {/* API Keys Management Section (New from guide) */}
      <div className="surface rounded-3xl p-8 border border-[var(--accent-border)]">
        <h3 className="text-xl font-semibold mb-6">MEXC API Keys (Secure)</h3>
        
        <div className="space-y-6">
          <div>
            <label className="block text-sm font-medium mb-2">API Key</label>
            <input 
              type="text" 
              value={apiKey}
              onChange={(e) => setApiKey(e.target.value)}
              className="surface w-full px-4 py-3 rounded-xl border border-[var(--accent-border)] focus:outline-none focus:border-emerald-500 font-mono"
              placeholder="Enter your MEXC API Key"
            />
          </div>

          <div>
            <label className="block text-sm font-medium mb-2">API Secret</label>
            <input 
              type="password" 
              value={secretKey}
              onChange={(e) => setSecretKey(e.target.value)}
              className="surface w-full px-4 py-3 rounded-xl border border-[var(--accent-border)] focus:outline-none focus:border-emerald-500 font-mono"
              placeholder="Enter your MEXC API Secret"
            />
          </div>

          <div className="flex gap-4">
            <button
              onClick={handleTestConnection}
              className="flex-1 py-3 bg-blue-600 hover:bg-blue-700 text-white rounded-xl font-medium transition"
            >
              Test Connection
            </button>
            <button
              onClick={handleSaveKeys}
              disabled={isSaving}
              className="flex-1 py-3 bg-emerald-600 hover:bg-emerald-700 disabled:bg-gray-600 text-white rounded-xl font-medium transition"
            >
              {isSaving ? 'Saving...' : 'Save Keys'}
            </button>
          </div>

          {statusMessage && (
            <div className={`text-sm p-3 rounded-xl ${statusMessage.includes('✅') ? 'bg-emerald-500/10 text-emerald-500' : 'bg-red-500/10 text-red-500'}`}>
              {statusMessage}
            </div>
          )}
        </div>
      </div>

      {/* Paper Mode & Settings */}
      <div className="surface rounded-3xl p-8 border border-[var(--accent-border)]">
        <h3 className="text-xl font-semibold mb-6">Settings</h3>
        <div className="flex items-center justify-between">
          <div>
            <div className="font-medium">Paper Mode</div>
            <div className="text-sm text-[var(--secondary-text)]">Finding only • No live trading</div>
          </div>
          <button
            onClick={() => setPaperMode(!paperMode)}
            className={`px-6 py-2 rounded-xl font-medium transition-all ${
              paperMode ? 'bg-emerald-500 text-white' : 'bg-gray-200 dark:bg-zinc-700'
            }`}
          >
            {paperMode ? 'ENABLED' : 'DISABLED'}
          </button>
        </div>
      </div>
    </div>
  );
}
