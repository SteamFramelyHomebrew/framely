// Run the same discovery code as the UI, without opening a window.
use framely_installer::discovery;
use std::sync::{Arc, atomic::AtomicBool};
fn main() -> anyhow::Result<()> {
    let cidr = std::env::args().nth(1).unwrap_or_else(|| {
        discovery::networks()
            .into_iter()
            .next()
            .unwrap_or_else(|| "192.168.1.0/24".into())
    });
    discovery::scan(
        &cidr,
        Arc::new(AtomicBool::new(false)),
        Arc::new(|device| {
            println!("{} {} SSH={}", device.name, device.ip, device.ssh);
        }),
    )
}
