mod auth;
mod http;
mod ipc;
mod jobs;
mod localization;
mod model;
mod package;
mod planner;
mod process;
mod recovery;
mod relations;
mod service;
mod session;
mod subscriptions;
#[cfg(test)]
mod tests;
mod update;
mod uploads;
use anyhow::{Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use clap::{Parser, Subcommand};
use serde_json::{json, Value};
use std::{fs, path::PathBuf};

#[derive(Parser)]
#[command(version, about = "Framely plugin manager")]
struct Cli {
    #[command(subcommand)]
    command: Cmd,
}
#[derive(Subcommand)]
enum Cmd {
    /// Disable and uninstall every plugin before removing the manager.
    PrepareUninstall {
        #[arg(long)]
        approve: bool,
        #[arg(long, default_value = "/run/framely/control.sock")]
        socket: PathBuf,
    },
    Daemon {
        #[arg(long, default_value = "/var/lib/framely")]
        state: PathBuf,
        #[arg(long, default_value = "/run/framely/control.sock")]
        socket: PathBuf,
        #[arg(long)]
        manager_uid: u32,
    },
    Session {
        #[arg(long, default_value = "/run/framely/control.sock")]
        socket: PathBuf,
        #[arg(long, default_value = "/var/lib/framely")]
        state: PathBuf,
        #[arg(long)]
        assets: PathBuf,
        #[arg(long)]
        native: PathBuf,
        // Accept old service arguments across upgrades and rollbacks.
        #[arg(long, hide = true)]
        mailbox: Option<PathBuf>,
        #[arg(long)]
        http_only: bool,
    },
    Pack {
        #[arg(long)]
        manifest: PathBuf,
        #[arg(long)]
        payload: PathBuf,
        #[arg(long)]
        output: PathBuf,
    },
    Verify {
        package: PathBuf,
    },
    Status {
        #[arg(long, default_value = "/run/framely/control.sock")]
        socket: PathBuf,
    },
    Install {
        package: PathBuf,
        #[arg(long)]
        source: Option<String>,
        #[arg(long, value_name = "PLUGIN_ID=CATALOG_URL")]
        dependency_source: Vec<String>,
        #[arg(long)]
        approve: bool,
        #[arg(long)]
        approve_run_as: bool,
        #[arg(long, default_value = "/run/framely/control.sock")]
        socket: PathBuf,
    },
    Call {
        method: String,
        #[arg(default_value = "{}")]
        params: String,
        #[arg(long, default_value = "/run/framely/control.sock")]
        socket: PathBuf,
    },
    ReleaseManifest {
        #[arg(long)]
        archive: PathBuf,
        #[arg(long)]
        url: String,
        #[arg(long)]
        output: PathBuf,
        #[arg(long, default_value = "")]
        changelog: String,
    },
    ApplyUpdate {
        #[arg(long, default_value = "/var/lib/framely")]
        state: PathBuf,
        #[arg(long)]
        manager_uid: u32,
        #[arg(long)]
        stage: Option<PathBuf>,
        #[arg(long)]
        rollback: bool,
    },
    Catalog {
        #[arg(long)]
        name: String,
        #[arg(long)]
        base_url: String,
        #[arg(long)]
        packages: PathBuf,
        #[arg(long)]
        output: PathBuf,
    },
}
fn run() -> Result<()> {
    match Cli::parse().command {
        Cmd::PrepareUninstall { approve, socket } => {
            anyhow::ensure!(unsafe { libc::geteuid() } == 0, "Uninstall requires root");
            let result = ipc::call_timeout(
                &socket,
                "system.uninstall.prepare",
                json!({"approve":approve}),
                std::time::Duration::from_secs(15 * 60),
            )?;
            println!("{}", serde_json::to_string(&result)?);
            Ok(())
        }
        Cmd::Daemon {
            state,
            socket,
            manager_uid,
        } => service::serve(&state, &socket, manager_uid),
        Cmd::Session {
            socket,
            state,
            assets,
            native,
            mailbox: _,
            http_only,
        } => session::serve(socket, state, assets, native, http_only),
        Cmd::Pack {
            manifest,
            payload,
            output,
        } => package::pack(&manifest, &payload, &output),
        Cmd::Verify { package: p } => {
            let v = package::verify(&fs::read(p)?)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({"manifest":v.manifest}))?
            );
            Ok(())
        }
        Cmd::Status { socket } => {
            println!(
                "{}",
                serde_json::to_string_pretty(&ipc::call(&socket, "status", json!({}))?)?
            );
            Ok(())
        }
        Cmd::Install {
            package: p,
            approve,
            approve_run_as,
            source,
            dependency_source,
            socket,
        } => {
            let bytes = fs::read(p)?;
            let info = ipc::call(
                &socket,
                "inspect",
                json!({"package":STANDARD.encode(&bytes)}),
            )?;
            let database: model::Database = serde_json::from_value(info["database"].clone())?;
            let mut choices = std::collections::BTreeMap::new();
            for value in dependency_source {
                let (id, url) = value
                    .split_once('=')
                    .context("依赖来源格式：PLUGIN_ID=CATALOG_URL")?;
                model::valid_id(id)?;
                choices.insert(id.to_owned(), url.to_owned());
            }
            let prepared = match planner::prepare(&database, bytes, source, choices) {
                Ok(p) => p,
                Err(e) => {
                    if let Some(choice) = e.downcast_ref::<planner::ChoiceRequired>() {
                        eprintln!("{}", serde_json::to_string_pretty(choice)?);
                    }
                    return Err(e);
                }
            };
            let items:Vec<_>=prepared.plan.items.iter().map(|i|json!({"id":i.manifest.id,"name":i.manifest.name,"version":i.manifest.version,"action":i.action,"runAs":i.manifest.run_as(),"memoryLimitMiB":i.manifest.memory_limit_mib(),"source":i.source,"runAsChanged":i.run_as_changed})).collect();
            println!(
                "{}",
                serde_json::to_string_pretty(
                    &json!({"items":items,"addSources":prepared.plan.add_sources,"enable":prepared.plan.enable,"disable":prepared.plan.disable,"affected":prepared.plan.affected})
                )?
            );
            anyhow::ensure!(approve, "请检查安装计划后使用 --approve 确认安装");
            let packages: Vec<_> = prepared
                .packages
                .iter()
                .map(|(bytes, source)| {
                    let mut request = bytes.request();
                    request["source"] = json!(source);
                    request
                })
                .collect();
            let result = ipc::call_timeout(
                &socket,
                "install.batch",
                json!({"plan":prepared.plan,"packages":packages,"approve":true,"approveRunAs":approve_run_as}),
                std::time::Duration::from_secs(1800),
            )?;
            println!("{}", serde_json::to_string_pretty(&result)?);
            Ok(())
        }
        Cmd::Call {
            method,
            params,
            socket,
        } => {
            let params: Value = serde_json::from_str(&params)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&ipc::call(&socket, &method, params)?)?
            );
            Ok(())
        }
        Cmd::ReleaseManifest {
            archive,
            url,
            output,
            changelog,
        } => update::create_descriptor(&archive, &url, &output, &changelog),
        Cmd::ApplyUpdate {
            state,
            manager_uid,
            stage,
            rollback,
        } => update::apply(&state, manager_uid, stage, rollback),
        Cmd::Catalog {
            name,
            base_url,
            packages,
            output,
        } => {
            model::validate_url(&base_url, true)?;
            let mut entries = Vec::new();
            for f in fs::read_dir(packages)? {
                let p = f?.path();
                if p.extension().and_then(|s| s.to_str()) != Some("framely") {
                    continue;
                }
                let bytes = fs::read(&p)?;
                let v = package::verify(&bytes)?;
                let publish = v.manifest.publish.clone().unwrap_or_default();
                entries.push(model::CatalogEntry {
                    run_as: Some(v.manifest.run_as()),
                    dependencies: v.manifest.dependencies.clone(),
                    optional_dependencies: v.manifest.optional_dependencies.clone(),
                    conflicts: v.manifest.conflicts.clone(),
                    exclusive_resources: v.manifest.exclusive_resources.clone(),
                    id: v.manifest.id,
                    name: v.manifest.name,
                    version: v.manifest.version,
                    description: v.manifest.description,
                    author: v.manifest.author,
                    author_url: v.manifest.author_url,
                    documentation_url: v.manifest.documentation_url,
                    homepage: v.manifest.homepage,
                    api_version: v.manifest.api_version,
                    url: v.manifest.download_url.unwrap_or(format!(
                        "{}/{}",
                        base_url.trim_end_matches('/'),
                        p.file_name()
                            .context("Missing filename")?
                            .to_str()
                            .context("Non UTF8 filename")?
                    )),
                    sha256: package::digest(&bytes),
                    icon: publish.icon,
                    details: v.manifest.details,
                    _legacy_category: None,
                    tags: v.manifest.tags,
                    screenshots: publish.screenshots,
                    changelog: v.manifest.changelog,
                });
            }
            entries.sort_by(|a, b| {
                a.id.cmp(&b.id).then_with(|| {
                    match (
                        semver::Version::parse(&a.version),
                        semver::Version::parse(&b.version),
                    ) {
                        (Ok(a), Ok(b)) => b.cmp(&a),
                        _ => b.version.cmp(&a.version),
                    }
                })
            });
            let directory = output.parent().unwrap_or(std::path::Path::new("."));
            let mut latest = Vec::new();
            let mut groups = std::collections::BTreeMap::<String, Vec<model::CatalogEntry>>::new();
            for entry in entries {
                groups.entry(entry.id.clone()).or_default().push(entry);
            }
            for (id, mut versions) in groups {
                let mut seen = std::collections::BTreeSet::new();
                anyhow::ensure!(
                    versions.iter().all(|v| seen.insert(v.version.clone())),
                    "Duplicate plugin version: {id}"
                );
                latest.push(versions.remove(0));
                let history_dir = directory.join("plugins").join(&id);
                fs::create_dir_all(&history_dir)?;
                fs::write(
                    history_dir.join("versions.json"),
                    serde_json::to_vec_pretty(&model::PluginVersions {
                        schema_version: 1,
                        id,
                        versions,
                    })?,
                )?;
            }
            fs::write(
                output,
                serde_json::to_vec_pretty(&model::Catalog {
                    schema_version: 1,
                    name,
                    plugins: latest,
                })?,
            )?;
            Ok(())
        }
    }
}
fn main() {
    if let Err(e) = run() {
        eprintln!("Framely: {e:#}");
        std::process::exit(1);
    }
}
