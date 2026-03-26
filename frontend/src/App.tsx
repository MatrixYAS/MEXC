// frontend/src/App.tsx
// Main Application Layout with Router and High-Contrast Theme System

import { useState, useEffect } from 'react';
import { BrowserRouter as Router, Routes, Route, Link } from 'react-router-dom';
import { ThemeToggle } from './components/ThemeToggle';
import LivePulse from './components/LivePulse';
import VerifiedExecutions from './components/VerifiedExecutions';
import MarketWhitelist from './components/MarketWhitelist';
import SystemHealth from './components/SystemHealth';

function App() {
  const [isDark, setIsDark] = useState(true); // Default to dark mode (more professional for trading)

  // Apply dark class to <html> for Tailwind dark: variant
  useEffect(() => {
    if (isDark) {
      document.documentElement.classList.add('dark');
    } else {
      document.documentElement.classList.remove('dark');
    }
  }, [isDark]);

  const toggleTheme = () => {
    setIsDark(!isDark);
  };

  return (
    <Router>
      <div className="min-h-screen bg-[var(--background)] text-[var(--primary-text)] transition-colors duration-150">
        {/* Top Navigation Bar */}
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

              <ThemeToggle isDark={isDark} toggleTheme={toggleTheme} />
            </div>
          </div>
        </nav>

        {/* Main Content */}
        <main className="max-w-7xl mx-auto px-6 py-8">
          <Routes>
            <Route path="/" element={<LivePulse />} />
            <Route path="/verified" element={<VerifiedExecutions />} />
            <Route path="/whitelist" element={<MarketWhitelist />} />
            <Route path="/health" element={<SystemHealth />} />
          </Routes>
        </main>

        {/* Footer */}
        <footer className="border-t border-[var(--accent-border)] py-6 text-center text-xs text-[var(--secondary-text)]">
          Headless-First • Built for 2-core VPS • Zero Degradation Philosophy
        </footer>
      </div>
    </Router>
  );
}

// Simple active nav link component
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
