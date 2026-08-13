// frontend/src/components/SettingsPage.tsx
// v2: All settings configurable in-app + verify-before-save MEXC credential flow.
// Save buttons only unlock after server-side validation (settings) or after all
// credential tests pass (keys).

import { useEffect, useState } from 'react';
import { api, SettingsSnapshot, KeyTestResult } from '../lib/api';

export default function SettingsPage() {
  const [settings, setSettings] = useState<SettingsSnapshot | null>(null);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const [isError, setIsError] = useState(false);

  // Key verification state
  const [apiKey, setApiKey] = useState('');
  const [secretKey, setSecretKey] = useState('');
  const [verifying, setVerifying] = useState(false);
  const [testResults, setTestResults] = useState<KeyTestResult[] | null>(null);
  const [allPassed, setAllPassed] = useState(false);
  const [hasKeys, setHasKeys] = useState(false);

  useEffect(() => {
    loadSettings();
    checkKeys();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const loadSettings = async () => {
    try {
      const s = await api.settings();
      setSettings(s);
    } catch (e: any) {
      setMessage(`Failed to load settings: ${e.message}`);
      setIsError(true);
    } finally {
      setLoading(false);
    }
  };

  const checkKeys = async () => {
    try {
      const present = await api.hasApiKeys();
      setHasKeys(present);
      const tests = await api.keyTests();
      if (tests.length > 0) setTestResults(tests);
    } catch {
      // ignore — no keys yet
    }
  };

  const handleSaveSettings = async () => {
    if (!settings) return;
    setSaving(true);
    setMessage(null);
    try {
      await api.saveSettings({
        min_profit_threshold: settings.min_profit_threshold,
        taker_fee: settings.taker_fee,
        slippage_buffer: settings.slippage_buffer,
        target_volume_usd: settings.target_volume_usd,
        tick_interval_ms: settings.tick_interval_ms,
        required_ticks: settings.required_ticks,
        max_whitelist: settings.max_whitelist,
        min_24h_volume_usd: settings.min_24h_volume_usd,
        retention_days: settings.retention_days,
        scan_paused: settings.scan_paused,
      });
      setMessage('✅ Settings saved and applied live (no restart needed).');
      setIsError(false);
    } catch (e: any) {
      setMessage(`❌ ${e.message}`);
      setIsError(true);
    } finally {
      setSaving(false);
    }
  };

  const handleVerify = async () => {
    setVerifying(true);
    setTestResults(null);
    setAllPassed(false);
    setMessage(null);
    try {
      const result = await api.verifyKeys({ api_key: apiKey, secret_key: secretKey });
      setTestResults(result.results);
      setAllPassed(result.all_passed);
      setMessage(
        result.all_passed
          ? '✅ All tests passed — Safe to save these credentials.'
          : '❌ One or more tests failed — review details before saving.'
      );
      setIsError(!result.all_passed);
    } catch (e: any) {
      setMessage(`❌ Verification failed: ${e.message}`);
      setIsError(true);
    } finally {
      setVerifying(false);
    }
  };

  const handleSaveKeys = async () => {
    try {
      await api.saveApiKeys({ api_key: apiKey, secret_key: secretKey });
      setMessage('✅ API keys saved (encrypted at rest). The scanner can now execute trades.');
      setIsError(false);
      setHasKeys(true);
      setApiKey('');
      setSecretKey('');
    } catch (e: any) {
      setMessage(`❌ ${e.message}`);
      setIsError(true);
    }
  };

  const handleDeleteKeys = async () => {
    if (!window.confirm('Remove saved MEXC API keys? The bot will stop being able to trade.')) return;
    try {
      await api.deleteApiKeys();
      setHasKeys(false);
      setTestResults(null);
      setMessage('API keys removed.');
    } catch (e: any) {
      setMessage(`❌ ${e.message}`);
      setIsError(true);
    }
  };

  if (loading) {
    return <div className="surface rounded-2xl p-10 text-center text-[var(--secondary-text)]">Loading settings…</div>;
  }
  if (!settings) {
    return <div className="surface rounded-2xl p-10 text-center text-red-500">Could not load settings.</div>;
  }

  const update = (patch: Partial<SettingsSnapshot>) => setSettings({ ...settings, ...patch });

  return (
    <div className="space-y-8 max-w-4xl">
      <div>
        <h2 className="text-3xl font-semibold tracking-tight">Settings</h2>
        <p className="text-[var(--secondary-text)] mt-1">
          Everything is configurable in-app. Changes apply live — no restart needed.
        </p>
      </div>

      {message && (
        <div className={`p-4 rounded-xl border text-sm ${isError ? 'border-red-500/50 bg-red-500/5 text-red-400' : 'border-emerald-500/50 bg-emerald-500/5 text-emerald-400'}`}>
          {message}
        </div>
      )}

      {/* ────────── MEXC Credentials (verify-before-save) ────────── */}
      <section className="surface rounded-2xl border border-[var(--accent-border)] p-6 space-y-5">
        <h3 className="text-xl font-semibold">MEXC API Credentials</h3>
        <p className="text-sm text-[var(--secondary-text)]">
          Credentials are <b>verified with real signed API calls before they can be saved</b>.
          The bot needs no key to scan; keys are only for trade execution.
          Note: Hugging Face container IPs change on restart — do NOT restrict the key to an IP unless you run your own VPS.
        </p>

        <div className="grid md:grid-cols-2 gap-4">
          <div>
            <label className="text-xs uppercase tracking-wide text-[var(--secondary-text)]">API Key</label>
            <input
              type="text"
              value={apiKey}
              onChange={(e) => setApiKey(e.target.value)}
              placeholder="mx0v..."
              className="w-full mt-1 px-4 py-3 rounded-xl border border-[var(--accent-border)] bg-transparent focus:outline-none focus:border-emerald-500 font-mono text-sm"
              autoComplete="off"
            />
          </div>
          <div>
            <label className="text-xs uppercase tracking-wide text-[var(--secondary-text)]">Secret Key</label>
            <input
              type="password"
              value={secretKey}
              onChange={(e) => setSecretKey(e.target.value)}
              placeholder="••••••••"
              className="w-full mt-1 px-4 py-3 rounded-xl border border-[var(--accent-border)] bg-transparent focus:outline-none focus:border-emerald-500 font-mono text-sm"
              autoComplete="off"
            />
          </div>
        </div>

        <div className="flex gap-3 flex-wrap">
          <button
            onClick={handleVerify}
            disabled={verifying || !apiKey.trim() || !secretKey.trim()}
            className="px-6 py-3 rounded-xl font-medium bg-sky-600 hover:bg-sky-700 disabled:bg-gray-700 disabled:cursor-not-allowed text-white transition"
          >
            {verifying ? 'Running live tests…' : 'Test Credentials First'}
          </button>
          <button
            onClick={handleSaveKeys}
            disabled={!allPassed || !apiKey.trim() || !secretKey.trim()}
            className="px-6 py-3 rounded-xl font-medium bg-emerald-600 hover:bg-emerald-700 disabled:bg-gray-700 disabled:cursor-not-allowed text-white transition"
          >
            {hasKeys ? 'Update Saved Keys' : 'Save Verified Keys'}
          </button>
          {hasKeys && (
            <button onClick={handleDeleteKeys} className="px-6 py-3 rounded-xl font-medium border border-red-500/50 text-red-400 hover:bg-red-500/10 transition">
              Remove Saved Keys
            </button>
          )}
        </div>

        {testResults && (
          <div className="space-y-2 mt-2">
            {testResults.map((t, i) => (
              <div key={i} className={`flex items-start gap-3 px-4 py-3 rounded-lg border text-sm ${t.passed ? 'border-emerald-500/40 bg-emerald-500/5' : 'border-red-500/40 bg-red-500/5'}`}>
                <span className={`font-bold ${t.passed ? 'text-emerald-500' : 'text-red-500'}`}>{t.passed ? 'PASS' : 'FAIL'}</span>
                <div className="flex-1">
                  <div className="font-medium">{t.test_name}</div>
                  <div className="text-[var(--secondary-text)] text-xs mt-0.5">{t.detail}</div>
                </div>
              </div>
            ))}
          </div>
        )}
      </section>

      {/* ────────── Scanner ────────── */}
      <section className="surface rounded-2xl border border-[var(--accent-border)] p-6 space-y-5">
        <div className="flex items-center justify-between">
          <h3 className="text-xl font-semibold">Arbitrage Scanner</h3>
          <label className="flex items-center gap-3 text-sm cursor-pointer">
            <span className={settings.scan_paused ? 'text-amber-400 font-medium' : 'text-emerald-500 font-medium'}>
              {settings.scan_paused ? '⏸ PAUSED' : '▶ RUNNING'}
            </span>
            <input
              type="checkbox"
              checked={settings.scan_paused}
              onChange={(e) => update({ scan_paused: e.target.checked })}
              className="w-4 h-4 accent-emerald-500"
            />
          </label>
        </div>

        <div className="grid md:grid-cols-2 gap-4">
          <Field
            label="Minimum net profit (%)"
            sub="after fees + slippage buffer; e.g. 0.25"
            value={(settings.min_profit_threshold * 100).toFixed(2)}
            onChange={(v) => { const n = parseFloat(v); if (!isNaN(n)) update({ min_profit_threshold: n / 100 }); }}
          />
          <Field
            label="Taker fee per leg (%)"
            sub="MEXC spot default: 0.05 (maker is 0)"
            value={(settings.taker_fee * 100).toFixed(2)}
            onChange={(v) => { const n = parseFloat(v); if (!isNaN(n)) update({ taker_fee: n / 100 }); }}
          />
          <Field
            label="Slippage buffer (%)"
            sub="extra safety margin added to fees"
            value={(settings.slippage_buffer * 100).toFixed(2)}
            onChange={(v) => { const n = parseFloat(v); if (!isNaN(n)) update({ slippage_buffer: n / 100 }); }}
          />
          <Field
            label="Tick interval (ms)"
            sub="how often loops are recomputed; 20–100 typical"
            value={String(settings.tick_interval_ms)}
            onChange={(v) => { const n = parseInt(v); if (!isNaN(n)) update({ tick_interval_ms: n }); }}
          />
          <Field
            label="Required consecutive ticks"
            sub="ghost filter: 2–10"
            value={String(settings.required_ticks)}
            onChange={(v) => { const n = parseInt(v); if (!isNaN(n)) update({ required_ticks: n }); }}
          />
          <Field
            label="Target volume USD (discover size)"
            sub="simulated trade size; report shows true capacity separately"
            value={String(settings.target_volume_usd)}
            onChange={(v) => { const n = parseFloat(v); if (!isNaN(n)) update({ target_volume_usd: n }); }}
          />
        </div>
      </section>

      {/* ────────── Market whitelist ────────── */}
      <section className="surface rounded-2xl border border-[var(--accent-border)] p-6 space-y-5">
        <h3 className="text-xl font-semibold">Market Whitelist</h3>
        <p className="text-sm text-[var(--secondary-text)]">
          Rebuilt every hour from ALL USDT pairs, ranked by 24h volume.
        </p>
        <div className="grid md:grid-cols-2 gap-4">
          <Field
            label="Max whitelist size"
            sub="top N USDT pairs by volume"
            value={String(settings.max_whitelist)}
            onChange={(v) => { const n = parseInt(v); if (!isNaN(n)) update({ max_whitelist: n }); }}
          />
          <Field
            label="Min 24h volume (USD)"
            sub="pairs below this are excluded"
            value={String(Math.round(settings.min_24h_volume_usd))}
            onChange={(v) => { const n = parseFloat(v); if (!isNaN(n)) update({ min_24h_volume_usd: n }); }}
          />
        </div>
      </section>

      {/* ────────── Data retention ────────── */}
      <section className="surface rounded-2xl border border-[var(--accent-border)] p-6 space-y-5">
        <h3 className="text-xl font-semibold">Trade History</h3>
        <div className="grid md:grid-cols-2 gap-4">
          <Field
            label="Retention (days)"
            sub="0 = keep forever. Trade history survives restarts on HF persistent storage."
            value={String(settings.retention_days)}
            onChange={(v) => { const n = parseInt(v); if (!isNaN(n)) update({ retention_days: n }); }}
          />
        </div>
      </section>

      <button
        onClick={handleSaveSettings}
        disabled={saving}
        className="w-full md:w-auto px-8 py-3 rounded-xl font-semibold bg-emerald-600 hover:bg-emerald-700 disabled:bg-gray-700 text-white transition"
      >
        {saving ? 'Saving…' : 'Save All Settings'}
      </button>
    </div>
  );
}

function Field({ label, sub, value, onChange }: { label: string; sub: string; value: string; onChange: (v: string) => void }) {
  return (
    <div>
      <label className="text-xs uppercase tracking-wide text-[var(--secondary-text)]">{label}</label>
      <input
        type="text"
        inputMode="decimal"
        value={value}
        onChange={(e) => onChange(e.target.value)}
        className="w-full mt-1 px-4 py-3 rounded-xl border border-[var(--accent-border)] bg-transparent focus:outline-none focus:border-emerald-500 font-mono"
      />
      <p className="text-xs text-[var(--secondary-text)] mt-1">{sub}</p>
    </div>
  );
}
