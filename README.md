# MEXC Ghost Hunter

**High-performance triangle arbitrage scanner for MEXC**  
Built with **Rust** (zero-degradation backend) + **React** (high-contrast minimalist UI)

---

## Features

- **Zero-Degradation Backend**: Runs 24/7 on a 2-core VPS with lock-free, zero-allocation hot path
- **Real-World Math Engine**: Validates gaps using $1,000 USD weighted average fill simulation across all legs
- **Triple-Tax Formula**: Accounts for 0.1% taker fee × 3
- **Anti-Ghost Persistence**: 3 consecutive ticks + staleness check (2000ms)
- **Adaptive 24h Whitelist**: Auto-refreshes 300 coins based on volume + closed loops
- **Headless Logging**: Stores verified opportunities in SQLite even when UI is closed
- **High-Contrast UI**: Sharp Light/Dark mode with anti-blur design
- **Real-time Live Pulse** via SSE
- **Docker-ready** for local + Hugging Face deployment

---

## Project Structure
