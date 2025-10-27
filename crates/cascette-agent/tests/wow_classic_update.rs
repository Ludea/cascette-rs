//! Integration test for WoW Classic update workflow (T104)
//!
//! This test verifies the complete update workflow:
//! - Install WoW Classic 1.14.2 build 42597 from scratch
//! - Update to WoW Classic 1.15.7 build 63696
//! - Verify delta download (only changed files downloaded)
//! - Verify updated installation integrity

use std::path::PathBuf;
use std::time::Duration;

const INITIAL_INSTALL_DIR: &str =
    "/home/danielsreichenbach/Downloads/cascette/wow_classic_update_test/1.14.2";
const UPDATED_INSTALL_DIR: &str =
    "/home/danielsreichenbach/Downloads/cascette/wow_classic_update_test/1.15.7";
const REFERENCE_DIR: &str = "/home/danielsreichenbach/Downloads/wow_classic/1.15.7.63696";

const PRODUCT_CODE: &str = "wow_classic";
const INITIAL_BUILD_ID: u32 = 42597;
const INITIAL_VERSION: &str = "1.14.2.42597";
const TARGET_BUILD_ID: u32 = 63696;
const TARGET_VERSION: &str = "1.15.7.63696";

#[tokio::test]
#[ignore] // Run explicitly with: cargo test --test wow_classic_update -- --ignored
async fn test_wow_classic_update_workflow() -> anyhow::Result<()> {
    println!("\n=== WoW Classic Update Integration Test ===\n");
    println!(
        "Testing update from {} to {}",
        INITIAL_VERSION, TARGET_VERSION
    );

    // Verify test setup
    verify_test_setup()?;

    // Clean test directories
    clean_test_directories()?;

    // Start agent service
    println!("\nStarting agent service...");
    let agent_process = start_agent_service().await?;

    // Wait for service to be ready
    tokio::time::sleep(Duration::from_secs(2)).await;

    // Verify health endpoint
    verify_service_health().await?;

    // Phase 1: Install initial version (1.14.2)
    println!("\n=== Phase 1: Installing {} ===", INITIAL_VERSION);
    let install_op_id = request_installation(INITIAL_BUILD_ID).await?;
    println!("Installation operation ID: {}", install_op_id);

    let install_state = monitor_operation(install_op_id).await?;
    if install_state != "complete" {
        anyhow::bail!("Initial installation failed with state: {}", install_state);
    }
    println!("✓ Initial installation completed");

    // Phase 2: Update to target version (1.15.7)
    println!("\n=== Phase 2: Updating to {} ===", TARGET_VERSION);
    let update_op_id = request_update(TARGET_BUILD_ID).await?;
    println!("Update operation ID: {}", update_op_id);

    // Monitor and capture metrics
    let (update_state, metrics) = monitor_update_operation(update_op_id).await?;
    if update_state != "complete" {
        anyhow::bail!("Update failed with state: {}", update_state);
    }
    println!("✓ Update completed");

    // Verify delta download was used
    println!("\n=== Phase 3: Verifying Delta Download ===");
    verify_delta_metrics(&metrics)?;

    // Stop agent service
    println!("\nStopping agent service...");
    stop_agent_service(agent_process).await?;

    // Verify updated installation
    println!("\n=== Phase 4: Verifying Updated Installation ===");
    verify_updated_installation()?;

    println!("\n=== ✓ All Update Checks Passed ===\n");
    Ok(())
}

fn verify_test_setup() -> anyhow::Result<()> {
    println!("Verifying test setup...");

    let reference_path = PathBuf::from(REFERENCE_DIR);
    if !reference_path.exists() {
        anyhow::bail!(
            "Reference installation (1.15.7) not found at: {}",
            REFERENCE_DIR
        );
    }

    // Verify reference has required CASC structure
    let data_dir = reference_path.join("Data");
    if !data_dir.exists() {
        anyhow::bail!("Reference installation missing Data directory");
    }

    println!("✓ Test setup verified");
    Ok(())
}

fn clean_test_directories() -> anyhow::Result<()> {
    println!("Cleaning test directories...");

    for dir in [INITIAL_INSTALL_DIR, UPDATED_INSTALL_DIR] {
        let path = PathBuf::from(dir);
        if path.exists() {
            std::fs::remove_dir_all(&path)
                .map_err(|e| anyhow::anyhow!("Failed to clean directory {}: {}", dir, e))?;
        }
        std::fs::create_dir_all(&path)
            .map_err(|e| anyhow::anyhow!("Failed to create directory {}: {}", dir, e))?;
    }

    println!("✓ Test directories cleaned");
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

async fn request_installation(build_id: u32) -> anyhow::Result<String> {
    let client = reqwest::Client::new();
    let url = format!("http://127.0.0.1:1120/agent/products/{}", PRODUCT_CODE);

    let payload = serde_json::json!({
        "install_path": INITIAL_INSTALL_DIR,
        "build_id": build_id,
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

async fn request_update(build_id: u32) -> anyhow::Result<String> {
    let client = reqwest::Client::new();
    let url = format!("http://127.0.0.1:1120/agent/update/{}", PRODUCT_CODE);

    let payload = serde_json::json!({
        "build_id": build_id,
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

async fn monitor_update_operation(operation_id: String) -> anyhow::Result<(String, UpdateMetrics)> {
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
            if let Some(bytes_downloaded) =
                progress.get("bytes_downloaded").and_then(|v| v.as_u64())
            {
                metrics.bytes_downloaded = bytes_downloaded;
            }
            if let Some(bytes_total) = progress.get("bytes_total").and_then(|v| v.as_u64()) {
                metrics.bytes_total = bytes_total;
            }
            if let Some(files_completed) = progress.get("files_completed").and_then(|v| v.as_u64())
            {
                metrics.files_updated = files_completed;
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
                    let mb_downloaded = metrics.bytes_downloaded as f64 / 1024.0 / 1024.0;
                    let mb_total = metrics.bytes_total as f64 / 1024.0 / 1024.0;
                    println!(
                        "  Progress: {:.1}% ({}) - {:.1} MB / {:.1} MB",
                        percentage, phase, mb_downloaded, mb_total
                    );
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
    bytes_total: u64,
    files_updated: u64,
}

fn verify_delta_metrics(metrics: &UpdateMetrics) -> anyhow::Result<()> {
    // A full WoW Classic 1.15.7 install is approximately 4GB
    // An update from 1.14.2 should be significantly smaller (typically < 1GB)
    const FULL_INSTALL_SIZE_GB: f64 = 4.0;
    const MAX_DELTA_PERCENT: f64 = 40.0; // Update should be < 40% of full install

    let gb_downloaded = metrics.bytes_downloaded as f64 / 1024.0 / 1024.0 / 1024.0;
    let delta_percent = (gb_downloaded / FULL_INSTALL_SIZE_GB) * 100.0;

    println!("  Downloaded: {:.2} GB", gb_downloaded);
    println!(
        "  Delta efficiency: {:.1}% of full install size",
        delta_percent
    );

    if delta_percent > MAX_DELTA_PERCENT {
        anyhow::bail!(
            "Update downloaded too much data: {:.1}% of full install (expected < {}%)",
            delta_percent,
            MAX_DELTA_PERCENT
        );
    }

    if metrics.files_updated == 0 {
        anyhow::bail!("No files were updated (expected > 0)");
    }

    println!("  Files updated: {}", metrics.files_updated);
    println!("✓ Delta download verified (efficient update)");
    Ok(())
}

fn verify_updated_installation() -> anyhow::Result<()> {
    let install_path = PathBuf::from(INITIAL_INSTALL_DIR);

    // Verify CASC structure exists
    let data_dir = install_path.join("Data");
    if !data_dir.exists() {
        anyhow::bail!("Updated installation missing Data directory");
    }

    // Verify .build.info was updated
    let build_info = install_path.join(".build.info");
    if !build_info.exists() {
        anyhow::bail!("Updated installation missing .build.info");
    }

    let build_info_content = std::fs::read_to_string(&build_info)
        .map_err(|e| anyhow::anyhow!("Failed to read .build.info: {}", e))?;

    if !build_info_content.contains(&TARGET_BUILD_ID.to_string()) {
        anyhow::bail!(
            ".build.info not updated to target version (expected build_id: {})",
            TARGET_BUILD_ID
        );
    }

    println!("✓ .build.info updated to target version");

    // Verify key CASC directories
    for subdir in ["config", "data", "indices"] {
        let dir_path = data_dir.join(subdir);
        if !dir_path.exists() {
            anyhow::bail!("Updated installation missing Data/{} directory", subdir);
        }
    }

    println!("✓ CASC directory structure intact");
    println!("✓ Updated installation verified");
    Ok(())
}

async fn stop_agent_service(mut agent_process: tokio::process::Child) -> anyhow::Result<()> {
    agent_process
        .kill()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to stop agent service: {}", e))?;
    Ok(())
}
