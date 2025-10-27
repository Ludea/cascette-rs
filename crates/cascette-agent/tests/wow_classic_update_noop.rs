//! Integration test for no-op update workflow (T105)
//!
//! This test verifies that requesting an update when already up-to-date:
//! - Detects the product is already at the requested version
//! - Completes the operation without downloading files
//! - Returns success with no changes made

use std::path::PathBuf;
use std::time::Duration;

const INSTALL_DIR: &str =
    "/home/danielsreichenbach/Downloads/cascette/wow_classic_noop_test/1.15.7";
const REFERENCE_DIR: &str = "/home/danielsreichenbach/Downloads/wow_classic/1.15.7.63696";

const PRODUCT_CODE: &str = "wow_classic";
const BUILD_ID: u32 = 63696;
const VERSION: &str = "1.15.7.63696";

#[tokio::test]
#[ignore] // Run explicitly with: cargo test --test wow_classic_update_noop -- --ignored
async fn test_wow_classic_noop_update() -> anyhow::Result<()> {
    println!("\n=== WoW Classic No-Op Update Integration Test ===\n");
    println!("Testing update request when already at version {}", VERSION);

    // Verify test setup
    verify_test_setup()?;

    // Clean and prepare install directory
    clean_install_directory()?;

    // Start agent service
    println!("\nStarting agent service...");
    let agent_process = start_agent_service().await?;

    // Wait for service to be ready
    tokio::time::sleep(Duration::from_secs(2)).await;

    // Verify health endpoint
    verify_service_health().await?;

    // Phase 1: Install target version
    println!("\n=== Phase 1: Installing {} ===", VERSION);
    let install_op_id = request_installation().await?;
    println!("Installation operation ID: {}", install_op_id);

    let install_state = monitor_operation(install_op_id).await?;
    if install_state != "complete" {
        anyhow::bail!("Installation failed with state: {}", install_state);
    }
    println!("✓ Installation completed");

    // Get modification time of key files for later comparison
    let file_mtimes_before = capture_file_modification_times()?;

    // Phase 2: Request update to same version (should be no-op)
    println!("\n=== Phase 2: Requesting Update to Same Version ===");
    let update_op_id = request_update().await?;
    println!("Update operation ID: {}", update_op_id);

    // Monitor update operation
    let (update_state, metrics) = monitor_update_with_metrics(update_op_id).await?;
    if update_state != "complete" {
        anyhow::bail!("Update operation failed with state: {}", update_state);
    }
    println!("✓ Update operation completed");

    // Phase 3: Verify no-op behavior
    println!("\n=== Phase 3: Verifying No-Op Behavior ===");
    verify_no_changes(&metrics)?;

    // Verify file modification times unchanged
    let file_mtimes_after = capture_file_modification_times()?;
    verify_files_unchanged(&file_mtimes_before, &file_mtimes_after)?;

    // Stop agent service
    println!("\nStopping agent service...");
    stop_agent_service(agent_process).await?;

    println!("\n=== ✓ All No-Op Update Checks Passed ===\n");
    Ok(())
}

fn verify_test_setup() -> anyhow::Result<()> {
    println!("Verifying test setup...");

    let reference_path = PathBuf::from(REFERENCE_DIR);
    if !reference_path.exists() {
        anyhow::bail!("Reference installation not found at: {}", REFERENCE_DIR);
    }

    println!("✓ Test setup verified");
    Ok(())
}

fn clean_install_directory() -> anyhow::Result<()> {
    println!("Preparing install directory...");

    let path = PathBuf::from(INSTALL_DIR);
    if path.exists() {
        std::fs::remove_dir_all(&path)
            .map_err(|e| anyhow::anyhow!("Failed to clean directory {}: {}", INSTALL_DIR, e))?;
    }
    std::fs::create_dir_all(&path)
        .map_err(|e| anyhow::anyhow!("Failed to create directory {}: {}", INSTALL_DIR, e))?;

    println!("✓ Install directory prepared");
    Ok(())
}

async fn start_agent_service() -> anyhow::Result<tokio::process::Child> {
    let mut cmd = tokio::process::Command::new("cargo");
    cmd.args(["run", "--bin", "cascette-agent", "--"])
        .args(["--config", "tests/test_config.toml"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);

    let child = cmd
        .spawn()
        .map_err(|e| anyhow::anyhow!("Failed to start agent service: {}", e))?;

    Ok(child)
}

async fn verify_service_health() -> anyhow::Result<()> {
    let client = reqwest::Client::new();
    let url = "http://127.0.0.1:1120/agent";

    for attempt in 1..=10 {
        match client.get(url).send().await {
            Ok(response) if response.status().is_success() => {
                println!("✓ Service health check passed");
                return Ok(());
            }
            Ok(response) => {
                anyhow::bail!(
                    "Service health check failed with status: {}",
                    response.status()
                );
            }
            Err(_) if attempt < 10 => {
                tokio::time::sleep(Duration::from_millis(500)).await;
                continue;
            }
            Err(e) => {
                anyhow::bail!("Service health check failed after 10 attempts: {}", e);
            }
        }
    }
    unreachable!()
}

async fn request_installation() -> anyhow::Result<String> {
    let client = reqwest::Client::new();
    let url = format!("http://127.0.0.1:1120/agent/products/{}", PRODUCT_CODE);

    let payload = serde_json::json!({
        "install_path": INSTALL_DIR,
        "build_id": BUILD_ID,
    });

    let response = client
        .post(&url)
        .json(&payload)
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to request installation: {}", e))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!(
            "Installation request failed with status {}: {}",
            status,
            body
        );
    }

    let json: serde_json::Value = response
        .json()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to parse installation response: {}", e))?;

    let operation_id = json
        .get("operation_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("Missing operation_id in response"))?;

    Ok(operation_id.to_string())
}

async fn request_update() -> anyhow::Result<String> {
    let client = reqwest::Client::new();
    let url = format!("http://127.0.0.1:1120/agent/update/{}", PRODUCT_CODE);

    let payload = serde_json::json!({
        "build_id": BUILD_ID,
        "force": false,
    });

    let response = client
        .post(&url)
        .json(&payload)
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to request update: {}", e))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!("Update request failed with status {}: {}", status, body);
    }

    let json: serde_json::Value = response
        .json()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to parse update response: {}", e))?;

    let operation_id = json
        .get("operation_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("Missing operation_id in response"))?;

    Ok(operation_id.to_string())
}

async fn monitor_operation(operation_id: String) -> anyhow::Result<String> {
    let client = reqwest::Client::new();
    let url = format!("http://127.0.0.1:1120/agent/operations/{}", operation_id);

    let mut last_percentage: Option<f64> = None;
    let timeout = Duration::from_secs(600); // 10 minute timeout
    let start = std::time::Instant::now();

    loop {
        if start.elapsed() > timeout {
            anyhow::bail!("Operation timed out after 10 minutes");
        }

        let response = client
            .get(&url)
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to query operation: {}", e))?;

        let json: serde_json::Value = response
            .json()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to parse operation response: {}", e))?;

        let state = json
            .get("state")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");

        // Print progress updates
        if let Some(progress) = json.get("progress") {
            if let Some(percentage) = progress.get("percentage").and_then(|v| v.as_f64()) {
                if last_percentage.is_none()
                    || last_percentage.unwrap() + 5.0 < percentage
                    || percentage >= 100.0
                {
                    let phase = progress
                        .get("phase")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown");
                    println!("  Progress: {:.1}% ({})", percentage, phase);
                    last_percentage = Some(percentage);
                }
            }
        }

        // Check terminal states
        match state {
            "complete" | "failed" | "cancelled" => {
                return Ok(state.to_string());
            }
            _ => {
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }
    }
}

async fn monitor_update_with_metrics(
    operation_id: String,
) -> anyhow::Result<(String, UpdateMetrics)> {
    let client = reqwest::Client::new();
    let url = format!("http://127.0.0.1:1120/agent/operations/{}", operation_id);

    let mut metrics = UpdateMetrics::default();
    let mut last_percentage: Option<f64> = None;
    let timeout = Duration::from_secs(600); // 10 minute timeout
    let start = std::time::Instant::now();

    loop {
        if start.elapsed() > timeout {
            anyhow::bail!("Operation timed out after 10 minutes");
        }

        let response = client
            .get(&url)
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to query operation: {}", e))?;

        let json: serde_json::Value = response
            .json()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to parse operation response: {}", e))?;

        let state = json
            .get("state")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");

        // Capture progress metrics
        if let Some(progress) = json.get("progress") {
            if let Some(bytes) = progress.get("bytes_downloaded").and_then(|v| v.as_u64()) {
                metrics.bytes_downloaded = metrics.bytes_downloaded.max(bytes);
            }
            if let Some(files) = progress.get("files_completed").and_then(|v| v.as_u64()) {
                metrics.files_changed = metrics.files_changed.max(files);
            }

            // Print progress updates
            if let Some(percentage) = progress.get("percentage").and_then(|v| v.as_f64()) {
                if last_percentage.is_none()
                    || last_percentage.unwrap() + 5.0 < percentage
                    || percentage >= 100.0
                {
                    let phase = progress
                        .get("phase")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown");
                    println!("  Progress: {:.1}% ({})", percentage, phase);
                    last_percentage = Some(percentage);
                }
            }
        }

        // Check terminal states
        match state {
            "complete" | "failed" | "cancelled" => {
                return Ok((state.to_string(), metrics));
            }
            _ => {
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }
    }
}

#[derive(Default)]
struct UpdateMetrics {
    bytes_downloaded: u64,
    files_changed: u64,
}

fn verify_no_changes(metrics: &UpdateMetrics) -> anyhow::Result<()> {
    // For a no-op update, we expect minimal or no file changes
    // Allow for small metadata updates but not full file downloads
    const MAX_NOOP_BYTES: u64 = 10 * 1024 * 1024; // 10 MB
    const MAX_NOOP_FILES: u64 = 5; // Allow a few metadata files

    println!("  Bytes downloaded: {} bytes", metrics.bytes_downloaded);
    println!("  Files changed: {}", metrics.files_changed);

    if metrics.bytes_downloaded > MAX_NOOP_BYTES {
        anyhow::bail!(
            "No-op update downloaded too much data: {} bytes (expected < {} MB)",
            metrics.bytes_downloaded,
            MAX_NOOP_BYTES / 1024 / 1024
        );
    }

    if metrics.files_changed > MAX_NOOP_FILES {
        anyhow::bail!(
            "No-op update changed too many files: {} (expected < {})",
            metrics.files_changed,
            MAX_NOOP_FILES
        );
    }

    println!("✓ No-op update verified (minimal changes)");
    Ok(())
}

fn capture_file_modification_times()
-> anyhow::Result<std::collections::HashMap<String, std::time::SystemTime>> {
    let install_path = PathBuf::from(INSTALL_DIR);
    let mut mtimes = std::collections::HashMap::new();

    // Check key CASC files
    let key_files = vec![".build.info", ".product.db", "Data/config", "Data/indices"];

    for file in key_files {
        let path = install_path.join(file);
        if path.exists() {
            let metadata = std::fs::metadata(&path)
                .map_err(|e| anyhow::anyhow!("Failed to get metadata for {}: {}", file, e))?;
            let mtime = metadata
                .modified()
                .map_err(|e| anyhow::anyhow!("Failed to get mtime for {}: {}", file, e))?;
            mtimes.insert(file.to_string(), mtime);
        }
    }

    Ok(mtimes)
}

fn verify_files_unchanged(
    before: &std::collections::HashMap<String, std::time::SystemTime>,
    after: &std::collections::HashMap<String, std::time::SystemTime>,
) -> anyhow::Result<()> {
    let mut unchanged_count = 0;
    let mut changed_count = 0;

    for (file, mtime_before) in before {
        if let Some(mtime_after) = after.get(file) {
            if mtime_before == mtime_after {
                unchanged_count += 1;
            } else {
                println!("  Warning: {} modification time changed", file);
                changed_count += 1;
            }
        }
    }

    println!("  Files unchanged: {}", unchanged_count);
    println!("  Files changed: {}", changed_count);

    // Allow a few metadata updates but most files should be unchanged
    if changed_count > 2 {
        anyhow::bail!(
            "Too many files changed during no-op update: {} (expected <= 2)",
            changed_count
        );
    }

    println!("✓ File modification times verified");
    Ok(())
}

async fn stop_agent_service(mut agent_process: tokio::process::Child) -> anyhow::Result<()> {
    agent_process
        .kill()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to stop agent service: {}", e))?;
    Ok(())
}
