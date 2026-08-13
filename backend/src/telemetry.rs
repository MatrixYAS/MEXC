// backend/src/telemetry.rs
// Fixed: No more block_on deadlock, now fully async with Mutex

use crate::data::models::Telemetry;
use crate::engine::MathEngine;
use crate::engine::scanner::Scanner;
use sysinfo::{CpuRefreshKind, MemoryRefreshKind, System};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Mutex;
use tokio::time::{interval, Duration};
use tracing;

pub struct TelemetryCollector {
    system: Mutex<System>,
    math_engine: Arc<MathEngine>,
    start_time: Instant,
    last_math_loop_duration: Mutex<f64>,
    last_stats: Mutex<(usize, usize)>,
    scanner: Arc<Mutex<Scanner>>,
}

impl TelemetryCollector {
    pub fn engine(&self) -> &Arc<MathEngine> {
        &self.math_engine
    }

    pub fn new(math_engine: Arc<MathEngine>, scanner: Arc<Mutex<Scanner>>) -> Self {
        let mut sys = System::new();
        sys.refresh_all();
        Self {
            system: Mutex::new(sys),
            math_engine,
            start_time: Instant::now(),
            last_math_loop_duration: Mutex::new(0.0),
            last_stats: Mutex::new((0, 0)),
            scanner,
        }
    }

    pub async fn set_math_loop_ms(&self, ms: f64) {
        *self.last_math_loop_duration.lock().await = ms;
    }

    pub async fn collect(&self) -> Telemetry {
        let (cpu_usage, ram_used) = {
            let mut sys = self.system.lock().await;
            sys.refresh_cpu_specifics(CpuRefreshKind::everything());
            sys.refresh_memory_specifics(MemoryRefreshKind::everything());
            (sys.global_cpu_usage(), sys.used_memory() / 1024 / 1024)
        };

        let active = {
            let s = self.scanner.lock().await;
            s.topology.len()
        };
        let loop_ms = *self.last_math_loop_duration.lock().await;

        Telemetry {
            cpu_usage,
            ram_usage_mb: ram_used as u64,
            ws_latency_ms: 0.0,
            math_loop_time_ms: loop_ms,
            active_triangles: active,
            timestamp: chrono::Utc::now(),
        }
    }

    pub fn uptime_ms(&self) -> i64 {
        self.start_time.elapsed().as_millis() as i64
    }

    pub async fn start_collector(self: Arc<Self>) {
        // stats are now computed inline on demand in collect(); keep this task
        // alive so the collector is not dropped (historical placeholder).
        let mut ticker = interval(Duration::from_secs(60));
        loop {
            ticker.tick().await;
        }
    }
}
