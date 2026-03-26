// frontend/src/components/ThemeToggle.tsx
// High-Contrast Theme Toggle Component
// Sharp transition as per "Anti-Blur" requirement

import { Sun, Moon } from 'lucide-react';

interface ThemeToggleProps {
  isDark: boolean;
  toggleTheme: () => void;
}

export function ThemeToggle({ isDark, toggleTheme }: ThemeToggleProps) {
  return (
    <button
      onClick={toggleTheme}
      className="surface p-2.5 rounded-xl border border-[var(--accent-border)] hover:border-emerald-500/50 transition-all duration-150 flex items-center justify-center w-10 h-10 focus:outline-none focus:ring-2 focus:ring-emerald-500/30"
      aria-label="Toggle theme"
      title={isDark ? "Switch to Light Mode" : "Switch to Dark Mode"}
    >
      {isDark ? (
        <Sun 
          size={20} 
          className="text-yellow-400 transition-transform hover:rotate-12" 
        />
      ) : (
        <Moon 
          size={20} 
          className="text-slate-700 dark:text-slate-300 transition-transform hover:-rotate-12" 
        />
      )}
    </button>
  );
}
