import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import path from 'path'

// https://vitejs.dev/config/
export default defineConfig({
  plugins: [react()],
  
  // Base path for deployment (important for Hugging Face / subpath)
  base: '/',

  resolve: {
    alias: {
      "@": path.resolve(__dirname, "./src"),
    },
  },

  // Server configuration for local development + proxy to Rust backend
  server: {
    port: 5173,           // Standard Vite dev port
    strictPort: true,
    host: true,           // Listen on all interfaces (useful in Docker)
    
    // Proxy API calls to the Rust backend during development
    proxy: {
      '/api': {
        target: 'http://localhost:7860',
        changeOrigin: true,
        secure: false,
      },
    },
  },

  build: {
    outDir: 'dist',
    sourcemap: true,
    rollupOptions: {
      output: {
        manualChunks: {
          vendor: ['react', 'react-dom'],
        },
      },
    },
  },
})
