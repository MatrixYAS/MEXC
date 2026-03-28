// backend/src/telemetry.rs
// Fixed: No more block_on deadlock, now fully async with Mutex

use crate::data::models::Telemetry;
use crate::engine::MathEngine;
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
}

impl TelemetryCollector {
    pub fn new(math_engine: Arc<MathEngine>) -> Self {
        let mut sys = System::new();
        sys.refresh_all();
        Self {
            system: Mutex::new(sys),
            math_engine,
            start_time: Instant::now(),
            last_math_loop_duration: Mutex::new(0.0),
            last_stats: Mutex::new((0, 0)),
        }
    }

    pub async fn collect(&self) -> Telemetry {
        let (cpu_usage, ram_used) = {
            let mut sys = self.system.lock().await;
            sys.refresh_cpu_specifics(CpuRefreshKind::everything());
            sys.refresh_memory_specifics(MemoryRefreshKind::everything());
            (sys.global_cpu_usage(), sys.used_memory() / 1024 / 1024)
        };

        let (_, active) = *self.last_stats.lock().await;
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
        let mut ticker = interval(Duration::from_secs(10));
        loop {
            ticker.tick().await;
            
            let stats = self.math_engine.get_stats().await;
            *self.last_stats.lock().await = stats;

            let t = self.collect().await;
            tracing::debug!(
                "Telemetry - CPU: {:.1}% | RAM: {}MB | Active Triangles: {}",
                t.cpu_usage, t.ram_usage_mb, t.active_triangles
            );
        }
    }
}
