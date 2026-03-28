// frontend/src/App.tsx
// Fixed: Real login calling backend /api/login + proper auth flow

import { useState, useEffect } from 'react';
import { BrowserRouter as Router, Routes, Route, Link, Navigate } from 'react-router-dom';
import { ThemeToggle } from './components/ThemeToggle';
import LivePulse from './components/LivePulse';
import VerifiedExecutions from './components/VerifiedExecutions';
import MarketWhitelist from './components/MarketWhitelist';
import SystemHealth from './components/SystemHealth';

function App() {
  const [isDark, setIsDark] = useState(true);
  const [isAuthenticated, setIsAuthenticated] = useState(false);
  const [loginError, setLoginError] = useState('');
  const [loginLoading, setLoginLoading] = useState(false);
  const [passwordInput, setPasswordInput] = useState('');

  // Check for existing auth token on load
  useEffect(() => {
    const token = localStorage.getItem('authToken');
    if (token) {
      setIsAuthenticated(true);
    }
  }, []);

  // Apply dark class
  useEffect(() => {
    if (isDark) {
      document.documentElement.classList.add('dark');
    } else {
      document.documentElement.classList.remove('dark');
    }
  }, [isDark]);

  const toggleTheme = () => setIsDark(!isDark);

  const handleLogin = async () => {
    if (!passwordInput.trim()) return;

    setLoginLoading(true);
    setLoginError('');

    try {
      const res = await fetch('/api/login', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ password: passwordInput }),
      });

      if (res.ok) {
        localStorage.setItem('authToken', 'authenticated');
        setIsAuthenticated(true);
        setPasswordInput('');
      } else {
        setLoginError('Invalid password. Try again.');
      }
    } catch (err) {
      setLoginError('Cannot reach server. Is the backend running?');
      console.error(err);
    } finally {
      setLoginLoading(false);
    }
  };

  const handleLogout = () => {
    localStorage.removeItem('authToken');
    setIsAuthenticated(false);
  };

  // Login screen
  if (!isAuthenticated) {
    return (
      <div className="min-h-screen bg-[var(--background)] flex items-center justify-center">
        <div className="surface p-10 rounded-3xl border border-[var(--accent-border)] max-w-md w-full text-center">
          <h1 className="text-3xl font-semibold mb-2">MEXC Ghost Hunter</h1>
          <p className="text-[var(--secondary-text)] mb-8">Enter admin password to continue</p>
          
          <input
            type="password"
            value={passwordInput}
            onChange={(e) => setPasswordInput(e.target.value)}
            className="surface w-full px-4 py-3 rounded-xl border border-[var(--accent-border)] focus:outline-none focus:border-emerald-500 mb-4"
            placeholder="Admin Password"
            onKeyDown={(e) => { if (e.key === 'Enter') handleLogin(); }}
          />
          
          {loginError && (
            <p className="text-red-500 text-sm mb-3">{loginError}</p>
          )}

          <button
            onClick={handleLogin}
            disabled={loginLoading}
            className="w-full py-3 bg-emerald-600 hover:bg-emerald-700 disabled:bg-gray-600 text-white rounded-xl font-medium transition"
          >
            {loginLoading ? 'Verifying...' : 'Login'}
          </button>

          <p className="text-xs text-[var(--secondary-text)] mt-6">
            Password is set via ADMIN_PASSWORD environment variable
          </p>
        </div>
      </div>
    );
  }

  return (
    <Router>
      <div className="min-h-screen bg-[var(--background)] text-[var(--primary-text)] transition-colors duration-150">
        {/* Navigation Bar */}
        <nav className="surface border-b border-[var(--accent-border)] sticky top-0 z-50">
          <div className="max-w-7xl mx-auto px-6 py-4 flex items-center justify-between">
            <div className="flex items-center gap-3">
              <div className="w-8 h-8 bg-emerald-500 rounded-full flex items-center justify-center text-white font-bold text-xl">
                👻
              </div>
              <div>
                <h1 className="text-2xl font-semibold tracking-tight">MEXC Ghost Hunter</h1>
                <p className="text-xs text-[var(--secondary-text)] -mt-1">Triangle Arbitrage Scanner</p>
              </div>
            </div>

            <div className="flex items-center gap-8">
              <div className="flex gap-8 text-sm font-medium">
                <NavLink to="/">Live Pulse</NavLink>
                <NavLink to="/verified">Verified Executions</NavLink>
                <NavLink to="/whitelist">Market Maintenance</NavLink>
                <NavLink to="/health">System Health</NavLink>
              </div>

              <div className="flex items-center gap-4">
                <ThemeToggle isDark={isDark} toggleTheme={toggleTheme} />
                <button
                  onClick={handleLogout}
                  className="text-xs px-4 py-2 border border-[var(--accent-border)] hover:bg-[var(--surface)] rounded-lg transition"
                >
                  Logout
                </button>
              </div>
            </div>
          </div>
        </nav>

        <main className="max-w-7xl mx-auto px-6 py-8">
          <Routes>
            <Route path="/" element={<LivePulse />} />
            <Route path="/verified" element={<VerifiedExecutions />} />
            <Route path="/whitelist" element={<MarketWhitelist />} />
            <Route path="/health" element={<SystemHealth />} />
            <Route path="*" element={<Navigate to="/" replace />} />
          </Routes>
        </main>

        <footer className="border-t border-[var(--accent-border)] py-6 text-center text-xs text-[var(--secondary-text)]">
          Headless-First • Zero Degradation • Built for 2-core VPS
        </footer>
      </div>
    </Router>
  );
}

function NavLink({ to, children }: { to: string; children: React.ReactNode }) {
  return (
    <Link
      to={to}
      className="hover:text-emerald-500 transition-colors relative after:absolute after:bottom-[-2px] after:left-0 after:h-[2px] after:bg-emerald-500 after:scale-x-0 hover:after:scale-x-100 after:transition-transform"
    >
      {children}
    </Link>
  );
}

export default App;
