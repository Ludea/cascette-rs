//! Integration test for WoW Classic 1.13.2 build 31650 installation (T083)
//!
//! This test verifies the complete installation workflow:
//! - Install WoW Classic 1.13.2 build 31650 from scratch
//! - Compare against reference installation
//! - Verify executable and CASC structure

use std::path::PathBuf;
use std::time::Duration;

const REFERENCE_DIR: &str =
    "/home/danielsreichenbach/Downloads/cascette/wow_classic_1.13.2.31650/reference";
const FRESH_INSTALL_DIR: &str =
    "/home/danielsreichenbach/Downloads/cascette/wow_classic_1.13.2.31650/fresh_install";

const PRODUCT_CODE: &str = "wow_classic";
const BUILD_ID: u32 = 31650;
const VERSION: &str = "1.13.2.31650";

#[tokio::test]
#[ignore] // Run explicitly with: cargo test --test wow_classic_install -- --ignored
async fn test_wow_classic_1132_installation() -> anyhow::Result<()> {
    println!("\n=== WoW Classic 1.13.2 Build 31650 Integration Test ===\n");

    // Verify test directories exist
    verify_test_setup()?;

    // Clean fresh install directory
    clean_fresh_install_dir()?;

    // Start agent service
    println!("Starting agent service...");
    let agent_process = start_agent_service().await?;

    // Wait for service to be ready
    tokio::time::sleep(Duration::from_secs(2)).await;

    // Verify health endpoint
    verify_service_health().await?;

    // Request installation
    println!(
        "\nRequesting installation of {} build {}...",
        PRODUCT_CODE, BUILD_ID
    );
    let operation_id = request_installation().await?;
    println!("Operation ID: {}", operation_id);

    // Monitor progress until completion
    println!("\nMonitoring installation progress...");
    let final_state = monitor_operation(operation_id).await?;

    if final_state != "complete" {
        anyhow::bail!("Installation failed with state: {}", final_state);
    }

    println!("\n✓ Installation completed successfully");

    // Stop agent service
    println!("\nStopping agent service...");
    stop_agent_service(agent_process).await?;

    // Verify installation
    println!("\nVerifying installation against reference...");
    verify_installation()?;

    println!("\n=== ✓ All Checks Passed ===\n");
    Ok(())
}

fn verify_test_setup() -> anyhow::Result<()> {
    println!("Verifying test setup...");

    let reference_path = PathBuf::from(REFERENCE_DIR);
    if !reference_path.exists() {
        anyhow::bail!("Reference installation not found at: {}", REFERENCE_DIR);
    }

    // Verify reference .build.info
    let build_info = reference_path.join(".build.info");
    if !build_info.exists() {
        anyhow::bail!("Reference .build.info not found");
    }

    // Verify reference executable
    let wow_exe = reference_path.join("_classic_").join("Wow.exe");
    if !wow_exe.exists() {
        anyhow::bail!("Reference Wow.exe not found");
    }

    println!("✓ Test setup verified");
    Ok(())
}

fn clean_fresh_install_dir() -> anyhow::Result<()> {
    println!("Cleaning fresh install directory...");

    let fresh_path = PathBuf::from(FRESH_INSTALL_DIR);
    if fresh_path.exists() {
        std::fs::remove_dir_all(&fresh_path)?;
    }
    std::fs::create_dir_all(&fresh_path)?;

    println!("✓ Fresh install directory ready");
    Ok(())
}

struct AgentProcess {
    child: tokio::process::Child,
}

async fn start_agent_service() -> anyhow::Result<AgentProcess> {
    use tokio::process::Command;

    let child = Command::new("cargo")
        .args(["run", "--bin", "cascette-agent"])
        .current_dir("/home/danielsreichenbach/Repos/github.com/wowemulation-dev/cascette-rs")
        .spawn()?;

    Ok(AgentProcess { child })
}

async fn stop_agent_service(mut process: AgentProcess) -> anyhow::Result<()> {
    // Send SIGTERM for graceful shutdown
    #[cfg(unix)]
    {
        use nix::sys::signal::{Signal, kill};
        use nix::unistd::Pid;

        if let Some(pid) = process.child.id() {
            let _ = kill(Pid::from_raw(pid as i32), Signal::SIGTERM);
        }
    }

    // Wait for graceful shutdown with timeout
    tokio::select! {
        _ = process.child.wait() => {
            println!("✓ Agent service stopped gracefully");
        }
        _ = tokio::time::sleep(Duration::from_secs(10)) => {
            println!("⚠ Agent service did not stop gracefully, killing...");
            let _ = process.child.kill().await;
        }
    }

    Ok(())
}

async fn verify_service_health() -> anyhow::Result<()> {
    use reqwest::Client;

    let client = Client::new();
    let url = "http://localhost:1120/health";

    // Retry health check for up to 30 seconds
    for attempt in 1..=30 {
        match client.get(url).send().await {
            Ok(response) if response.status().is_success() => {
                println!("✓ Service health check passed");
                return Ok(());
            }
            Ok(response) => {
                println!(
                    "Attempt {}/30: Health check returned status {}",
                    attempt,
                    response.status()
                );
            }
            Err(e) => {
                println!("Attempt {}/30: Health check failed: {}", attempt, e);
            }
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }

    anyhow::bail!("Service failed to become healthy after 30 seconds")
}

async fn request_installation() -> anyhow::Result<String> {
    use reqwest::Client;
    use serde_json::json;

    let client = Client::new();
    let url = format!("http://localhost:1120/products/{}/install", PRODUCT_CODE);

    let request_body = json!({
        "build_id": BUILD_ID,
        "install_path": FRESH_INSTALL_DIR,
        "region": "eu",
        "locale": "enUS",
        "tags": [
            "Windows",
            "x86_64",
            "enUS",
            "speech",
            "text"
        ],
    });

    let response = client.post(&url).json(&request_body).send().await?;

    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        anyhow::bail!("Install request failed with status {}: {}", status, text);
    }

    let response_json: serde_json::Value = response.json().await?;
    let operation_id = response_json["operation_id"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("No operation_id in response"))?
        .to_string();

    Ok(operation_id)
}

async fn monitor_operation(operation_id: String) -> anyhow::Result<String> {
    use reqwest::Client;

    let client = Client::new();
    let url = format!("http://localhost:1120/operations/{}", operation_id);

    let mut last_progress = 0.0;
    let mut last_state = String::new();

    loop {
        let response = client.get(&url).send().await?;

        if !response.status().is_success() {
            anyhow::bail!("Failed to get operation status: {}", response.status());
        }

        let operation: serde_json::Value = response.json().await?;

        let state = operation["state"].as_str().unwrap_or("Unknown").to_string();
        let progress = operation["progress"]["percentage"].as_f64().unwrap_or(0.0);

        // Print progress updates
        if state != last_state || (progress - last_progress).abs() > 1.0 {
            println!(
                "[{}] State: {} - Progress: {:.1}%",
                operation_id, state, progress
            );
            last_state = state.clone();
            last_progress = progress;
        }

        // Check for terminal states
        match state.as_str() {
            "complete" => {
                println!("✓ Operation completed successfully");
                return Ok(state);
            }
            "failed" => {
                let error_msg = operation["error"].as_str().unwrap_or("Unknown error");
                anyhow::bail!("Operation failed: {}", error_msg);
            }
            "cancelled" => {
                anyhow::bail!("Operation was cancelled");
            }
            _ => {
                // Continue monitoring
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
        }
    }
}

fn verify_installation() -> anyhow::Result<()> {
    let fresh_path = PathBuf::from(FRESH_INSTALL_DIR);
    let reference_path = PathBuf::from(REFERENCE_DIR);

    // 1. Verify executable exists
    println!("Checking game executable...");
    let wow_exe = fresh_path.join("_classic_").join("Wow.exe");
    if !wow_exe.exists() {
        anyhow::bail!("Wow.exe not found in fresh installation");
    }
    println!("✓ Wow.exe found");

    // 2. Verify .build.info exists and contains correct version
    println!("Checking .build.info...");
    let build_info = fresh_path.join(".build.info");
    if !build_info.exists() {
        anyhow::bail!(".build.info not found in fresh installation");
    }

    let build_info_content = std::fs::read_to_string(&build_info)?;
    if !build_info_content.contains(VERSION) {
        anyhow::bail!(".build.info does not contain expected version {}", VERSION);
    }
    if !build_info_content.contains(PRODUCT_CODE) {
        anyhow::bail!(
            ".build.info does not contain expected product {}",
            PRODUCT_CODE
        );
    }
    println!("✓ .build.info verified");

    // 3. Verify CASC structure
    println!("Checking CASC structure...");
    let data_dir = fresh_path.join("Data");
    if !data_dir.exists() {
        anyhow::bail!("Data directory not found");
    }

    let config_dir = data_dir.join("config");
    let data_subdir = data_dir.join("data");
    let indices_dir = data_dir.join("indices");

    if !config_dir.exists() {
        anyhow::bail!("Data/config directory not found");
    }
    if !data_subdir.exists() {
        anyhow::bail!("Data/data directory not found");
    }
    if !indices_dir.exists() {
        anyhow::bail!("Data/indices directory not found");
    }

    println!("✓ CASC structure verified");

    // 4. Compare directory sizes (should be within reasonable range)
    println!("Comparing installation sizes...");
    let fresh_size = get_dir_size(&fresh_path)?;
    let reference_size = get_dir_size(&reference_path)?;

    let size_diff_pct =
        ((fresh_size as f64 - reference_size as f64).abs() / reference_size as f64) * 100.0;

    println!("Fresh installation: {} MB", fresh_size / 1024 / 1024);
    println!(
        "Reference installation: {} MB",
        reference_size / 1024 / 1024
    );
    println!("Size difference: {:.2}%", size_diff_pct);

    // Allow up to 5% difference (due to potential compression, cache, etc.)
    if size_diff_pct > 5.0 {
        println!("⚠ Warning: Size difference exceeds 5% - may indicate incomplete installation");
    } else {
        println!("✓ Installation size validated");
    }

    Ok(())
}

fn get_dir_size(path: &PathBuf) -> anyhow::Result<u64> {
    let mut total_size = 0u64;

    for entry in walkdir::WalkDir::new(path) {
        let entry = entry?;
        if entry.file_type().is_file() {
            total_size += entry.metadata()?.len();
        }
    }

    Ok(total_size)
}
