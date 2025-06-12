use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::sync::RwLock;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WorkerStatus {
    Starting,
    Healthy,
    Error(String),
    Dead,
}

#[derive(Debug, Clone, Serialize)]
pub struct WorkerHealth {
    pub status: WorkerStatus,
    pub last_heartbeat: SystemTime,
    pub jobs_processed: u64,
    pub last_error: Option<String>,
    pub uptime: Duration,
    pub restart_count: u32,
}

impl Default for WorkerHealth {
    fn default() -> Self {
        Self {
            status: WorkerStatus::Starting,
            last_heartbeat: SystemTime::now(),
            jobs_processed: 0,
            last_error: None,
            uptime: Duration::from_secs(0),
            restart_count: 0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct WorkerHealthMonitor {
    health: Arc<RwLock<WorkerHealth>>,
    start_time: SystemTime,
}

impl Default for WorkerHealthMonitor {
    fn default() -> Self {
        Self::new()
    }
}

impl WorkerHealthMonitor {
    pub fn new() -> Self {
        Self {
            health: Arc::new(RwLock::new(WorkerHealth::default())),
            start_time: SystemTime::now(),
        }
    }

    pub async fn heartbeat(&self) {
        let mut health = self.health.write().await;
        health.last_heartbeat = SystemTime::now();
        health.uptime = self.start_time.elapsed().unwrap_or_default();
    }

    pub async fn set_status(&self, status: WorkerStatus) {
        let mut health = self.health.write().await;
        health.status = status;
        health.last_heartbeat = SystemTime::now();
        health.uptime = self.start_time.elapsed().unwrap_or_default();
    }

    pub async fn job_processed(&self) {
        let mut health = self.health.write().await;
        health.jobs_processed += 1;
        health.last_heartbeat = SystemTime::now();
        health.uptime = self.start_time.elapsed().unwrap_or_default();
    }

    pub async fn set_error(&self, error: String) {
        let mut health = self.health.write().await;
        health.status = WorkerStatus::Error(error.clone());
        health.last_error = Some(error);
        health.last_heartbeat = SystemTime::now();
        health.uptime = self.start_time.elapsed().unwrap_or_default();
    }

    pub async fn increment_restart(&self) {
        let mut health = self.health.write().await;
        health.restart_count += 1;
        health.status = WorkerStatus::Starting;
        health.last_heartbeat = SystemTime::now();
        health.uptime = self.start_time.elapsed().unwrap_or_default();
    }

    pub async fn get_health(&self) -> WorkerHealth {
        let health = self.health.read().await;
        health.clone()
    }

    pub async fn is_healthy(&self) -> bool {
        let health = self.health.read().await;
        let time_since_heartbeat = health.last_heartbeat.elapsed().unwrap_or_default();

        match &health.status {
            WorkerStatus::Healthy => time_since_heartbeat < Duration::from_secs(60),
            WorkerStatus::Starting => time_since_heartbeat < Duration::from_secs(120),
            _ => false,
        }
    }

    pub fn get_arc_clone(&self) -> Arc<RwLock<WorkerHealth>> {
        Arc::clone(&self.health)
    }
}
