/** @type {import('tailwindcss').Config} */
export default {
  content: [
    "./index.html",
    "./src/**/*.{js,ts,jsx,tsx}",
  ],
  darkMode: 'class', // Enables dark mode via .dark class on html element
  theme: {
    extend: {
      // High-contrast semantic color palette as specified in the PRD
      colors: {
        // Light Mode (Slate/Zinc)
        light: {
          background: '#FFFFFF',      // Pure White
          surface: '#F8FAFC',         // Light Gray
          primary: '#0F172A',         // Navy/Black
          secondary: '#64748B',       // Slate Gray
          accent: '#E2E8F0',          // Steel
          border: '#E2E8F0',
          success: '#16A34A',         // Emerald
        },
        
        // Dark Mode (Midnight/Onyx)
        dark: {
          background: '#0A0A0B',      // Deep Black
          surface: '#171717',         // Dark Zinc
          primary: '#F8FAFC',         // Off-White
          secondary: '#A1A1AA',       // Zinc Gray
          accent: '#262626',          // Charcoal
          border: '#262626',
          success: '#4ADE80',         // Neon Green
        },
      },
      
      // Sharp transitions for theme switching (0ms or max 150ms)
      transitionDuration: {
        'theme': '150ms',
      },
      
      fontFamily: {
        sans: ['Inter', 'system-ui', 'sans-serif'],
      },
    },
  },
  plugins: [],
}
