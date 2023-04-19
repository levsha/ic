use candid::Principal;
use std::path::Path;
use std::process::{Child, Command};
use std::str::FromStr;
use tokio::time::{sleep, Duration};

struct KillOnDrop(Child);

pub struct RosettaContext {
    _proc: KillOnDrop,
    _state: tempfile::TempDir,
    pub port: u16,
}

impl RosettaContext {
    pub fn port(&self) -> u16 {
        self.port
    }
}

impl Drop for KillOnDrop {
    fn drop(&mut self) {
        let _ = self.0.kill();
    }
}

pub async fn start_rosetta(
    rosetta_bin: &Path,
    ledger_canister_id: Principal,
    network_url: String,
) -> RosettaContext {
    assert!(
        rosetta_bin.exists(),
        "ic-icrc-rosetta-bin path {} does not exist",
        rosetta_bin.display()
    );

    let state = tempfile::TempDir::new().expect("failed to create a temporary directory");
    let port_file = state.path().join("port");

    let _proc = KillOnDrop(
        Command::new(rosetta_bin)
            .arg("--ledger-id")
            .arg(ledger_canister_id.to_string())
            .arg("--network-type")
            .arg("testnet")
            .arg("--network-url")
            .arg(network_url)
            .arg("--port-file")
            .arg(port_file.clone())
            .stdout(std::process::Stdio::inherit())
            .stderr(std::process::Stdio::inherit())
            .spawn()
            .unwrap_or_else(|e| {
                panic!(
                    "Failed to execute ic-icrc-rosetta-bin (path = {}, exists? = {}): {}",
                    rosetta_bin.display(),
                    rosetta_bin.exists(),
                    e
                )
            }),
    );

    let mut tries_left = 100;
    while tries_left > 0 && !port_file.exists() {
        sleep(Duration::from_millis(100)).await;
        tries_left -= 1;
    }

    let port = std::fs::read_to_string(port_file).expect("Expected port in port file");
    let port = u16::from_str(&port)
        .unwrap_or_else(|e| panic!("Expected port in port file, got {}: {}", port, e));

    RosettaContext {
        _proc,
        _state: state,
        port,
    }
}