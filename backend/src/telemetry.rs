// backend/src/telemetry.rs
// System Health & Hardware Monitoring
// Pulls CPU, RAM, Network Latency, and Math Loop timing every 10 seconds
// As specified in the PRD for "Zero Degradation" monitoring

use crate::data::models::Telemetry;
use crate::engine::MathEngine;
use sysinfo::{CpuRefreshKind, MemoryRefreshKind, System};
use std::sync::Arc;
use std::time::Instant;
use tokio::time::{interval, Duration};
use tracing;

pub struct TelemetryCollector {
    system: System,
    math_engine: Arc<MathEngine>,
    start_time: Instant,
    last_math_loop_duration: f64,
}

impl TelemetryCollector {
    pub fn new(math_engine: Arc<MathEngine>) -> Self {
        let mut sys = System::new();
        sys.refresh_all();

        Self {
            system: sys,
            math_engine,
            start_time: Instant::now(),
            last_math_loop_duration: 0.0,
        }
    }

    /// Collect current telemetry snapshot
    pub fn collect(&mut self) -> Telemetry {
        // Refresh system metrics
        self.system.refresh_cpu_specifics(CpuRefreshKind::everything());
        self.system.refresh_memory_specifics(MemoryRefreshKind::everything());

        let cpu_usage = self.system.global_cpu_usage();
        let ram_used = self.system.used_memory() / 1024 / 1024; // MB

        // Get engine stats
        let (total_persistent, active) = futures::executor::block_on(self.math_engine.get_stats());

        Telemetry {
            cpu_usage,
            ram_usage_mb: ram_used as u64,
            ws_latency_ms: 0.0, // Will be updated from WS pool if needed
            math_loop_time_ms: self.last_math_loop_duration,
            active_triangles: active,
            timestamp: chrono::Utc::now(),
        }
    }

    /// Record how long the math processing loop took (for heartbeat warning)
    pub fn record_math_loop_time(&mut self, duration_ms: f64) {
        self.last_math_loop_duration = duration_ms;
        
        if duration_ms > 10.0 {
            tracing::warn!("Performance Warning: Math engine loop took {:.2}ms (>10ms threshold)", duration_ms);
        }
    }

    /// Get uptime in milliseconds
    pub fn uptime_ms(&self) -> i64 {
        self.start_time.elapsed().as_millis() as i64
    }

    /// Background telemetry collector task (runs every 10 seconds)
    pub async fn start_collector(self: Arc<Self>) {
        let mut ticker = interval(Duration::from_secs(10));
        
        loop {
            ticker.tick().await;
            
            let _telemetry = self.collect();
            // In a real setup, this could be sent to a channel for the Axum API
            // For now we just keep the data fresh
            tracing::debug!(
                "Telemetry - CPU: {:.1}% | RAM: {}MB | Active Triangles: {}",
                _telemetry.cpu_usage,
                _telemetry.ram_usage_mb,
                _telemetry.active_triangles
            );
        }
    }
}
