//! Drive detection and management commands

use crate::AppState;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use tauri::State;
use tracing::{info, debug, warn};

/// Drive information (basic, from system detection)
#[derive(Debug, Serialize, Clone)]
pub struct DriveInfo {
    pub path: String,
    pub name: String,
    pub drive_type: String,
    pub total_space: u64,
    pub free_space: u64,
    pub is_ready: bool,
}

/// Online/offline status for a drive
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum DriveOnlineStatus {
    /// Available and has been scanned
    Online,
    /// Was scanned but currently not available
    Offline,
    /// Was offline but now reconnected (needs sync)
    ReadyToSync,
    /// Available but never scanned
    NeverScanned,
}

/// Extended drive information with online/offline status
#[derive(Debug, Serialize, Clone)]
pub struct DriveStatus {
    pub path: String,
    pub name: String,
    pub drive_type: String,
    pub total_space: u64,
    pub free_space: u64,
    pub is_ready: bool,
    /// Currently accessible
    pub is_online: bool,
    /// Has data in database
    pub is_scanned: bool,
    /// When last scanned (unix timestamp)
    pub last_scan_at: Option<i64>,
    /// Files in database
    pub indexed_files: i64,
    /// Size in database
    pub indexed_size: i64,
    /// Combined status
    pub status: DriveOnlineStatus,
}

/// Get all available drives (local and network)
#[tauri::command]
pub async fn get_available_drives() -> Result<Vec<DriveInfo>, String> {
    info!("Detecting available drives...");
    let mut drives = Vec::new();
    let mut detected_count = 0;

    #[cfg(target_os = "windows")]
    {
        use std::ffi::OsString;
        use std::os::windows::ffi::OsStringExt;

        #[link(name = "kernel32")]
        extern "system" {
            fn GetLogicalDriveStringsW(buffer_length: u32, buffer: *mut u16) -> u32;
            fn GetDriveTypeW(root_path: *const u16) -> u32;
            fn GetDiskFreeSpaceExW(
                directory: *const u16,
                free_bytes_available: *mut u64,
                total_bytes: *mut u64,
                total_free_bytes: *mut u64,
            ) -> i32;
            fn GetVolumeInformationW(
                root_path: *const u16,
                volume_name: *mut u16,
                volume_name_size: u32,
                serial_number: *mut u32,
                max_component_length: *mut u32,
                file_system_flags: *mut u32,
                file_system_name: *mut u16,
                file_system_name_size: u32,
            ) -> i32;
        }

        const DRIVE_UNKNOWN: u32 = 0;
        const DRIVE_NO_ROOT_DIR: u32 = 1;
        const DRIVE_REMOVABLE: u32 = 2;
        const DRIVE_FIXED: u32 = 3;
        const DRIVE_REMOTE: u32 = 4;
        const DRIVE_CDROM: u32 = 5;
        const DRIVE_RAMDISK: u32 = 6;

        let mut buffer: [u16; 512] = [0; 512];
        let len = unsafe { GetLogicalDriveStringsW(buffer.len() as u32, buffer.as_mut_ptr()) };

        if len == 0 {
            return Err("Failed to get drive strings".to_string());
        }

        let mut start = 0;
        for i in 0..len as usize {
            if buffer[i] == 0 {
                if start < i {
                    let drive_path = OsString::from_wide(&buffer[start..i]);
                    let drive_str = drive_path.to_string_lossy().to_string();

                    let mut path_wide: Vec<u16> = drive_str.encode_utf16().collect();
                    path_wide.push(0);

                    let drive_type_raw = unsafe { GetDriveTypeW(path_wide.as_ptr()) };
                    let drive_type = match drive_type_raw {
                        DRIVE_REMOVABLE => "Removable",
                        DRIVE_FIXED => "Local Disk",
                        DRIVE_REMOTE => "Network Drive",
                        DRIVE_CDROM => "CD-ROM",
                        DRIVE_RAMDISK => "RAM Disk",
                        _ => "Unknown",
                    };

                    if drive_type_raw == DRIVE_UNKNOWN || drive_type_raw == DRIVE_NO_ROOT_DIR {
                        start = i + 1;
                        continue;
                    }

                    let mut volume_name: [u16; 256] = [0; 256];
                    let mut serial: u32 = 0;
                    let mut max_len: u32 = 0;
                    let mut flags: u32 = 0;
                    let mut fs_name: [u16; 256] = [0; 256];

                    let has_volume_info = unsafe {
                        GetVolumeInformationW(
                            path_wide.as_ptr(),
                            volume_name.as_mut_ptr(),
                            volume_name.len() as u32,
                            &mut serial,
                            &mut max_len,
                            &mut flags,
                            fs_name.as_mut_ptr(),
                            fs_name.len() as u32,
                        )
                    } != 0;

                    let name = if has_volume_info {
                        let end = volume_name
                            .iter()
                            .position(|&c| c == 0)
                            .unwrap_or(volume_name.len());
                        let vol_name =
                            OsString::from_wide(&volume_name[..end]).to_string_lossy().to_string();
                        if vol_name.is_empty() {
                            format!("{} ({})", drive_type, drive_str.trim_end_matches('\\'))
                        } else {
                            format!("{} ({})", vol_name, drive_str.trim_end_matches('\\'))
                        }
                    } else {
                        format!("{} ({})", drive_type, drive_str.trim_end_matches('\\'))
                    };

                    let mut free_bytes: u64 = 0;
                    let mut total_bytes: u64 = 0;
                    let mut total_free: u64 = 0;

                    let disk_space_ok = unsafe {
                        GetDiskFreeSpaceExW(
                            path_wide.as_ptr(),
                            &mut free_bytes,
                            &mut total_bytes,
                            &mut total_free,
                        )
                    } != 0;

                    // For network drives, GetDiskFreeSpaceExW may fail even if accessible
                    // Use the comprehensive check that tries multiple methods with retries
                    let is_ready = if disk_space_ok {
                        true
                    } else if drive_type_raw == DRIVE_REMOTE {
                        // Network drive: use comprehensive accessibility check
                        eprintln!("[DRIVES] Network drive {} needs accessibility check...", drive_str);
                        let accessible = check_network_drive_accessible(&drive_str);
                        eprintln!("[DRIVES] Network drive {} accessible={}", drive_str, accessible);
                        accessible
                    } else {
                        false
                    };

                    detected_count += 1;
                    info!(
                        "Detected drive {}: {} - type={}, ready={} (disk_space_ok={}), space={}/{}",
                        detected_count, drive_str, drive_type, is_ready, disk_space_ok,
                        free_bytes, total_bytes
                    );

                    drives.push(DriveInfo {
                        path: drive_str,
                        name,
                        drive_type: drive_type.to_string(),
                        total_space: if is_ready { total_bytes } else { 0 },
                        free_space: if is_ready { free_bytes } else { 0 },
                        is_ready,
                    });
                }
                start = i + 1;
            }
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        use std::fs;

        if let Ok(mounts) = fs::read_to_string("/proc/mounts") {
            for line in mounts.lines() {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 2 {
                    let mount_point = parts[1];
                    if mount_point.starts_with("/sys")
                        || mount_point.starts_with("/proc")
                        || mount_point.starts_with("/dev")
                        || mount_point.starts_with("/run")
                        || mount_point == "/"
                    {
                        continue;
                    }

                    drives.push(DriveInfo {
                        path: mount_point.to_string(),
                        name: mount_point.to_string(),
                        drive_type: "Mount".to_string(),
                        total_space: 0,
                        free_space: 0,
                        is_ready: true,
                    });
                }
            }
        }

        drives.insert(
            0,
            DriveInfo {
                path: "/".to_string(),
                name: "Root".to_string(),
                drive_type: "Local".to_string(),
                total_space: 0,
                free_space: 0,
                is_ready: true,
            },
        );
    }

    // Also try to enumerate network connections (for drives without letters)
    #[cfg(target_os = "windows")]
    {
        if let Some(network_drives) = enumerate_network_connections() {
            for net_drive in network_drives {
                // Check if we already have this drive (by path)
                if !drives.iter().any(|d| d.path.to_uppercase() == net_drive.path.to_uppercase()) {
                    debug!("Adding network connection: {}", net_drive.path);
                    drives.push(net_drive);
                }
            }
        }
    }

    let local_count = drives.iter().filter(|d| d.drive_type == "Local Disk").count();
    let network_count = drives.iter().filter(|d| d.drive_type == "Network Drive").count();
    let removable_count = drives.iter().filter(|d| d.drive_type == "Removable").count();
    let other_count = drives.len() - local_count - network_count - removable_count;

    info!(
        "Found {} drives: {} local, {} network, {} removable, {} other",
        drives.len(), local_count, network_count, removable_count, other_count
    );
    Ok(drives)
}

/// Try to reconnect a remembered network drive
#[cfg(target_os = "windows")]
fn try_reconnect_network_drive(local_path: &str, remote_path: &str) -> bool {
    use std::ptr;

    #[repr(C)]
    #[allow(non_snake_case)]
    struct NETRESOURCEW {
        dwScope: u32,
        dwType: u32,
        dwDisplayType: u32,
        dwUsage: u32,
        lpLocalName: *mut u16,
        lpRemoteName: *mut u16,
        lpComment: *mut u16,
        lpProvider: *mut u16,
    }

    #[link(name = "mpr")]
    extern "system" {
        fn WNetAddConnection2W(
            lpNetResource: *const NETRESOURCEW,
            lpPassword: *const u16,
            lpUserName: *const u16,
            dwFlags: u32,
        ) -> u32;
    }

    const RESOURCETYPE_DISK: u32 = 0x00000001;
    const CONNECT_UPDATE_PROFILE: u32 = 0x00000001;
    const NO_ERROR: u32 = 0;
    const ERROR_ALREADY_ASSIGNED: u32 = 85;

    // Prepare local name (e.g., "X:")
    let local_name = local_path.trim_end_matches('\\');
    let mut local_wide: Vec<u16> = local_name.encode_utf16().collect();
    local_wide.push(0);

    // Prepare remote name (e.g., "\\server\share")
    let mut remote_wide: Vec<u16> = remote_path.encode_utf16().collect();
    remote_wide.push(0);

    let net_resource = NETRESOURCEW {
        dwScope: 0,
        dwType: RESOURCETYPE_DISK,
        dwDisplayType: 0,
        dwUsage: 0,
        lpLocalName: local_wide.as_mut_ptr(),
        lpRemoteName: remote_wide.as_mut_ptr(),
        lpComment: ptr::null_mut(),
        lpProvider: ptr::null_mut(),
    };

    let result = unsafe {
        WNetAddConnection2W(
            &net_resource,
            ptr::null(),  // Use stored credentials
            ptr::null(),  // Use current user
            CONNECT_UPDATE_PROFILE,
        )
    };

    if result == NO_ERROR || result == ERROR_ALREADY_ASSIGNED {
        eprintln!("[NET] Successfully reconnected {} -> {} (result={})", local_path, remote_path, result);
        true
    } else {
        eprintln!("[NET] WNetAddConnection2W failed for {} -> {} with error: {}", local_path, remote_path, result);
        false
    }
}

/// Check if a network drive is accessible (may trigger reconnection)
#[cfg(target_os = "windows")]
fn check_network_drive_accessible(path: &str) -> bool {
    check_network_drive_accessible_with_remote(path, None)
}

/// Check if a network drive is accessible, optionally trying to reconnect using remote path
#[cfg(target_os = "windows")]
fn check_network_drive_accessible_with_remote(path: &str, remote_path: Option<&str>) -> bool {
    use std::fs;

    eprintln!("[NET] Checking network drive: {}", path);

    #[link(name = "kernel32")]
    extern "system" {
        fn GetDiskFreeSpaceExW(
            directory: *const u16,
            free_bytes_available: *mut u64,
            total_bytes: *mut u64,
            total_free_bytes: *mut u64,
        ) -> i32;
        fn GetFileAttributesW(file_name: *const u16) -> u32;
        fn GetLastError() -> u32;
    }

    const INVALID_FILE_ATTRIBUTES: u32 = 0xFFFFFFFF;

    // Method 1: Try GetDiskFreeSpaceExW (often triggers reconnection)
    let mut path_wide: Vec<u16> = path.encode_utf16().collect();
    path_wide.push(0);

    let mut free_bytes: u64 = 0;
    let mut total_bytes: u64 = 0;
    let mut total_free: u64 = 0;

    let disk_space_ok = unsafe {
        GetDiskFreeSpaceExW(
            path_wide.as_ptr(),
            &mut free_bytes,
            &mut total_bytes,
            &mut total_free,
        )
    } != 0;

    if disk_space_ok {
        eprintln!("[NET] {} accessible via GetDiskFreeSpaceExW (space: {}/{})", path, free_bytes, total_bytes);
        debug!("Network drive {} accessible via GetDiskFreeSpaceExW", path);
        return true;
    }

    let last_error = unsafe { GetLastError() };
    eprintln!("[NET] {} GetDiskFreeSpaceExW failed with error code: {}", path, last_error);

    // Method 2: Try GetFileAttributesW (lighter check)
    let attrs = unsafe { GetFileAttributesW(path_wide.as_ptr()) };
    if attrs != INVALID_FILE_ATTRIBUTES {
        eprintln!("[NET] {} accessible via GetFileAttributesW (attrs: 0x{:x})", path, attrs);
        debug!("Network drive {} accessible via GetFileAttributesW", path);
        return true;
    }
    let last_error2 = unsafe { GetLastError() };
    eprintln!("[NET] {} GetFileAttributesW failed with error code: {}", path, last_error2);

    // Method 3: Try to explicitly reconnect the drive if we have the remote path
    if let Some(remote) = remote_path {
        if try_reconnect_network_drive(path, remote) {
            // Retry after reconnection
            let disk_space_ok = unsafe {
                GetDiskFreeSpaceExW(
                    path_wide.as_ptr(),
                    &mut free_bytes,
                    &mut total_bytes,
                    &mut total_free,
                )
            } != 0;

            if disk_space_ok {
                eprintln!("[NET] {} accessible after reconnection (space: {}/{})", path, free_bytes, total_bytes);
                debug!("Network drive {} accessible after WNetAddConnection2W", path);
                return true;
            }
        }
    }

    // Method 4: Try std::fs::metadata
    match fs::metadata(path) {
        Ok(meta) => {
            eprintln!("[NET] {} accessible via fs::metadata (is_dir: {})", path, meta.is_dir());
            debug!("Network drive {} accessible via fs::metadata", path);
            return true;
        }
        Err(e) => {
            eprintln!("[NET] {} fs::metadata failed: {} (kind: {:?})", path, e, e.kind());
        }
    }

    // Method 5: Try read_dir
    let drive_path = std::path::Path::new(path);
    match drive_path.read_dir() {
        Ok(_entries) => {
            eprintln!("[NET] {} accessible via read_dir", path);
            debug!("Network drive {} accessible via read_dir", path);
            return true;
        }
        Err(e) => {
            eprintln!("[NET] {} read_dir failed: {} (kind: {:?})", path, e, e.kind());
        }
    }

    eprintln!("[NET] {} NOT accessible after all attempts", path);
    debug!("Network drive {} not accessible after all attempts", path);
    false
}

/// Enumerate network connections (mapped drives and remembered connections)
#[cfg(target_os = "windows")]
fn enumerate_network_connections() -> Option<Vec<DriveInfo>> {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use std::ptr;

    #[repr(C)]
    #[allow(non_snake_case)]
    struct NETRESOURCEW {
        dwScope: u32,
        dwType: u32,
        dwDisplayType: u32,
        dwUsage: u32,
        lpLocalName: *mut u16,
        lpRemoteName: *mut u16,
        lpComment: *mut u16,
        lpProvider: *mut u16,
    }

    #[link(name = "mpr")]
    extern "system" {
        fn WNetOpenEnumW(
            dwScope: u32,
            dwType: u32,
            dwUsage: u32,
            lpNetResource: *const NETRESOURCEW,
            lphEnum: *mut *mut std::ffi::c_void,
        ) -> u32;
        fn WNetEnumResourceW(
            hEnum: *mut std::ffi::c_void,
            lpcCount: *mut u32,
            lpBuffer: *mut u8,
            lpBufferSize: *mut u32,
        ) -> u32;
        fn WNetCloseEnum(hEnum: *mut std::ffi::c_void) -> u32;
    }

    const RESOURCE_CONNECTED: u32 = 0x00000001;
    const RESOURCE_REMEMBERED: u32 = 0x00000003;
    const RESOURCETYPE_DISK: u32 = 0x00000001;
    const NO_ERROR: u32 = 0;
    const ERROR_NO_MORE_ITEMS: u32 = 259;

    let mut drives = Vec::new();

    // Try both connected and remembered resources
    for scope in [RESOURCE_CONNECTED, RESOURCE_REMEMBERED] {
        let mut h_enum: *mut std::ffi::c_void = ptr::null_mut();

        let result = unsafe {
            WNetOpenEnumW(scope, RESOURCETYPE_DISK, 0, ptr::null(), &mut h_enum)
        };

        if result != NO_ERROR {
            debug!("WNetOpenEnumW failed with error {} for scope {}", result, scope);
            continue;
        }

        let mut buffer: Vec<u8> = vec![0u8; 16384];
        loop {
            let mut count: u32 = 0xFFFFFFFF; // Request all entries
            let mut buffer_size: u32 = buffer.len() as u32;

            let result = unsafe {
                WNetEnumResourceW(h_enum, &mut count, buffer.as_mut_ptr(), &mut buffer_size)
            };

            if result == ERROR_NO_MORE_ITEMS {
                break;
            }

            if result != NO_ERROR {
                warn!("WNetEnumResourceW failed with error {}", result);
                break;
            }

            // Parse the resources
            let resources = unsafe {
                std::slice::from_raw_parts(
                    buffer.as_ptr() as *const NETRESOURCEW,
                    count as usize
                )
            };

            for res in resources {
                if res.lpRemoteName.is_null() {
                    continue;
                }

                // Get remote name (UNC path)
                let remote_name = unsafe {
                    let len = (0..).find(|&i| *res.lpRemoteName.offset(i) == 0).unwrap_or(0);
                    OsString::from_wide(std::slice::from_raw_parts(res.lpRemoteName, len as usize))
                        .to_string_lossy()
                        .to_string()
                };

                // Get local name (drive letter) if mapped
                let local_name = if !res.lpLocalName.is_null() {
                    unsafe {
                        let len = (0..).find(|&i| *res.lpLocalName.offset(i) == 0).unwrap_or(0);
                        Some(OsString::from_wide(std::slice::from_raw_parts(res.lpLocalName, len as usize))
                            .to_string_lossy()
                            .to_string())
                    }
                } else {
                    None
                };

                // Add network connections:
                // - Without drive letter: always add (UNC path access)
                // - With drive letter: add if REMEMBERED but disconnected
                //   (GetLogicalDriveStringsW only returns CONNECTED drives)
                let should_add = if local_name.is_some() {
                    // Has drive letter - check if it's in the REMEMBERED scope (disconnected)
                    scope == RESOURCE_REMEMBERED
                } else {
                    // No drive letter - UNC path only
                    remote_name.starts_with("\\\\")
                };

                if should_add {
                    let path = if let Some(ref local) = local_name {
                        // Use drive letter path if available
                        if local.ends_with('\\') || local.ends_with(':') {
                            if local.ends_with(':') {
                                format!("{}\\", local)
                            } else {
                                local.clone()
                            }
                        } else {
                            format!("{}\\", local)
                        }
                    } else {
                        // Use UNC path
                        if remote_name.ends_with('\\') {
                            remote_name.clone()
                        } else {
                            format!("{}\\", remote_name)
                        }
                    };

                    // Check if the network drive is actually accessible
                    // REMEMBERED drives may need a reconnection attempt using WNetAddConnection2W
                    // Pass the remote UNC path so we can try to reconnect
                    let is_ready = check_network_drive_accessible_with_remote(&path, Some(&remote_name));

                    debug!(
                        "Found network connection: {} -> {} (accessible={})",
                        path, remote_name, is_ready
                    );

                    drives.push(DriveInfo {
                        path,
                        name: format!("Network ({})", remote_name),
                        drive_type: "Network Drive".to_string(),
                        total_space: 0,
                        free_space: 0,
                        is_ready,
                    });
                }
            }
        }

        unsafe { WNetCloseEnum(h_enum) };
    }

    if drives.is_empty() {
        None
    } else {
        info!("Found {} network connections without drive letters", drives.len());
        Some(drives)
    }
}

/// Get statistics for each drive in the database
#[tauri::command]
pub async fn get_drive_stats(state: State<'_, AppState>) -> Result<Vec<crate::database::DriveStatsResult>, String> {
    let db = state.db.lock().await;
    db.get_drive_stats().map_err(|e| e.to_string())
}

/// Get all drives with their online/offline status
/// Merges currently available drives with known (scanned) drives from database
#[tauri::command]
pub async fn get_drives_status(state: State<'_, AppState>) -> Result<Vec<DriveStatus>, String> {
    info!("Getting drives status with online/offline detection");

    // Get currently available drives
    let available_drives = get_available_drives().await?;
    let available_paths: HashSet<String> = available_drives
        .iter()
        .map(|d| d.path.to_uppercase())
        .collect();

    // Get known drives from database
    let db = state.db.lock().await;
    let known_drives = db.get_known_drives().map_err(|e| e.to_string())?;

    let mut result = Vec::new();

    // Process available drives
    for drive in &available_drives {
        let drive_path_upper = drive.path.to_uppercase();
        let known = known_drives.iter().find(|k| k.path.to_uppercase() == drive_path_upper);

        // Drive is truly online only if is_ready is true
        // (REMEMBERED network drives have is_ready=false)
        let is_truly_online = drive.is_ready;

        let (is_scanned, last_scan_at, indexed_files, indexed_size, status) = if let Some(k) = known {
            if is_truly_online {
                // Drive is available and has been scanned
                (true, Some(k.last_scan_at), k.total_files, k.total_size, DriveOnlineStatus::Online)
            } else {
                // Drive is remembered but disconnected
                (true, Some(k.last_scan_at), k.total_files, k.total_size, DriveOnlineStatus::Offline)
            }
        } else {
            if is_truly_online {
                // Drive is available but never scanned
                (false, None, 0, 0, DriveOnlineStatus::NeverScanned)
            } else {
                // Drive is remembered but disconnected and never scanned
                (false, None, 0, 0, DriveOnlineStatus::Offline)
            }
        };

        result.push(DriveStatus {
            path: drive.path.clone(),
            name: drive.name.clone(),
            drive_type: drive.drive_type.clone(),
            total_space: drive.total_space,
            free_space: drive.free_space,
            is_ready: drive.is_ready,
            is_online: is_truly_online,
            is_scanned,
            last_scan_at,
            indexed_files,
            indexed_size,
            status,
        });
    }

    // Add offline drives (known but not currently available)
    for known in &known_drives {
        if !available_paths.contains(&known.path.to_uppercase()) {
            result.push(DriveStatus {
                path: known.path.clone(),
                name: known.volume_name.clone().unwrap_or_else(|| format!("Offline ({})", known.path)),
                drive_type: known.drive_type.clone(),
                total_space: 0,
                free_space: 0,
                is_ready: false,
                is_online: false,
                is_scanned: true,
                last_scan_at: Some(known.last_scan_at),
                indexed_files: known.total_files,
                indexed_size: known.total_size,
                status: DriveOnlineStatus::Offline,
            });
        }
    }

    info!("Returning {} drives ({} online, {} offline)",
        result.len(),
        result.iter().filter(|d| d.is_online).count(),
        result.iter().filter(|d| !d.is_online).count()
    );

    Ok(result)
}

/// Remove an offline drive's data from the database
#[tauri::command]
pub async fn remove_offline_drive(
    state: State<'_, AppState>,
    drive_path: String,
) -> Result<(i64, i64), String> {
    info!("Removing offline drive data: {}", drive_path);

    let db = state.db.lock().await;
    db.remove_known_drive(&drive_path).map_err(|e| e.to_string())
}

/// Detect overlapping drives (e.g., Z:\ containing files from X:\ and T:\)
/// This can happen with merged drives like ZimaOS, SUBST, or network shares
#[tauri::command]
pub async fn detect_drive_overlaps(
    state: State<'_, AppState>,
    sample_size: Option<usize>,
    threshold_percent: Option<f64>,
) -> Result<Vec<crate::database::DriveOverlap>, String> {
    let sample = sample_size.unwrap_or(100);
    let threshold = threshold_percent.unwrap_or(50.0);

    info!("Detecting drive overlaps (sample={}, threshold={}%)", sample, threshold);

    let db = state.db.lock().await;
    let overlaps = db.detect_overlapping_drives(sample, threshold)
        .map_err(|e| e.to_string())?;

    if overlaps.is_empty() {
        info!("No overlapping drives detected");
    } else {
        for overlap in &overlaps {
            info!(
                "Overlap detected: {}% of {} files from {} also exist on {}",
                overlap.overlap_percent.round(),
                overlap.sample_size,
                overlap.source_drive,
                overlap.target_drive
            );
        }
    }

    Ok(overlaps)
}

/// Pre-scan overlap detection result
#[derive(Debug, Serialize, Clone)]
pub struct PreScanOverlapResult {
    /// The drive being checked
    pub drive_path: String,
    /// Whether this drive appears to be a merged/overlay drive
    pub is_overlapping: bool,
    /// Drives that this drive overlaps with
    pub overlaps_with: Vec<String>,
    /// Percentage of sampled files found on other drives
    pub overlap_percent: f64,
    /// Number of files sampled
    pub sample_size: usize,
    /// Recommendation: "scan", "skip", "warn"
    pub recommendation: String,
}

/// Check if a drive overlaps with already-scanned drives BEFORE scanning it
/// This samples files directly from the filesystem and checks if they exist in the DB
#[tauri::command]
pub async fn check_pre_scan_overlap(
    state: State<'_, AppState>,
    drive_path: String,
    sample_size: Option<usize>,
) -> Result<PreScanOverlapResult, String> {
    use std::path::Path;
    use walkdir::WalkDir;

    let sample = sample_size.unwrap_or(50);
    info!("Pre-scan overlap check for {} (sampling {} files)", drive_path, sample);

    // Sample files from the filesystem
    let mut sampled_files: Vec<String> = Vec::new();
    let root = Path::new(&drive_path);

    if !root.exists() {
        return Err(format!("Drive {} is not accessible", drive_path));
    }

    // Walk the filesystem and collect sample files
    for entry in WalkDir::new(&drive_path)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .take(sample * 10) // Collect more than needed, then sample randomly
    {
        let path = entry.path().to_string_lossy().to_string();
        // Get relative path (after drive root)
        let relative = if path.len() > drive_path.len() {
            path[drive_path.len()..].trim_start_matches('\\').trim_start_matches('/').to_string()
        } else {
            continue;
        };
        if !relative.is_empty() {
            sampled_files.push(relative);
        }
    }

    // Randomly sample if we have too many
    if sampled_files.len() > sample {
        use rand::seq::SliceRandom;
        let mut rng = rand::thread_rng();
        sampled_files.shuffle(&mut rng);
        sampled_files.truncate(sample);
    }

    if sampled_files.is_empty() {
        info!("No files found to sample on {}", drive_path);
        return Ok(PreScanOverlapResult {
            drive_path,
            is_overlapping: false,
            overlaps_with: vec![],
            overlap_percent: 0.0,
            sample_size: 0,
            recommendation: "scan".to_string(),
        });
    }

    info!("Sampled {} files from {}", sampled_files.len(), drive_path);

    // Check against database
    let db = state.db.lock().await;
    let known_drives = db.get_known_drives().map_err(|e| e.to_string())?;

    let mut overlaps_with: Vec<String> = Vec::new();
    let mut total_matches = 0usize;

    for known in &known_drives {
        // Skip if checking against the same drive
        if known.path.to_uppercase() == drive_path.to_uppercase() {
            continue;
        }

        let matches = db.count_matching_paths(&known.path, &sampled_files)
            .map_err(|e| e.to_string())?;

        if matches > 0 {
            let percent = (matches as f64 / sampled_files.len() as f64) * 100.0;
            info!(
                "Found {} matches ({:.1}%) with drive {}",
                matches, percent, known.path
            );
            if percent >= 30.0 {
                overlaps_with.push(known.path.clone());
            }
            total_matches += matches;
        }
    }

    let overlap_percent = if sampled_files.is_empty() {
        0.0
    } else {
        // Cap at 100% since same file could exist on multiple drives
        ((total_matches as f64 / sampled_files.len() as f64) * 100.0).min(100.0)
    };

    let is_overlapping = overlap_percent >= 50.0;
    let recommendation = if overlap_percent >= 80.0 {
        "skip".to_string() // Almost certainly a merged drive
    } else if overlap_percent >= 50.0 {
        "warn".to_string() // Likely overlapping, warn user
    } else {
        "scan".to_string() // Safe to scan
    };

    info!(
        "Pre-scan check result for {}: {}% overlap, recommendation={}",
        drive_path, overlap_percent.round(), recommendation
    );

    Ok(PreScanOverlapResult {
        drive_path,
        is_overlapping,
        overlaps_with,
        overlap_percent,
        sample_size: sampled_files.len(),
        recommendation,
    })
}
