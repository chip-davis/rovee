//! Scratch test for the `govee` crate, separate from the TUI.
//!
//!   cargo run --example govee_test          # list lights found on the LAN
//!   cargo run --example govee_test -- on    # also turn the first one on
//!   cargo run --example govee_test -- off   # ...or off
//!
//! Uses the local LAN API: no API key needed, but the lights must have
//! "LAN Control" enabled in the Govee Home app and be on the same network.

use govee::backend::GoveeBackend;
use govee::backend::local::LocalBackend;
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 2s discovery timeout, 60s device cache
    let backend = LocalBackend::new(Duration::from_secs(2), 60).await?;
    let devices = backend.list_devices().await?;

    if devices.is_empty() {
        println!("No Govee devices found on the LAN.");
        println!("Check that LAN Control is enabled for them in the Govee Home app.");
        return Ok(());
    }

    for d in &devices {
        println!("{}  {}  ({})", d.id, d.name, d.model);
        match backend.get_state(&d.id).await {
            Ok(state) => println!("    {state:?}"),
            Err(e) => println!("    state unavailable: {e}"),
        }
    }

    if let Some(arg) = std::env::args().nth(1) {
        let on = match arg.as_str() {
            "on" => true,
            "off" => false,
            other => return Err(format!("unknown argument {other:?}, use on/off").into()),
        };
        let first = &devices[0];
        backend.set_power(&first.id, on).await?;
        println!("Turned {} {}", first.name, if on { "on" } else { "off" });
    }

    Ok(())
}
