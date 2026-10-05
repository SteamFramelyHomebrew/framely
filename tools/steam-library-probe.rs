//! Diagnostic: enumerate local Steam libraries using the same code as the session agent.
#[path = "../src/steam.rs"] mod steam;
mod process {
    pub fn tool(name:&str)->std::process::Command {std::process::Command::new(name)}
    pub fn user_home(uid: u32) -> anyhow::Result<String> {
        for line in std::fs::read_to_string("/etc/passwd")?.lines() {
            let fields: Vec<_> = line.split(':').collect();
            if fields.len() >= 7 && fields[2] == uid.to_string() { return Ok(fields[5].into()); }
        }
        anyhow::bail!("User home not found")
    }
}
fn main() -> anyhow::Result<()> {
    let apps=steam::discover(&steam::home()?);
    for app in &apps { println!("{}\t{}\ticon={}",app.id,app.name,app.icon.is_some()); }
    println!("apps={}",apps.len());Ok(())
}
