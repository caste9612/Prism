//! Queue-based processing system for decoupled scanning and indexing
//!
//! Architecture:
//! - Drive Monitor: Detects drive changes, adds to scan queue
//! - Scan Worker: Processes scan queue, uses existing Scanner to index files
//! - State Broadcaster: Emits state updates to frontend

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{RwLock, mpsc, Notify};
use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tracing::{info, warn, error};

use crate::AppState;

// ============================================================================
// TYPES AND STRUCTURES
// ============================================================================

/// Status of a drive in the system
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum DriveProcessStatus {
    /// Drive is online but not queued for scanning
    Online,
    /// Drive is offline
    Offline,
    /// Drive is queued for scanning
    Queued,
    /// Drive is currently being scanned
    Scanning,
    /// Fully indexed and up to date
    Indexed,
    /// Error occurred during processing
    Error,
}

/// Information about a drive's current state
#[derive(Debug, Clone, Serialize)]
pub struct DriveState {
    pub path: String,
    pub name: String,
    pub drive_type: String,
    pub status: DriveProcessStatus,
    pub scan_progress: f64,        // 0.0 - 100.0
    pub files_found: i64,
    pub files_indexed: i64,
    pub total_size: u64,
    pub error_message: Option<String>,
    pub is_known: bool,            // Has been scanned before
}

/// Overall system state - the "source of truth"
#[derive(Debug, Clone, Serialize)]
pub struct SystemState {
    /// State of each drive
    pub drives: HashMap<String, DriveState>,
    /// Drives waiting to be scanned
    pub scan_queue: Vec<String>,
    /// Is the scanner currently working?
    pub scanner_busy: bool,
    /// Currently scanning drive (if any)
    pub current_scan: Option<String>,
    /// Total files scanned in current session
    pub session_files_scanned: i64,
    /// Total files indexed in current session
    pub session_files_indexed: i64,
}

impl Default for SystemState {
    fn default() -> Self {
        Self {
            drives: HashMap::new(),
            scan_queue: Vec::new(),
            scanner_busy: false,
            current_scan: None,
            session_files_scanned: 0,
            session_files_indexed: 0,
        }
    }
}

/// Request to scan a drive
#[derive(Debug, Clone)]
pub struct ScanRequest {
    pub drive_path: String,
    pub drive_name: String,
    pub drive_type: String,
    pub is_incremental: bool,
}

// ============================================================================
// QUEUE MANAGER
// ============================================================================

/// Central manager for all queues and workers
pub struct QueueManager {
    /// Shared system state
    state: Arc<RwLock<SystemState>>,
    /// Channel to send scan requests
    scan_tx: mpsc::Sender<ScanRequest>,
    /// Notify when system state changes
    state_changed: Arc<Notify>,
    /// App handle for emitting events
    app_handle: AppHandle,
    /// App state for database access
    app_state: Arc<AppState>,
    /// Is the system running?
    running: Arc<RwLock<bool>>,
}

impl QueueManager {
    /// Create a new queue manager and start all workers
    pub async fn new(app_handle: AppHandle, app_state: Arc<AppState>) -> Arc<Self> {
        let (scan_tx, scan_rx) = mpsc::channel::<ScanRequest>(100);

        let manager = Arc::new(Self {
            state: Arc::new(RwLock::new(SystemState::default())),
            scan_tx,
            state_changed: Arc::new(Notify::new()),
            app_handle,
            app_state,
            running: Arc::new(RwLock::new(true)),
        });

        // Start workers
        manager.start_scan_worker(scan_rx).await;
        manager.start_state_broadcaster().await;

        info!("QueueManager initialized with all workers");
        manager
    }

    /// Get current system state
    pub async fn get_state(&self) -> SystemState {
        self.state.read().await.clone()
    }

    /// Queue a drive for scanning
    pub async fn queue_drive(&self, request: ScanRequest) -> Result<(), String> {
        let path = request.drive_path.clone();

        // Update state
        {
            let mut state = self.state.write().await;

            // Check if already queued or scanning
            if let Some(drive) = state.drives.get(&path) {
                if drive.status == DriveProcessStatus::Queued ||
                   drive.status == DriveProcessStatus::Scanning {
                    return Err(format!("Drive {} is already queued or scanning", path));
                }
            }

            // Add/update drive state
            state.drives.insert(path.clone(), DriveState {
                path: path.clone(),
                name: request.drive_name.clone(),
                drive_type: request.drive_type.clone(),
                status: DriveProcessStatus::Queued,
                scan_progress: 0.0,
                files_found: 0,
                files_indexed: 0,
                total_size: 0,
                error_message: None,
                is_known: request.is_incremental,
            });

            state.scan_queue.push(path.clone());
        }

        // Send to scan worker
        self.scan_tx.send(request).await
            .map_err(|e| format!("Failed to queue drive: {}", e))?;

        self.state_changed.notify_waiters();
        info!("Drive {} queued for scanning", path);
        Ok(())
    }

    /// Update drive status
    pub async fn update_drive_status(&self, path: &str, status: DriveProcessStatus) {
        let mut state = self.state.write().await;
        if let Some(drive) = state.drives.get_mut(path) {
            drive.status = status;
        }
        self.state_changed.notify_waiters();
    }

    /// Update drive scan progress
    pub async fn update_scan_progress(&self, path: &str, progress: f64, files_found: i64) {
        let mut state = self.state.write().await;
        if let Some(drive) = state.drives.get_mut(path) {
            drive.scan_progress = progress;
            drive.files_found = files_found;
        }
        self.state_changed.notify_waiters();
    }

    /// Start the scan worker
    async fn start_scan_worker(&self, mut rx: mpsc::Receiver<ScanRequest>) {
        let state = self.state.clone();
        let state_changed = self.state_changed.clone();
        let app_state = self.app_state.clone();
        let running = self.running.clone();
        let app_handle = self.app_handle.clone();

        tokio::spawn(async move {
            info!("Scan worker started");

            while *running.read().await {
                // Wait for next scan request
                match rx.recv().await {
                    Some(request) => {
                        let path = request.drive_path.clone();
                        info!("Scan worker processing: {}", path);

                        // Update state to scanning
                        {
                            let mut s = state.write().await;
                            s.scanner_busy = true;
                            s.current_scan = Some(path.clone());
                            if let Some(drive) = s.drives.get_mut(&path) {
                                drive.status = DriveProcessStatus::Scanning;
                            }
                            // Remove from queue
                            s.scan_queue.retain(|p| p != &path);
                        }
                        state_changed.notify_waiters();

                        // Perform the scan using existing scanner infrastructure
                        let result = Self::perform_scan(
                            &request,
                            &app_state,
                            &state,
                            &state_changed,
                            &app_handle,
                        ).await;

                        // Update state based on result
                        {
                            let mut s = state.write().await;
                            s.scanner_busy = false;
                            s.current_scan = None;
                            if let Some(drive) = s.drives.get_mut(&path) {
                                match result {
                                    Ok(files_count) => {
                                        drive.status = DriveProcessStatus::Indexed;
                                        drive.files_indexed = files_count;
                                        drive.scan_progress = 100.0;
                                        s.session_files_scanned += files_count;
                                        s.session_files_indexed += files_count;
                                    }
                                    Err(ref e) => {
                                        drive.status = DriveProcessStatus::Error;
                                        drive.error_message = Some(e.clone());
                                    }
                                }
                            }
                        }
                        state_changed.notify_waiters();

                        if let Err(e) = result {
                            error!("Scan failed for {}: {}", path, e);
                        } else {
                            info!("Scan completed for {}", path);
                        }
                    }
                    None => {
                        info!("Scan worker channel closed");
                        break;
                    }
                }
            }

            info!("Scan worker stopped");
        });
    }

    /// Perform the actual scan using the existing scanner infrastructure
    async fn perform_scan(
        request: &ScanRequest,
        app_state: &Arc<AppState>,
        _state: &Arc<RwLock<SystemState>>,
        _state_changed: &Arc<Notify>,
        app_handle: &AppHandle,
    ) -> Result<i64, String> {
        use crate::scanner::{ScanConfig, Scanner};

        let path = request.drive_path.clone();

        // Create scan configuration with defaults
        let num_threads = std::thread::available_parallelism()
            .map(|p| p.get())
            .unwrap_or(4);

        let config = ScanConfig {
            root_paths: vec![path.clone()],
            exclude_hidden: true,
            exclude_system: true,
            exclude_patterns: vec![],
            min_file_size: 0,
            max_file_size: None,
            hash_threshold: 1024 * 1024, // 1MB
            batch_size: 10000,
            num_threads,
        };

        // Create a new scan ID in the database
        let scan_id = {
            let db = app_state.db.lock().await;
            db.create_scan(&[path.clone()]).map_err(|e| e.to_string())?
        };

        info!("Starting queue scan for {} with scan_id {}", path, scan_id);

        // Create scanner
        let scanner = Scanner::new(config);
        let app_handle_clone = app_handle.clone();
        let is_incremental = request.is_incremental;

        // Run the scan - use the existing scanner which writes directly to the database
        let db_clone = app_state.db.clone();
        let scan_result: Result<i64, String> = tokio::task::spawn_blocking(move || {
            // Get a sync lock on the database for the scanner
            let rt = tokio::runtime::Handle::current();
            let db = rt.block_on(async { db_clone.lock().await });

            if is_incremental {
                scanner.scan_incremental_with_events(&db, scan_id, Some(&app_handle_clone))
                    .map(|progress| (progress.files_new + progress.files_updated) as i64)
                    .map_err(|e| e.to_string())
            } else {
                scanner.scan_with_events(&db, scan_id, Some(&app_handle_clone))
                    .map(|progress| progress.files_scanned as i64)
                    .map_err(|e| e.to_string())
            }
        }).await.map_err(|e| format!("Scan task failed: {}", e))?;

        let total_files = scan_result?;

        // Mark scan as complete in database
        {
            let db = app_state.db.lock().await;
            if let Err(e) = db.update_scan_status(scan_id, "completed", Some(total_files), None) {
                warn!("Failed to mark scan as complete: {}", e);
            }
        }

        // Emit final progress
        let _ = app_handle.emit("queue-scan-complete", serde_json::json!({
            "drive_path": path,
            "files_indexed": total_files,
            "scan_id": scan_id,
        }));

        Ok(total_files)
    }

    /// Start the state broadcaster (emits state updates to frontend)
    async fn start_state_broadcaster(&self) {
        let state = self.state.clone();
        let state_changed = self.state_changed.clone();
        let app_handle = self.app_handle.clone();
        let running = self.running.clone();

        tokio::spawn(async move {
            info!("State broadcaster started");

            loop {
                // Wait for state change or timeout (for periodic updates)
                tokio::select! {
                    _ = state_changed.notified() => {}
                    _ = tokio::time::sleep(tokio::time::Duration::from_secs(1)) => {}
                }

                if !*running.read().await {
                    break;
                }

                // Emit current state
                let current_state = state.read().await.clone();
                let _ = app_handle.emit("queue-system-state", &current_state);
            }

            info!("State broadcaster stopped");
        });
    }

    /// Stop all workers
    pub async fn stop(&self) {
        info!("Stopping QueueManager");
        *self.running.write().await = false;
        self.state_changed.notify_waiters();
    }
}

// ============================================================================
// GLOBAL QUEUE MANAGER
// ============================================================================

lazy_static::lazy_static! {
    static ref QUEUE_MANAGER: RwLock<Option<Arc<QueueManager>>> = RwLock::new(None);
}

/// Initialize the global queue manager
pub async fn init_queue_manager(app_handle: AppHandle, app_state: Arc<AppState>) {
    let manager = QueueManager::new(app_handle, app_state).await;
    *QUEUE_MANAGER.write().await = Some(manager);
    info!("Global QueueManager initialized");
}

/// Get the global queue manager
pub async fn get_queue_manager() -> Option<Arc<QueueManager>> {
    QUEUE_MANAGER.read().await.clone()
}

/// Queue a drive for scanning
pub async fn queue_drive_for_scan(
    drive_path: String,
    drive_name: String,
    drive_type: String,
    is_incremental: bool,
) -> Result<(), String> {
    let manager = get_queue_manager().await
        .ok_or("QueueManager not initialized")?;

    manager.queue_drive(ScanRequest {
        drive_path,
        drive_name,
        drive_type,
        is_incremental,
    }).await
}

/// Get current system state
pub async fn get_system_state() -> Result<SystemState, String> {
    let manager = get_queue_manager().await
        .ok_or("QueueManager not initialized")?;

    Ok(manager.get_state().await)
}

// ============================================================================
// TAURI COMMANDS
// ============================================================================

/// Initialize the queue system (call from app setup)
#[tauri::command]
pub async fn init_queue_system(
    app_handle: AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<String, String> {
    let app_state = Arc::new(AppState {
        db: state.db.clone(),
        db_path: state.db_path.clone(),
        log_dir: state.log_dir.clone(),
        is_scanning: state.is_scanning.clone(),
    });

    init_queue_manager(app_handle, app_state).await;
    Ok("Queue system initialized".to_string())
}

/// Queue a drive for scanning via command
#[tauri::command]
pub async fn queue_drive_scan(
    drive_path: String,
    drive_name: String,
    drive_type: String,
    is_incremental: bool,
) -> Result<String, String> {
    queue_drive_for_scan(drive_path.clone(), drive_name, drive_type, is_incremental).await?;
    Ok(format!("Drive {} queued for scanning", drive_path))
}

/// Get current queue system state
#[tauri::command]
pub async fn get_queue_state() -> Result<SystemState, String> {
    get_system_state().await
}
