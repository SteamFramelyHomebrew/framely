use crate::{model::*, package, service::Service};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs,
    io::{Cursor, Write},
    path::Path,
};
use zip::{write::SimpleFileOptions, ZipArchive, ZipWriter};
pub(crate) fn accepted_service(root: &Path, manager: u32) -> anyhow::Result<Service> {
    let mut core = Service::load(root, manager)?;
    core.handle(
        "agreement.accept",
        json!({"version":AGREEMENT_VERSION,"userAgreement":true,"privacyStatement":true}),
    )?;
    Ok(core)
}
pub(super) fn fixture(
    root: &Path,
    id: &str,
    version: &str,
    variant: u8,
    run_as: Option<&str>,
) -> Vec<u8> {
    let payload = root.join(format!("payload-{version}-{variant}"));
    fs::create_dir_all(&payload).unwrap();
    fs::write(payload.join("page.js"), "console.log('test')").unwrap();
    fs::write(payload.join("backend"), "#!/bin/sh\nexit 0\n").unwrap();
    let m = json!({"schemaVersion":1,"apiVersion":1,"id":id,"name":"Test","author":"Developer","version":version,"backend":run_as.map(|r|json!({"entry":"backend","runAs":r})),"ui":{"quickPage":"page.js"},"files":{}});
    let manifest = root.join(format!("manifest-{version}-{variant}.json"));
    fs::write(&manifest, serde_json::to_vec(&m).unwrap()).unwrap();
    let out = root.join(format!("{version}-{variant}.framely"));
    package::pack(&manifest, &payload, &out).unwrap();
    fs::read(out).unwrap()
}
fn rewrite(bytes: &[u8], replace: impl Fn(&str, &mut Vec<u8>)) -> Vec<u8> {
    use std::io::Read;
    let mut input = ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut output = ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..input.len() {
        let mut f = input.by_index(i).unwrap();
        let name = f.name().to_owned();
        let mut data = Vec::new();
        f.read_to_end(&mut data).unwrap();
        replace(&name, &mut data);
        output
            .start_file(name, SimpleFileOptions::default())
            .unwrap();
        output.write_all(&data).unwrap();
    }
    output.finish().unwrap().into_inner()
}
fn install(core: &mut Service, bytes: &[u8], extra: Value) -> anyhow::Result<Value> {
    use base64::{engine::general_purpose::STANDARD, Engine};
    let mut p = json!({"package":STANDARD.encode(bytes),"approve":true});
    for (k, v) in extra.as_object().unwrap() {
        p[k] = v.clone();
    }
    core.handle("install", p)
}

#[test]
fn plugin_packages_can_exceed_previous_archive_and_expanded_limits() {
    for (size, compression) in [
        (65 * 1024 * 1024, zip::CompressionMethod::Stored),
        (129 * 1024 * 1024, zip::CompressionMethod::Deflated),
    ] {
        let payload = vec![b'x'; size];
        let manifest = json!({"schemaVersion":1,"apiVersion":1,"id":"test.large","name":"Large","author":"Test","version":"1.0.0","ui":{"quickPage":"page.js"},"files":{"page.js":package::digest(&payload)}});
        let mut archive = ZipWriter::new(Cursor::new(Vec::new()));
        let options = SimpleFileOptions::default().compression_method(compression);
        archive.start_file("manifest.json", options).unwrap();
        archive
            .write_all(&serde_json::to_vec(&manifest).unwrap())
            .unwrap();
        archive.start_file("page.js", options).unwrap();
        archive.write_all(&payload).unwrap();
        drop(payload);
        let bytes = archive.finish().unwrap().into_inner();
        let verified = package::verify(&bytes).unwrap();
        assert_eq!(verified.files["page.js"].len(), size);
        drop(verified);
        if compression == zip::CompressionMethod::Stored {
            let request = json!({"package":STANDARD.encode(&bytes)});
            assert_eq!(package::from_request(&request).unwrap(), bytes);
            drop(request);
            let expected = package::digest(&bytes);
            let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
            let url = format!("http://{}/large.framely", server.server_addr());
            let worker = std::thread::spawn(move || {
                server
                    .recv()
                    .unwrap()
                    .respond(tiny_http::Response::from_data(bytes))
                    .unwrap();
            });
            let downloaded = package::stage_request_proxy(
                &json!({"url":url,"allowHttp":true,"sha256":expected}),
                &Default::default(),
                |_, _| Ok(()),
            )
            .unwrap();
            assert!(std::fs::metadata(&downloaded.path).unwrap().len() > 64 * 1024 * 1024);
            assert_eq!(downloaded.manifest().unwrap().id, "test.large");
            let path = downloaded.path.clone();
            drop(downloaded);
            assert!(!path.exists());
            worker.join().unwrap();
        }
    }
}

#[test]
fn unsigned_package_hashes_reject_unlisted_metadata() {
    let dir = tempfile::tempdir().unwrap();
    let bytes = fixture(dir.path(), "test.plugin", "1.0.0", 1, None);
    let mut zip = ZipArchive::new(Cursor::new(&bytes)).unwrap();
    assert!(zip.by_name("manifest.sig").is_err());
    assert!(zip.by_name("public-key.hex").is_err());
    assert!(package::verify(&bytes).is_ok());
    assert!(package::verify(&rewrite(&bytes, |n, b| if n == "page.js" {
        b.push(b'!')
    }))
    .is_err());
    // Manifest formatting is no longer bound to a developer signature.
    assert!(
        package::verify(&rewrite(&bytes, |n, b| if n == "manifest.json" {
            b.push(b' ')
        }))
        .is_ok()
    );
    assert!(
        package::verify(&rewrite(&bytes, |n, b| if n == "manifest.json" {
            let mut m: Value = serde_json::from_slice(b).unwrap();
            m["files"]["page.js"] = json!("0".repeat(64));
            *b = serde_json::to_vec(&m).unwrap();
        }))
        .is_err()
    );
    let mut old = ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..zip.len() {
        let mut f = zip.by_index(i).unwrap();
        old.start_file(f.name(), SimpleFileOptions::default())
            .unwrap();
        std::io::copy(&mut f, &mut old).unwrap();
    }
    for name in ["public-key.hex", "manifest.sig"] {
        old.start_file(name, SimpleFileOptions::default()).unwrap();
        old.write_all(b"legacy metadata ignored").unwrap();
    }
    assert!(package::verify(&old.finish().unwrap().into_inner()).is_err());
}
#[test]
fn malicious_archive_paths_links_and_duplicates_are_rejected() {
    for path in ["../escape", "/absolute", "a/../../b", "a\\b"] {
        let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
        zip.start_file(path, SimpleFileOptions::default()).unwrap();
        zip.write_all(b"x").unwrap();
        assert!(package::verify(&zip.finish().unwrap().into_inner()).is_err());
    }
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    zip.add_symlink("link", "/etc/shadow", SimpleFileOptions::default())
        .unwrap();
    assert!(package::verify(&zip.finish().unwrap().into_inner()).is_err());
}
#[test]
fn backend_memory_limit_defaults_validation_and_round_trip() {
    let base = json!({"schemaVersion":1,"apiVersion":1,"id":"test.memory","name":"Memory","author":"Dev","version":"1","backend":{"entry":"backend"},"files":{"backend":package::digest(b"x")}});
    let manifest: Manifest = serde_json::from_value(base.clone()).unwrap();
    manifest.validate().unwrap();
    assert_eq!(manifest.memory_limit_mib(), 512);
    // Preserve the old serialized shape when the default is used.
    assert!(serde_json::to_value(&manifest).unwrap()["backend"]
        .get("memoryLimitMiB")
        .is_none());
    for limit in [1, 512, 2048, u32::MAX] {
        let mut value = base.clone();
        value["backend"]["memoryLimitMiB"] = json!(limit);
        let manifest: Manifest = serde_json::from_value(value).unwrap();
        manifest.validate().unwrap();
        let packed = serde_json::to_value(&manifest).unwrap();
        let restored: Manifest = serde_json::from_value(packed).unwrap();
        assert_eq!(restored.memory_limit_mib(), limit);
    }
    for limit in [
        json!(0),
        json!(-1),
        json!(1.5),
        json!("2048"),
        json!(true),
        json!(null),
        json!(4294967296u64),
    ] {
        let mut value = base.clone();
        value["backend"]["memoryLimitMiB"] = limit;
        assert!(serde_json::from_value::<Manifest>(value).map_or(true, |m| m.validate().is_err()));
    }
    let mut ui_only = base;
    ui_only.as_object_mut().unwrap().remove("backend");
    assert_eq!(
        serde_json::from_value::<Manifest>(ui_only)
            .unwrap()
            .memory_limit_mib(),
        512
    );
}

#[test]
fn default_identity_and_invalid_identities() {
    let m = json!({"schemaVersion":1,"apiVersion":1,"id":"test","name":"Test","author":"Dev","version":"1","backend":{"entry":"backend"},"ui":{},"files":{"backend":package::digest(b"x")}});
    let manifest: Manifest = serde_json::from_value(m.clone()).unwrap();
    assert_eq!(manifest.run_as(), RunAs::Steamos);
    manifest.validate().unwrap();
    for user in ["framely", "superuser"] {
        let mut bad = m.clone();
        bad["backend"]["runAs"] = json!(user);
        assert!(serde_json::from_value::<Manifest>(bad).is_err());
    }
    let mut retired = m;
    retired["permissions"] = json!([]);
    assert!(serde_json::from_value::<Manifest>(retired).is_err());
}
#[test]
fn selected_versions_and_reinstallation_preserve_favorites() {
    let dir = tempfile::tempdir().unwrap();
    let state = dir.path().join("state");
    let mut core = accepted_service(&state, 1000).unwrap();
    let a = fixture(dir.path(), "test.plugin", "1.0.0", 1, None);
    install(&mut core, &a, json!({})).unwrap();
    core.handle(
        "plugin.favorite",
        json!({"plugin":"test.plugin","favorite":true}),
    )
    .unwrap();
    let b = fixture(dir.path(), "test.plugin", "2.0.0", 2, None);
    assert!(core.db.plugins["test.plugin"].source.is_none());
    let plan = crate::planner::prepare(&core.db, b.clone(), None, Default::default()).unwrap();
    assert_eq!(plan.plan.items[0].action, "update");
    install(&mut core, &b, json!({})).unwrap();
    assert_eq!(core.db.plugins.len(), 1);
    assert!(core.db.plugins["test.plugin"].favorite);
    install(&mut core, &a, json!({})).unwrap();
    install(&mut core, &a, json!({})).unwrap();
    assert!(core
        .handle("plugin.rollback", json!({"plugin":"test.plugin"}))
        .is_err());
    assert_eq!(core.db.plugins["test.plugin"].manifest.version, "1.0.0");
    let altered = rewrite(&a, |name, data| {
        if name == "manifest.json" {
            let mut m: Value = serde_json::from_slice(data).unwrap();
            m["name"] = json!("Changed release");
            *data = serde_json::to_vec(&m).unwrap();
        }
    });
    assert!(install(&mut core, &altered, json!({}))
        .unwrap_err()
        .to_string()
        .contains("同一插件版本"));
    assert_eq!(core.db.plugins["test.plugin"].manifest.version, "1.0.0");
    assert!(!state
        .join("plugins/test.plugin/versions/1.0.0/public-key.hex")
        .exists());
    let loaded = accepted_service(&state, 1000).unwrap();
    assert!(loaded.db.plugins["test.plugin"].favorite);
}
#[test]
fn privilege_expansion_requires_consent() {
    let dir = tempfile::tempdir().unwrap();
    let mut core = accepted_service(&dir.path().join("state"), 1000).unwrap();
    let a = fixture(dir.path(), "test.plugin", "1", 1, Some("steamos"));
    install(&mut core, &a, json!({})).unwrap();
    let same_user = rewrite(&a, |name, bytes| {
        if name == "manifest.json" {
            let mut manifest: Value = serde_json::from_slice(bytes).unwrap();
            manifest["version"] = json!("1.1");
            *bytes = serde_json::to_vec(&manifest).unwrap();
        }
    });
    let inspection = core
        .handle("inspect", json!({"package":STANDARD.encode(&same_user)}))
        .unwrap();
    assert_eq!(inspection["runAsChanged"], false);
    install(&mut core, &same_user, json!({})).unwrap();
    let b = fixture(dir.path(), "test.plugin", "2", 1, Some("root"));
    assert!(install(&mut core, &b, json!({})).is_err());
    install(&mut core, &b, json!({"approveRunAs":true})).unwrap();
    assert_eq!(
        core.db.plugins["test.plugin"].manifest.run_as(),
        RunAs::Root
    );
}
#[test]
fn notification_limits_and_sources() {
    let n = Notification {
        id: "test".into(),
        title: "t".into(),
        body: "b".into(),
        image: None,
        actions: vec![],
        duration_ms: 8000,
        inbox: false,
    };
    n.validate().unwrap();
    let mut invalid = n.clone();
    invalid.actions = (0..4)
        .map(|i| Action {
            id: i.to_string(),
            label: "x".into(),
            icon: "x".into(),
            close_on_click: true,
            remove_from_inbox_on_click: true,
        })
        .collect();
    assert!(invalid.validate().is_err());
    invalid = n.clone();
    invalid.image = Some("https://example.org/image.png".into());
    invalid.validate().unwrap();
    invalid.image = Some("file:///etc/shadow".into());
    assert!(invalid.validate().is_err());
    let s = Source {
        id: "local".into(),
        name: "Local".into(),
        url: "http://localhost:8080/catalog.json".into(),
        enabled: true,
        allow_http: false,
    };
    assert!(s.validate().is_err());
    let mut allowed = s;
    allowed.allow_http = true;
    allowed.validate().unwrap();
}
#[test]
fn unlisted_entries_and_incompatible_manifests() {
    let dir = tempfile::tempdir().unwrap();
    let bytes = fixture(dir.path(), "test.plugin", "1", 1, None);
    let mut zip = ZipArchive::new(Cursor::new(&bytes)).unwrap();
    let mut entries = BTreeMap::new();
    use std::io::Read;
    for i in 0..zip.len() {
        let mut f = zip.by_index(i).unwrap();
        let mut b = Vec::new();
        f.read_to_end(&mut b).unwrap();
        entries.insert(f.name().to_owned(), b);
    }
    entries.insert("extra".into(), b"unlisted".to_vec());
    let mut out = ZipWriter::new(Cursor::new(Vec::new()));
    for (n, b) in entries {
        out.start_file(n, SimpleFileOptions::default()).unwrap();
        out.write_all(&b).unwrap();
    }
    assert!(package::verify(&out.finish().unwrap().into_inner()).is_err());
    let mut m = package::verify(&bytes).unwrap().manifest;
    m.api_version = 999;
    assert!(m.validate().is_err());
}

#[test]
fn presentation_rehydrates_after_legacy_state_downgrade() {
    let dir = tempfile::tempdir().unwrap();
    let state = dir.path().join("state");
    let mut core = accepted_service(&state, 1000).unwrap();
    let bytes = fixture(dir.path(), "test.meta", "1", 3, None);
    install(&mut core, &bytes, json!({})).unwrap();
    let path = state.join("plugins/test.meta/versions/1/manifest.json");
    let mut m: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    m["details"] = json!("Description restored");
    m["category"] = json!("旧分类");
    m["authorUrl"] = json!("https://example.org/author");
    m["documentationUrl"] = json!("https://example.org/docs#usage");
    m["homepage"] = json!("https://example.org/plugin");
    fs::write(path, serde_json::to_vec(&m).unwrap()).unwrap();
    let loaded = accepted_service(&state, 1000).unwrap();
    assert_eq!(
        loaded.db.plugins["test.meta"].manifest.details,
        "Description restored"
    );
    assert_eq!(
        loaded.db.plugins["test.meta"]
            .manifest
            .documentation_url
            .as_deref(),
        Some("https://example.org/docs#usage")
    );
    assert_eq!(
        loaded.db.plugins["test.meta"]
            .manifest
            .author_url
            .as_deref(),
        Some("https://example.org/author")
    );
    assert_eq!(
        loaded.db.plugins["test.meta"].manifest.homepage.as_deref(),
        Some("https://example.org/plugin")
    );
    assert!(
        serde_json::to_value(&loaded.db.plugins["test.meta"].manifest)
            .unwrap()
            .get("category")
            .is_none()
    );
}

#[test]
fn source_is_strict_and_catalog_allows_additional_metadata() {
    assert!(serde_json::from_value::<UpdateSource>(
        json!({"url":"https://example.org/release.json","publicKey":"old"})
    )
    .is_err());
    assert!(serde_json::from_value::<CatalogEntry>(json!({"id":"test.plugin","name":"Test","version":"1.0.0","description":"Test","author":"Developer","apiVersion":1,"url":"https://example.org/test.framely","sha256":"a".repeat(64),"futureMetadata":{"custom":true}})).is_ok());
}

#[test]
fn update_channels_default_to_stable_and_persist_across_reloads() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("update-source.json"),br#"{"url":"https://github.com/example/framely/releases/latest/download/framely-release.json"}"#).unwrap();
    let mut core = accepted_service(dir.path(), 1000).unwrap();
    assert_eq!(core.db.update_channel, UpdateChannel::Stable);
    assert_eq!(
        core.db
            .update_source
            .as_ref()
            .unwrap()
            .github_repository()
            .as_deref(),
        Some("example/framely")
    );
    core.handle("system.channel.save", json!({"channel":"testing"}))
        .unwrap();
    let core = accepted_service(dir.path(), 1000).unwrap();
    assert_eq!(core.db.update_channel, UpdateChannel::Testing);
    let saved: UpdateSource =
        serde_json::from_slice(&fs::read(dir.path().join("update-source.json")).unwrap()).unwrap();
    assert_eq!(saved.url, core.db.update_source.as_ref().unwrap().url);
    assert!(serde_json::from_value::<UpdateChannel>(json!("unknown")).is_err());
    for url in [
        "https://github.com.evil.test/a/b/releases/latest/download/framely-release.json",
        "https://github.com/a/b/releases/download/v1/framely-release.json",
    ] {
        let source: UpdateSource = serde_json::from_value(json!({"url":url})).unwrap();
        assert!(source.github_repository().is_none());
    }
}

#[test]
fn publication_metadata_survives_pack_and_rejects_invalid_image_urls() {
    let dir = tempfile::tempdir().unwrap();
    fixture(dir.path(), "test.plugin", "1.0.0", 1, None);
    let path = dir.path().join("manifest-1.0.0-1.json");
    let mut manifest: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    manifest["downloadUrl"] =
        json!("https://github.com/example/plugin/releases/download/v1.0.0/plugin.framely");
    manifest["publish"] = json!({"icon":"https://example.org/icon.png","screenshots":["https://example.org/screen.png"]});
    fs::write(&path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    let out = dir.path().join("publish.framely");
    package::pack(&path, &dir.path().join("payload-1.0.0-1"), &out).unwrap();
    let verified = package::verify(&fs::read(&out).unwrap()).unwrap();
    assert_eq!(verified.manifest.publish.unwrap().screenshots.len(), 1);
    manifest["publish"]["icon"] = json!("http://example.org/icon.png");
    fs::write(&path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    assert!(package::pack(&path, &dir.path().join("payload-1.0.0-1"), &out).is_err());
}
#[test]
fn lifecycle_transactions_recovery_and_privilege_arguments() {
    use crate::process::TEST_TOOLS;
    use std::os::unix::fs::PermissionsExt;
    use std::time::{Duration, Instant};
    let temp = tempfile::tempdir().unwrap();
    let tools = temp.path().join("tools");
    fs::create_dir(&tools).unwrap();
    let runner = format!(
        r#"#!/usr/bin/python3
import os,sys,json,pathlib
root=pathlib.Path({root:?});args=sys.argv[1:]
(root/'arguments.jsonl').open('a').write(json.dumps(args)+'\n')
unit=next(a.split('=',1)[1] for a in args if a.startswith('--unit='))
for a in args:
 if a.startswith('--setenv='):
  k,v=a[len('--setenv='):].split('=',1);os.environ[k]=v
os.setsid();(root/(unit+'.pid')).write_text(str(os.getpid()))
command=args[args.index('--')+1:];os.execv(command[0],command)
"#,
        root = temp.path().to_str().unwrap()
    );
    let ctl = format!(
        r#"#!/usr/bin/python3
import sys,os,signal,pathlib
root=pathlib.Path({root:?});args=sys.argv[1:]
if args[0]=='show':
 p=root/(args[1]+'.result');print(p.read_text() if p.exists() else '',end='')
elif args[0]=='stop':
 p=root/(args[1]+'.pid')
 if p.exists():
  try:os.killpg(int(p.read_text()),signal.SIGTERM)
  except ProcessLookupError:pass
  p.unlink(missing_ok=True)
elif args[0]=='reset-failed':(root/(args[1]+'.result')).unlink(missing_ok=True)
"#,
        root = temp.path().to_str().unwrap()
    );
    for (name, source) in [
        ("systemd-run", runner),
        ("systemctl", ctl),
        ("chown", "#!/bin/sh\nexit 0\n".into()),
    ] {
        let p = tools.join(name);
        fs::write(&p, source).unwrap();
        fs::set_permissions(p, fs::Permissions::from_mode(0o755)).unwrap();
    }
    TEST_TOOLS.with(|p| *p.borrow_mut() = Some(tools));
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            TEST_TOOLS.with(|p| *p.borrow_mut() = None);
        }
    }
    let _reset = Reset;
    let backend = r#"#!/usr/bin/python3
import os,sys,json,pathlib,time
trace=pathlib.Path(os.environ['FRAMELY_DATA_DIR'])/'trace.jsonl'
def record(ctx):
 with trace.open('a') as f:f.write(json.dumps(ctx)+'\n')
 if os.environ['FRAMELY_PLUGIN_VERSION']=='badupdate' and ctx['phase']=='onUpdate':raise RuntimeError('migration failed')
 if ctx['phase']=='onUninstall':raise RuntimeError('cleanup failed')
 if os.environ['FRAMELY_PLUGIN_VERSION']=='slowinstall':time.sleep(3)
phase=os.environ.get('FRAMELY_LIFECYCLE')
if phase:
 try:record(json.loads(os.environ['FRAMELY_LIFECYCLE_CONTEXT']))
 except Exception as e:print(e,file=sys.stderr);sys.exit(1)
 sys.exit(0)
for line in sys.stdin:
 request=json.loads(line);method=request['method']
 if method=='crash':os._exit(7)
 if method=='finish':os._exit(0)
 if method=='hang':time.sleep(30)
 if method.startswith('framely.lifecycle.'):
  if os.environ['FRAMELY_PLUGIN_VERSION']=='slowstart' and method.endswith('start'):time.sleep(30)
  record(request['params'])
  if os.environ['FRAMELY_PLUGIN_VERSION'] in ('badstart','badrollback') and method.endswith('start'):
   print(json.dumps({'id':request['id'],'error':'init failed'}),flush=True);continue
 print(json.dumps({'id':request['id'],'result':{'ok':True}}),flush=True)
"#;
    let make = |version: &str, restart: &str| {
        let payload = temp.path().join(format!("fixture-{version}"));
        fs::create_dir_all(&payload).unwrap();
        fs::write(payload.join("backend.py"), backend).unwrap();
        fs::write(payload.join("page.js"), "test").unwrap();
        let hook = json!({"entry":"backend.py"});
        let manifest = json!({"schemaVersion":1,"apiVersion":1,"id":"test.lifecycle","name":"Lifecycle","author":"Test","version":version,"backend":{"entry":"backend.py","runAs":"steamos","autostart":version!="badrollback","restart":restart,"restartLimit":3,"memoryLimitMiB":if version=="1" {2048} else {512}},"lifecycle":{"onInstall":hook,"onUpdate":hook,"onUninstall":hook,"onCrashCleanup":hook,"onStart":true,"onStop":true,"timeoutSeconds":1},"ui":{"quickPage":"page.js"},"files":{}});
        let path = temp.path().join(format!("manifest-{version}.json"));
        fs::write(&path, serde_json::to_vec(&manifest).unwrap()).unwrap();
        let out = temp.path().join(format!("{version}.framely"));
        package::pack(&path, &payload, &out).unwrap();
        fs::read(out).unwrap()
    };
    let mut core =
        accepted_service(&temp.path().join("state"), unsafe { libc::geteuid() }).unwrap();
    install(&mut core, &make("1", "on-failure"), json!({})).unwrap();
    let trace = core.root.join("data/test.lifecycle/steamos/trace.jsonl");
    let phases = || {
        fs::read_to_string(&trace)
            .unwrap()
            .lines()
            .map(|l| {
                serde_json::from_str::<Value>(l).unwrap()["phase"]
                    .as_str()
                    .unwrap()
                    .to_owned()
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(phases(), ["onInstall", "onStart"]);
    core.handle("plugin.restart", json!({"plugin":"test.lifecycle"}))
        .unwrap();
    assert_eq!(&phases()[2..], ["onStop", "onStart"]);
    assert!(core
        .handle(
            "plugin.call",
            json!({"plugin":"test.lifecycle","method":"framely.lifecycle.stop"})
        )
        .is_err());
    assert!(install(&mut core, &make("badupdate", "on-failure"), json!({})).is_err());
    assert_eq!(core.db.plugins["test.lifecycle"].manifest.version, "1");
    assert_eq!(
        fs::read_link(core.root.join("plugins/test.lifecycle/current")).unwrap(),
        Path::new("versions/1")
    );
    assert!(core
        .handle(
            "plugin.call",
            json!({"plugin":"test.lifecycle","method":"ping"})
        )
        .is_ok());
    assert!(install(&mut core, &make("badstart", "on-failure"), json!({})).is_err());
    assert_eq!(core.db.plugins["test.lifecycle"].manifest.version, "1");
    assert!(!core
        .root
        .join("plugins/test.lifecycle/versions/badstart")
        .exists());
    let timeout_start = Instant::now();
    assert!(install(&mut core, &make("slowstart", "on-failure"), json!({})).is_err());
    assert!(timeout_start.elapsed() < Duration::from_secs(4));
    assert_eq!(core.db.plugins["test.lifecycle"].manifest.version, "1");

    assert!(core
        .handle(
            "plugin.call",
            json!({"plugin":"test.lifecycle","method":"crash"})
        )
        .is_err());
    let state = core.handle("status", json!({})).unwrap();
    assert_eq!(state["runtime"]["test.lifecycle"]["phase"], "recovering");
    assert!(phases().iter().any(|s| s == "onCrashCleanup"));
    let events = core.handle("events", json!({})).unwrap();
    assert!(events
        .as_array()
        .unwrap()
        .iter()
        .any(|v| v["kind"] == "plugin.lifecycle"
            && v["state"]["phase"] == "crashed"
            && v["state"]["detail"]["exitCode"] == 7));
    std::thread::sleep(Duration::from_millis(1100));
    core.maintenance();
    assert!(core.handle("status", json!({})).unwrap()["running"]
        .as_array()
        .unwrap()
        .iter()
        .any(|v| v == "test.lifecycle"));
    fs::write(
        temp.path().join("framely-backend-test.lifecycle.result"),
        "Result=oom-kill\nExecMainCode=2\nExecMainStatus=9\n",
    )
    .unwrap();
    assert!(core
        .handle(
            "plugin.call",
            json!({"plugin":"test.lifecycle","method":"crash"})
        )
        .is_err());
    let events = core.handle("events", json!({})).unwrap();
    assert!(events
        .as_array()
        .unwrap()
        .iter()
        .any(|v| v["kind"] == "plugin.lifecycle"
            && v["state"]["phase"] == "crashed"
            && v["state"]["detail"]["oom"] == true
            && v["state"]["detail"]["signal"] == 9));
    install(&mut core, &make("never", "never"), json!({})).unwrap();
    assert!(core
        .handle(
            "plugin.call",
            json!({"plugin":"test.lifecycle","method":"crash"})
        )
        .is_err());
    std::thread::sleep(Duration::from_millis(1100));
    core.maintenance();
    assert!(core.handle("status", json!({})).unwrap()["running"]
        .as_array()
        .unwrap()
        .is_empty());
    install(&mut core, &make("badrollback", "never"), json!({})).unwrap();
    install(&mut core, &make("good", "never"), json!({})).unwrap();
    assert!(install(&mut core, &make("badrollback", "never"), json!({})).is_err());
    assert_eq!(core.db.plugins["test.lifecycle"].manifest.version, "good");
    assert!(core
        .handle(
            "plugin.call",
            json!({"plugin":"test.lifecycle","method":"ping"})
        )
        .is_ok());
    core.handle(
        "agreement.revoke",
        json!({"version":AGREEMENT_VERSION,"approve":true}),
    )
    .unwrap();
    assert!(core.handle("status", json!({})).unwrap()["running"]
        .as_array()
        .unwrap()
        .is_empty());
    let last: Value =
        serde_json::from_str(fs::read_to_string(&trace).unwrap().lines().last().unwrap()).unwrap();
    assert_eq!(last["phase"], "onStop");
    assert_eq!(last["reason"], "agreement-revoked");
    assert!(!core.db.plugins["test.lifecycle"].enabled);
    core.handle(
        "agreement.accept",
        json!({"version":AGREEMENT_VERSION,"userAgreement":true,"privacyStatement":true}),
    )
    .unwrap();
    assert!(core.handle("status", json!({})).unwrap()["running"]
        .as_array()
        .unwrap()
        .is_empty());
    core.handle(
        "plugin.enable",
        json!({"plugin":"test.lifecycle","enabled":true}),
    )
    .unwrap();
    assert!(core
        .handle("plugin.uninstall", json!({"plugin":"test.lifecycle"}))
        .is_err());
    assert!(core.db.plugins.contains_key("test.lifecycle"));
    core.handle(
        "plugin.uninstall",
        json!({"plugin":"test.lifecycle","force":true}),
    )
    .unwrap();
    assert!(core.db.plugins.is_empty());
    assert!(trace.exists());
    let start = Instant::now();
    assert!(install(&mut core, &make("slowinstall", "never"), json!({})).is_err());
    assert!(start.elapsed() < Duration::from_secs(4));
    assert!(!core.db.plugins.contains_key("test.lifecycle"));
    let arguments = fs::read_to_string(temp.path().join("arguments.jsonl")).unwrap();
    assert!(arguments.contains(&format!("User={}", unsafe { libc::geteuid() })));
    assert!(!arguments.contains("PrivateNetwork=yes"));
    assert!(arguments.contains("NoNewPrivileges=yes"));
    let launches: Vec<Vec<String>> = arguments
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    for launch in &launches {
        let expected = if launch
            .iter()
            .any(|arg| arg == "--setenv=FRAMELY_PLUGIN_VERSION=1")
        {
            "--property=MemoryMax=2048M"
        } else {
            "--property=MemoryMax=512M"
        };
        assert!(launch.iter().any(|arg| arg == expected));
    }
    assert!(launches.iter().any(|args| args
        .iter()
        .any(|arg| arg == "--setenv=FRAMELY_LIFECYCLE=onInstall")
        && args.iter().any(|arg| arg == "--property=MemoryMax=2048M")));
    assert!(arguments.contains("RuntimeMaxSec=1"));
    core.shutdown();
}

#[test]
fn lifecycle_manifest_entries_users_and_timeouts_are_validated() {
    let dir = tempfile::tempdir().unwrap();
    let bytes = fixture(dir.path(), "test.lifecycle", "1", 1, Some("steamos"));
    let mut manifest = package::verify(&bytes).unwrap().manifest;
    manifest.lifecycle =
        Some(serde_json::from_value(json!({"onInstall":{"entry":"../escape"}})).unwrap());
    assert!(manifest.validate().is_err());
    manifest.lifecycle =
        Some(serde_json::from_value(json!({"runAs":"root","onStart":true})).unwrap());
    assert!(manifest.validate().is_err());
    manifest.lifecycle = Some(
        serde_json::from_value(json!({"onInstall":{"entry":"backend"},"timeoutSeconds":0}))
            .unwrap(),
    );
    assert!(manifest.validate().is_err());
    manifest.lifecycle = Some(
        serde_json::from_value(json!({"onInstall":{"entry":"backend"},"timeoutSeconds":1}))
            .unwrap(),
    );
    assert!(manifest.validate().is_ok());
    let target = dir.path().join("unpacked");
    let mut verified = package::verify(&bytes).unwrap();
    verified.manifest.backend = None;
    verified.manifest.lifecycle = manifest.lifecycle;
    package::unpack(&verified, &target).unwrap();
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(
        fs::metadata(target.join("backend"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o755
    );
}

#[test]
fn plugin_window_dimensions_validate_and_allow_custom_sizes() {
    let mut m: Manifest = serde_json::from_value(json!({"schemaVersion":1,"apiVersion":1,"id":"test.window","name":"Window","author":"Test","version":"1","ui":{"windows":{"main":{"entry":"page.js","title":"Window","dockIcon":true}}},"files":{"page.js":"0".repeat(64)}})).unwrap();
    m.validate().unwrap();
    let w = m.ui.windows.get_mut("main").unwrap();
    assert_eq!((w.width, w.height), (1600, 900));
    assert_eq!(w.width_meters, Some(3.0));
    w.width = 1920;
    w.height = 1080;
    w.width_meters = Some(1.8);
    m.validate().unwrap();
    let serialized = serde_json::to_value(&m).unwrap();
    assert_eq!(serialized["ui"]["windows"]["main"]["width"], 1920);
    m.ui.windows.get_mut("main").unwrap().width = 2561;
    assert!(m.validate().is_err());
    m.ui.windows.get_mut("main").unwrap().width = 1600;
    m.ui.windows.get_mut("main").unwrap().width_meters = Some(f32::NAN);
    assert!(m.validate().is_err());
}
fn relation_package(dir: &Path, id: &str, version: &str, extra: Value) -> Vec<u8> {
    let payload = dir.join(format!("payload-{id}-{version}"));
    fs::create_dir_all(&payload).unwrap();
    fs::write(payload.join("page.js"), "test").unwrap();
    fs::write(payload.join("fail.sh"), "#!/bin/sh\nexit 1\n").unwrap();
    let mut m = json!({"schemaVersion":1,"apiVersion":1,"id":id,"name":id,"author":"test","version":version,"ui":{"quickPage":"page.js"},"files":{}});
    for (key, value) in extra.as_object().unwrap() {
        m[key] = value.clone();
    }
    let path = dir.join(format!("{id}-{version}.json"));
    fs::write(&path, serde_json::to_vec(&m).unwrap()).unwrap();
    let out = dir.join(format!("{id}-{version}.framely"));
    package::pack(&path, &payload, &out).unwrap();
    fs::read(out).unwrap()
}
fn apply_prepared(core: &mut Service, prepared: crate::planner::Prepared) -> anyhow::Result<Value> {
    core.handle("install.batch",json!({"approve":true,"approveRunAs":true,"plan":prepared.plan,"packages":prepared.packages.iter().map(|(bytes,source)|{let mut request=bytes.request();request["source"]=json!(source);request}).collect::<Vec<_>>()}))
}
#[test]
fn dependency_install_conflicts_reverse_constraints_and_removal() {
    let tmp = tempfile::tempdir().unwrap();
    let mut core = accepted_service(&tmp.path().join("state"), unsafe { libc::geteuid() }).unwrap();
    let base = relation_package(tmp.path(), "test.base", "1.0.0", json!({}));
    install(&mut core, &base, json!({})).unwrap();
    let child = relation_package(
        tmp.path(),
        "test.child",
        "1.0.0",
        json!({"dependencies":{"test.base":"^1.0.0"}}),
    );
    install(&mut core, &child, json!({})).unwrap();
    let bad = relation_package(tmp.path(), "test.base", "2.0.0", json!({}));
    assert!(install(&mut core, &bad, json!({})).is_err());
    assert_eq!(core.db.plugins["test.base"].manifest.version, "1.0.0");
    let other = relation_package(
        tmp.path(),
        "test.other",
        "1.0.0",
        json!({"exclusiveResources":["test.resource"],"conflicts":{"test.base":"*"}}),
    );
    assert!(install(&mut core, &other, json!({})).is_err());
    let prepared = crate::planner::prepare(&core.db, other, None, Default::default()).unwrap();
    assert_eq!(prepared.plan.disable, ["test.base", "test.child"]);
    apply_prepared(&mut core, prepared).unwrap();
    assert!(!core.db.plugins["test.child"].enabled);
    let plan = core
        .handle("plugin.enable.preview", json!({"plugin":"test.child"}))
        .unwrap();
    assert_eq!(plan["disable"], json!(["test.other"]));
    assert!(core
        .handle(
            "plugin.enable",
            json!({"plugin":"test.child","enabled":true})
        )
        .is_err());
    core.handle("plugin.enable",json!({"plugin":"test.child","enabled":true,"approve":true,"fingerprint":plan["fingerprint"]})).unwrap();
    assert!(core
        .handle("plugin.uninstall", json!({"plugin":"test.base"}))
        .is_err());
    let preview = core
        .handle("plugin.disable.preview", json!({"plugin":"test.base"}))
        .unwrap();
    core.handle(
        "plugin.uninstall",
        json!({"plugin":"test.base","approveDependents":true,"fingerprint":preview["fingerprint"]}),
    )
    .unwrap();
    assert!(!core.db.plugins["test.child"].enabled);
    let unrelated = relation_package(tmp.path(), "test.unrelated", "1.0.0", json!({}));
    install(&mut core, &unrelated, json!({})).unwrap();
}
#[test]
fn same_source_dependency_downloads_and_stale_plan_are_verified() {
    use tiny_http::{Response, Server};
    let tmp = tempfile::tempdir().unwrap();
    let mut core = accepted_service(&tmp.path().join("state"), unsafe { libc::geteuid() }).unwrap();
    let base = relation_package(tmp.path(), "test.base", "1.0.0", json!({}));
    let child = relation_package(
        tmp.path(),
        "test.child",
        "1.0.0",
        json!({"dependencies":{"test.base":"^1.0.0"}}),
    );
    let server = Server::http("127.0.0.1:0").unwrap();
    let url = format!("http://{}", server.server_addr());
    let latest = json!({"id":"test.base","name":"test.base","version":"2.0.0","author":"test","description":"","apiVersion":1,"url":format!("{url}/unused.framely"),"sha256":package::digest(&base)});
    let old = json!({"id":"test.base","name":"test.base","version":"1.0.0","author":"test","description":"","apiVersion":1,"url":format!("{url}/base.framely"),"sha256":package::digest(&base)});
    let catalog = json!({"schemaVersion":1,"name":"test","plugins":[latest]});
    let history = json!({"schemaVersion":1,"id":"test.base","versions":[old]});
    let worker = std::thread::spawn(move || {
        for _ in 0..3 {
            let request = server.recv().unwrap();
            let response = if request.url() == "/catalog.json" {
                Response::from_data(serde_json::to_vec(&catalog).unwrap())
            } else if request.url() == "/plugins/test.base/versions.json" {
                Response::from_data(serde_json::to_vec(&history).unwrap())
            } else {
                Response::from_data(base.clone())
            };
            request.respond(response).unwrap();
        }
    });
    core.handle("sources.save",json!({"sources":[{"id":"test.source","name":"test","url":format!("{url}/catalog.json"),"allowHttp":true}]})).unwrap();
    let prepared = crate::planner::prepare(
        &core.db,
        child.clone(),
        Some("test.source".into()),
        Default::default(),
    )
    .unwrap();
    worker.join().unwrap();
    assert_eq!(
        prepared
            .plan
            .items
            .iter()
            .map(|p| p.manifest.id.as_str())
            .collect::<Vec<_>>(),
        ["test.base", "test.child"]
    );
    apply_prepared(&mut core, prepared).unwrap();
    let next = relation_package(
        tmp.path(),
        "test.child",
        "1.1.0",
        json!({"dependencies":{"test.base":"^1.0.0"}}),
    );
    let stale = crate::planner::prepare(
        &core.db,
        next,
        Some("test.source".into()),
        Default::default(),
    )
    .unwrap();
    core.handle(
        "plugin.favorite",
        json!({"plugin":"test.child","favorite":true}),
    )
    .unwrap();
    assert!(apply_prepared(&mut core, stale).is_err());
    assert_eq!(core.db.plugins["test.child"].manifest.version, "1.0.0");
}
fn fake_backend_tools(temp: &Path) -> impl Drop {
    use std::os::unix::fs::PermissionsExt;
    let tools = temp.join("tools");
    fs::create_dir_all(&tools).unwrap();
    let runner = format!(
        r#"#!/usr/bin/python3
import os,sys,pathlib
root=pathlib.Path({root:?});args=sys.argv[1:];unit=next(a.split('=',1)[1] for a in args if a.startswith('--unit='))
for arg in args:
 if arg.startswith('--setenv='):
  key,value=arg[9:].split('=',1);os.environ[key]=value
os.setsid();(root/(unit+'.pid')).write_text(str(os.getpid()))
command=args[args.index('--')+1:];os.execv(command[0],command)
"#,
        root = temp.to_str().unwrap()
    );
    let ctl = format!(
        r#"#!/usr/bin/python3
import os,sys,pathlib,signal
root=pathlib.Path({root:?});args=sys.argv[1:]
if args[0]=='stop':
 p=root/(args[1]+'.pid')
 if p.exists():
  try:os.killpg(int(p.read_text()),signal.SIGTERM)
  except ProcessLookupError:pass
  p.unlink(missing_ok=True)
"#,
        root = temp.to_str().unwrap()
    );
    for (name, source) in [
        ("systemd-run", runner),
        ("systemctl", ctl),
        ("chown", "#!/bin/sh\nexit 0\n".into()),
    ] {
        let path = tools.join(name);
        fs::write(&path, source).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    crate::process::TEST_TOOLS.with(|p| *p.borrow_mut() = Some(tools));
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            crate::process::TEST_TOOLS.with(|p| *p.borrow_mut() = None);
        }
    }
    Reset
}
#[test]
fn dependency_crash_suspends_and_recovers_in_start_stop_order() {
    let tmp = tempfile::tempdir().unwrap();
    let _tools = fake_backend_tools(tmp.path());
    let mut core = accepted_service(&tmp.path().join("state"), unsafe { libc::geteuid() }).unwrap();
    let trace = tmp.path().join("trace");
    let backend = format!(
        r#"#!/usr/bin/python3
import os,sys,json,pathlib
for line in sys.stdin:
 request=json.loads(line)
 if request['method']=='crash':os._exit(7)
 if request['method'].startswith('framely.lifecycle.'):
  with pathlib.Path({trace:?}).open('a') as log:log.write(os.environ['FRAMELY_PLUGIN_ID']+':'+request['method'].split('.')[-1]+'\n')
 print(json.dumps({{'id':request['id'],'result':True}}),flush=True)
"#,
        trace = trace.to_str().unwrap()
    );
    for (id, deps) in [
        ("test.base", json!({})),
        ("test.child", json!({"test.base":"^1.0.0"})),
    ] {
        let payload = tmp.path().join(id);
        fs::create_dir(&payload).unwrap();
        fs::write(payload.join("backend.py"), &backend).unwrap();
        let manifest = json!({"schemaVersion":1,"apiVersion":1,"id":id,"name":id,"author":"test","version":"1.0.0","dependencies":deps,"backend":{"entry":"backend.py","runAs":"steamos"},"lifecycle":{"onStart":true,"onStop":true},"files":{}});
        let path = tmp.path().join(format!("{id}.json"));
        fs::write(&path, serde_json::to_vec(&manifest).unwrap()).unwrap();
        let out = tmp.path().join(format!("{id}.framely"));
        package::pack(&path, &payload, &out).unwrap();
        install(&mut core, &fs::read(out).unwrap(), json!({})).unwrap();
    }
    core.handle("plugin.open", json!({"plugin":"test.child"}))
        .unwrap();
    assert_eq!(
        fs::read_to_string(&trace).unwrap(),
        "test.base:start\ntest.child:start\n"
    );
    assert!(core
        .handle(
            "plugin.call",
            json!({"plugin":"test.base","method":"crash"})
        )
        .is_err());
    let status = core.handle("status", json!({})).unwrap();
    assert_eq!(
        status["runtime"]["test.child"]["phase"],
        "waiting-dependency"
    );
    assert!(core.db.plugins["test.child"].enabled);
    assert!(core.db.plugins["test.child"].error.is_none());
    std::thread::sleep(std::time::Duration::from_millis(1100));
    core.maintenance();
    assert_eq!(
        fs::read_to_string(&trace).unwrap(),
        "test.base:start\ntest.child:start\ntest.child:stop\ntest.base:start\ntest.child:start\n"
    );
    core.shutdown();
}
#[test]
fn batch_hook_failure_and_interrupted_transaction_restore_state() {
    let tmp = tempfile::tempdir().unwrap();
    let _tools = fake_backend_tools(tmp.path());
    let mut core = accepted_service(&tmp.path().join("state"), unsafe { libc::geteuid() }).unwrap();
    let original_package = relation_package(tmp.path(), "test.original", "1.0.0", json!({}));
    install(&mut core, &original_package, json!({})).unwrap();
    let original = core.db.clone();
    let first = relation_package(tmp.path(), "test.first", "1.0.0", json!({}));
    let bad = relation_package(
        tmp.path(),
        "test.bad",
        "1.0.0",
        json!({"lifecycle":{"runAs":"steamos","onInstall":{"entry":"fail.sh"}}}),
    );
    let first_manifest = package::verify(&first).unwrap().manifest;
    let bad_manifest = package::verify(&bad).unwrap().manifest;
    let plan = crate::planner::Plan {
        fingerprint: crate::relations::fingerprint(&core.db).unwrap(),
        root: "test.bad".into(),
        items: vec![
            crate::planner::Item {
                manifest: first_manifest,
                source: None,
                action: "install".into(),
                run_as_changed: false,
            },
            crate::planner::Item {
                manifest: bad_manifest,
                source: None,
                action: "install".into(),
                run_as_changed: false,
            },
        ],
        add_sources: vec![],
        enable: vec!["test.bad".into()],
        disable: vec![],
        affected: vec!["test.first".into(), "test.bad".into()],
    };
    let error = apply_prepared(
        &mut core,
        crate::planner::Prepared {
            plan,
            packages: vec![
                (package::Staged::from_bytes(&first).unwrap(), None),
                (package::Staged::from_bytes(&bad).unwrap(), None),
            ],
        },
    )
    .unwrap_err();
    let message = format!("{error:#}");
    assert!(message.contains("批量安装失败，已恢复原版本和启用状态"));
    assert!(message.contains("安装插件 test.bad 失败"));
    assert!(message.contains("Lifecycle"));
    assert_eq!(
        serde_json::to_value(&core.db).unwrap(),
        serde_json::to_value(&original).unwrap()
    );
    assert!(!core.root.join("plugins/test.first").exists());
    assert!(!core.root.join("install-transaction.json").exists());
    install(&mut core, &first, json!({})).unwrap();
    fs::write(
        core.root.join("install-transaction.json"),
        serde_json::to_vec(&json!({"database":original,"targets":["test.first"],"running":[]}))
            .unwrap(),
    )
    .unwrap();
    let restored = accepted_service(&core.root, core.manager).unwrap();
    assert!(!restored.db.plugins.contains_key("test.first"));
    assert!(!restored.root.join("plugins/test.first").exists());
}
#[test]
fn cross_source_resolution_requires_choice_and_uses_the_chosen_package() {
    use tiny_http::{Response, Server};
    let tmp = tempfile::tempdir().unwrap();
    let mut core = accepted_service(&tmp.path().join("state"), unsafe { libc::geteuid() }).unwrap();
    let base = relation_package(tmp.path(), "test.base", "1.0.0", json!({}));
    let child = relation_package(
        tmp.path(),
        "test.child",
        "1.0.0",
        json!({"dependencies":{"test.base":"^1.0.0"}}),
    );
    let server = Server::http("127.0.0.1:0").unwrap();
    let url = format!("http://{}", server.server_addr());
    let catalog = json!({"schemaVersion":1,"name":"test","plugins":[{"id":"test.base","name":"test.base","version":"1.0.0","author":"test","description":"","apiVersion":1,"url":format!("{url}/base.framely"),"sha256":package::digest(&base)}]});
    let worker = std::thread::spawn(move || {
        for _ in 0..3 {
            let request = server
                .recv_timeout(std::time::Duration::from_secs(5))
                .unwrap()
                .expect("resolver request missing");
            let response = if request.url() == "/catalog.json" {
                Response::from_data(serde_json::to_vec(&catalog).unwrap())
            } else {
                Response::from_data(base.clone())
            };
            request.respond(response).unwrap();
        }
    });
    let source_url = format!("{url}/catalog.json");
    core.handle(
        "sources.save",
        json!({"sources":[{"id":"other-source","name":"other","url":source_url,"allowHttp":true}]}),
    )
    .unwrap();
    let error = crate::planner::prepare(&core.db, child.clone(), None, Default::default())
        .err()
        .unwrap();
    let choice = error
        .downcast_ref::<crate::planner::ChoiceRequired>()
        .unwrap();
    assert_eq!(choice.dependency, "test.base");
    assert_eq!(choice.candidates.len(), 1);
    let prepared = crate::planner::prepare(
        &core.db,
        child,
        None,
        std::collections::BTreeMap::from([("test.base".into(), source_url)]),
    )
    .unwrap();
    apply_prepared(&mut core, prepared).unwrap();
    assert_eq!(
        core.db.plugins["test.base"].source.as_deref(),
        Some("other-source")
    );
    worker.join().unwrap();
}

#[test]
fn plugin_web_links_survive_pack_and_catalog_and_reject_unsafe_schemes() {
    let dir = tempfile::tempdir().unwrap();
    fixture(dir.path(), "test.links", "1", 1, None);
    let path = dir.path().join("manifest-1-1.json");
    let mut value: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    for key in ["authorUrl", "documentationUrl", "homepage"] {
        value[key] = json!("https://example.org/plugin#usage");
    }
    fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
    let out = dir.path().join("links.framely");
    package::pack(&path, &dir.path().join("payload-1-1"), &out).unwrap();
    let manifest = package::verify(&fs::read(out).unwrap()).unwrap().manifest;
    let mut catalog = serde_json::to_value(&manifest).unwrap();
    // Catalog and manifest use the same optional field names.
    catalog = json!({"id":"test.links","name":"Links","version":"1","description":"Links","author":"Test","apiVersion":1,"url":"https://example.org/plugin.framely","sha256":"a".repeat(64),"authorUrl":catalog["authorUrl"],"documentationUrl":catalog["documentationUrl"],"homepage":catalog["homepage"]});
    let entry: CatalogEntry = serde_json::from_value(catalog.clone()).unwrap();
    entry.validate(false).unwrap();
    assert_eq!(entry.author_url, manifest.author_url);
    assert_eq!(entry.documentation_url, manifest.documentation_url);
    assert_eq!(entry.homepage, manifest.homepage);
    for key in ["authorUrl", "documentationUrl", "homepage"] {
        for url in [
            "javascript:alert(1)",
            "file:///tmp/test",
            "https://user:password@example.org",
            "https://",
            "https://example.org/a b",
        ] {
            value[key] = json!(url);
            assert!(serde_json::from_value::<Manifest>(value.clone())
                .unwrap()
                .validate()
                .is_err());
            catalog[key] = json!(url);
            assert!(serde_json::from_value::<CatalogEntry>(catalog.clone())
                .unwrap()
                .validate(false)
                .is_err());
        }
        value[key] = json!("https://example.org/plugin#usage");
        catalog[key] = value[key].clone();
    }
}

#[test]
fn plugin_web_links_launch_browser_with_a_single_argument() {
    use crate::process::{open_web_link, TEST_TOOLS};
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let opener = dir.path().join("xdg-open");
    fs::write(
        &opener,
        "#!/bin/sh\n[ \"$#\" = 1 ] && [ \"$1\" = 'https://example.org/docs#usage' ]\n",
    )
    .unwrap();
    fs::set_permissions(&opener, fs::Permissions::from_mode(0o755)).unwrap();
    TEST_TOOLS.with(|p| *p.borrow_mut() = Some(dir.path().to_owned()));
    assert!(open_web_link("https://example.org/docs#usage").is_ok());
    assert!(open_web_link("javascript:alert(1)").is_err());
    assert!(open_web_link("https://example.org/other").is_err());
    TEST_TOOLS.with(|p| *p.borrow_mut() = None);
}

#[test]
fn network_panel_defaults_persist_and_validate() {
    let root = tempfile::tempdir().unwrap();
    let mut core = accepted_service(root.path(), 1000).unwrap();
    assert!(core.db.network_panel.enabled);
    assert_eq!(core.db.network_panel.port, 15915);
    assert!(!core.db.network_panel.password_enabled);
    assert!(core
        .handle(
            "network.save",
            json!({"enabled":true,"port":15915,"passwordEnabled":true})
        )
        .is_err());
    assert!(core
        .handle("network.save", json!({"enabled":false,"port":80}))
        .is_err());
    core.handle("network.save", json!({"enabled":false,"port":16000}))
        .unwrap();
    let restored = accepted_service(root.path(), 1000).unwrap();
    assert!(!restored.db.network_panel.enabled);
    assert_eq!(restored.db.network_panel.port, 16000);
}

#[test]
fn proxy_settings_persist_and_reject_invalid_values() {
    let root = tempfile::tempdir().unwrap();
    let mut core = accepted_service(root.path(), 1000).unwrap();
    assert_eq!(core.db.proxy, crate::model::ProxySettings::default());
    core.handle("proxy.save",json!({"httpEnabled":true,"githubEnabled":true,"http":" http://127.0.0.1:7890 ","github":"https://gh-proxy.com/"})).unwrap();
    let loaded = accepted_service(root.path(), 1000).unwrap();
    assert_eq!(loaded.db.proxy.http, "http://127.0.0.1:7890");
    assert_eq!(loaded.db.proxy.github, "https://gh-proxy.com");
    assert!(loaded.db.proxy.http_enabled && loaded.db.proxy.github_enabled);
    assert!(core
        .handle(
            "proxy.save",
            json!({"http":"file:///tmp/proxy","github":""})
        )
        .is_err());
    assert_eq!(core.db.proxy.http, "http://127.0.0.1:7890");
    core.handle("proxy.save",json!({"httpEnabled":false,"githubEnabled":false,"http":"http://127.0.0.1:7890","github":"https://gh-proxy.com"})).unwrap();
    let disabled = accepted_service(root.path(), 1000).unwrap();
    assert!(!disabled.db.proxy.http_enabled && !disabled.db.proxy.github_enabled);
    assert_eq!(disabled.db.proxy.github, "https://gh-proxy.com");
    assert!(core
        .handle(
            "proxy.save",
            json!({"httpEnabled":true,"http":"","github":""})
        )
        .is_err());
    core.handle("proxy.save", json!({"http":"","github":""}))
        .unwrap();
    assert_eq!(core.db.proxy, crate::model::ProxySettings::default());
}

#[test]
fn first_launch_agreement_gates_operations_and_persists_consent() {
    let root = tempfile::tempdir().unwrap();
    let mut core = Service::load(root.path(), 1000).unwrap();
    assert_eq!(
        core.handle("agreement.status", json!({})).unwrap()["accepted"],
        false
    );
    core.autostart();
    core.maintenance();
    assert!(core.handle("status", json!({})).unwrap()["running"]
        .as_array()
        .unwrap()
        .is_empty());
    for method in [
        "install",
        "plugin.open",
        "sources.save",
        "proxy.save",
        "system.check.start",
        "network.save",
    ] {
        assert!(core
            .handle(method, json!({}))
            .unwrap_err()
            .to_string()
            .contains("请先同意"));
    }
    for params in [
        json!({"version":AGREEMENT_VERSION,"userAgreement":true}),
        json!({"version":"old","userAgreement":true,"privacyStatement":true}),
        json!({"version":AGREEMENT_VERSION,"userAgreement":false,"privacyStatement":true}),
    ] {
        assert!(core.handle("agreement.accept", params).is_err());
    }
    core.handle(
        "agreement.accept",
        json!({"version":AGREEMENT_VERSION,"userAgreement":true,"privacyStatement":true}),
    )
    .unwrap();
    let time = core.db.agreement_acceptance.as_ref().unwrap().accepted_at;
    assert!(time > 0);
    core.handle(
        "agreement.accept",
        json!({"version":AGREEMENT_VERSION,"userAgreement":true,"privacyStatement":true}),
    )
    .unwrap();
    assert_eq!(
        core.db.agreement_acceptance.as_ref().unwrap().accepted_at,
        time
    );
    let mut restored = Service::load(root.path(), 1000).unwrap();
    assert_eq!(
        restored.handle("agreement.status", json!({})).unwrap()["accepted"],
        true
    );
    restored
        .handle("proxy.save", json!({"http":"","github":""}))
        .unwrap();
    restored.db.agreement_acceptance.as_mut().unwrap().version = "old".into();
    assert_eq!(
        restored.handle("status", json!({})).unwrap()["agreement"]["accepted"],
        false
    );
    assert!(restored
        .handle("proxy.save", json!({"http":"","github":""}))
        .is_err());
}

#[test]
fn agreement_revocation_requires_confirmation_and_disables_all_plugins_persistently() {
    let root = tempfile::tempdir().unwrap();
    let mut core = accepted_service(root.path(), 1000).unwrap();
    for id in ["test.agreement-a", "test.agreement-b"] {
        let bytes = fixture(root.path(), id, "1", 1, None);
        install(&mut core, &bytes, json!({})).unwrap();
    }
    assert!(core.db.plugins.values().all(|p| p.enabled));
    assert!(core
        .handle("agreement.revoke", json!({"version":AGREEMENT_VERSION}))
        .is_err());
    assert!(core.db.agreement_acceptance.is_some());
    assert!(core.db.plugins.values().all(|p| p.enabled));
    fs::create_dir(root.path().join("state.json.tmp")).unwrap();
    assert!(core
        .handle(
            "agreement.revoke",
            json!({"version":AGREEMENT_VERSION,"approve":true})
        )
        .is_err());
    assert!(core.db.agreement_acceptance.is_some());
    assert!(core.db.plugins.values().all(|p| p.enabled));
    fs::remove_dir(root.path().join("state.json.tmp")).unwrap();
    core.handle(
        "agreement.revoke",
        json!({"version":AGREEMENT_VERSION,"approve":true}),
    )
    .unwrap();
    assert!(core.db.agreement_acceptance.is_none());
    assert!(core.db.plugins.values().all(|p| !p.enabled));
    let status = core.handle("status", json!({})).unwrap();
    assert_eq!(status["agreement"]["accepted"], false);
    assert!(status["running"].as_array().unwrap().is_empty());
    assert!(core
        .handle(
            "plugin.enable",
            json!({"plugin":"test.agreement-a","enabled":true})
        )
        .is_err());
    let events = core.handle("events", json!({})).unwrap();
    assert!(events
        .as_array()
        .unwrap()
        .iter()
        .any(|e| e["kind"] == "plugin.disabled" && e["plugin"] == "test.agreement-a"));
    let mut restored = Service::load(root.path(), 1000).unwrap();
    assert!(restored.db.agreement_acceptance.is_none());
    assert!(restored.db.plugins.values().all(|p| !p.enabled));
    restored
        .handle(
            "agreement.accept",
            json!({"version":AGREEMENT_VERSION,"userAgreement":true,"privacyStatement":true}),
        )
        .unwrap();
    assert!(restored.db.plugins.values().all(|p| !p.enabled));
}

#[test]
fn localization_defaults_install_fallback_and_persistence() {
    use crate::localization::{valid_locale, LanguagePack};
    let root = tempfile::tempdir().unwrap();
    let mut core = Service::load(root.path(), 1000).unwrap();
    assert_eq!(core.db.language, "auto");
    assert!(core.handle("language.list", json!({})).is_ok());
    core.handle("language.save", json!({"language":"en-US"}))
        .unwrap();
    assert_eq!(
        Service::load(root.path(), 1000).unwrap().db.language,
        "en-US"
    );
    assert!(core.db.agreement_acceptance.is_none());
    assert!(core
        .handle("language.save", json!({"language":"../en-US"}))
        .is_err());
    assert!(core.handle("language.install", json!({})).is_err());
    core.handle(
        "agreement.accept",
        json!({"version":AGREEMENT_VERSION,"userAgreement":true,"privacyStatement":true}),
    )
    .unwrap();
    let pack = json!({"schemaVersion":1,"locale":"fr-FR","name":"Français","messages":{"语言":"Langue","启用 {0}":"Activer {0}"}});
    core.handle("language.install", json!({"pack":pack}))
        .unwrap();
    let listed = core.handle("language.list", json!({})).unwrap();
    assert_eq!(listed["packs"][0]["locale"], "fr-FR");
    core.handle("language.save", json!({"language":"fr-FR"}))
        .unwrap();
    assert_eq!(
        Service::load(root.path(), 1000).unwrap().db.language,
        "fr-FR"
    );
    for code in ["../en-US", "en/US", "auto.json", ""] {
        assert!(!valid_locale(code));
        assert!(core
            .handle("language.save", json!({"language":code}))
            .is_err());
    }
    assert!(core
        .handle("language.save", json!({"language":"de-DE"}))
        .is_err());
    let mut bad: LanguagePack = serde_json::from_value(pack.clone()).unwrap();
    bad.messages.insert("启用 {0}".into(), "Activer".into());
    assert!(bad.validate().is_err());
    fs::write(root.path().join("locales/broken.json"), "not json").unwrap();
    let listed = core.handle("language.list", json!({})).unwrap();
    assert_eq!(listed["invalidFiles"], json!(["broken.json"]));
    assert_eq!(listed["packs"].as_array().unwrap().len(), 1);
    fs::create_dir(root.path().join("state.json.tmp")).unwrap();
    assert!(core
        .handle("language.save", json!({"language":"en-US"}))
        .is_err());
    assert_eq!(core.db.language, "fr-FR");
    let old: crate::model::Database =
        serde_json::from_value(json!({"plugins":{},"sources":[],"safeMode":false})).unwrap();
    assert_eq!(old.language, "auto");
}

#[test]
fn hook_failure_does_not_stop_plugin_with_hook_suffix() {
    let tmp = tempfile::tempdir().unwrap();
    let _tools = fake_backend_tools(tmp.path());
    let runner = tmp.path().join("tools/systemd-run");
    let script = fs::read_to_string(&runner).unwrap();
    fs::write(
        &runner,
        script.replace(
            "for arg in args:",
            "if (root/(unit+'.pid')).exists(): sys.exit(1)\nfor arg in args:",
        ),
    )
    .unwrap();
    let backend = b"#!/bin/sh\nsleep 60\n".to_vec();
    let m:Manifest=serde_json::from_value(json!({"schemaVersion":1,"apiVersion":1,"id":"review.foo-hook","name":"Victim","author":"Test","version":"1.0.0","backend":{"entry":"backend","runAs":"steamos"},"ui":{"quickPage":"page.js"},"files":{"backend":package::digest(&backend),"page.js":package::digest(b"test")}})).unwrap();
    let payload = tmp.path().join("victim");
    package::unpack(
        &package::Verified {
            manifest: m.clone(),
            files: BTreeMap::from([
                ("backend".into(), backend),
                ("page.js".into(), b"test".to_vec()),
            ]),
        },
        &payload,
    )
    .unwrap();
    let mut victim = crate::process::Running::start(
        &m,
        &payload,
        unsafe { libc::geteuid() },
        Default::default(),
        &tmp.path().join("logs"),
    )
    .unwrap();
    let pid = tmp.path().join("framely-backend-review.foo-hook.pid");
    for _ in 0..100 {
        if pid.exists() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(pid.exists());
    assert!(!victim.exited());
    let bytes = relation_package(
        tmp.path(),
        "review.foo",
        "1.0.0",
        json!({"lifecycle":{"runAs":"steamos","onInstall":{"entry":"fail.sh"}}}),
    );
    let verified = package::verify(&bytes).unwrap();
    let hook_payload = tmp.path().join("hook");
    package::unpack(&verified, &hook_payload).unwrap();
    let result = crate::process::hook(
        &verified.manifest,
        verified
            .manifest
            .lifecycle
            .as_ref()
            .unwrap()
            .on_install
            .as_ref()
            .unwrap(),
        &hook_payload,
        unsafe { libc::geteuid() },
        &tmp.path().join("logs"),
        json!({"phase":"onInstall"}),
        std::time::Duration::from_secs(1),
    );
    assert!(result.is_err());
    for _ in 0..100 {
        if !victim.exited() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(!victim.exited());
    println!("Hook and backend use disjoint service names");
}

#[test]
fn manager_uninstall_disables_all_plugins_and_removes_packages_before_body() {
    let root = tempfile::tempdir().unwrap();
    let mut core = accepted_service(root.path(), 1000).unwrap();
    let a = fixture(root.path(), "test.first", "1.0.0", 70, None);
    let b = fixture(root.path(), "test.second", "1.0.0", 71, None);
    install(&mut core, &a, json!({})).unwrap();
    install(&mut core, &b, json!({})).unwrap();
    fs::create_dir_all(root.path().join("data/test.first")).unwrap();
    fs::write(root.path().join("data/test.first/saved"), b"retain").unwrap();
    fs::write(root.path().join("manager-marker"), b"body").unwrap();
    // Uninstall must work even when the user has not accepted current agreements.
    core.db.agreement_acceptance = None;
    assert!(core.handle("system.uninstall.prepare", json!({})).is_err());
    let result = core
        .handle("system.uninstall.prepare", json!({"approve":true}))
        .unwrap();
    assert_eq!(result["uninstalled"].as_array().unwrap().len(), 2);
    assert!(core.db.plugins.is_empty());
    assert!(core.db.safe_mode);
    assert!(!root.path().join("plugins/test.first").exists());
    assert!(!root.path().join("plugins/test.second").exists());
    assert!(root.path().join("data/test.first/saved").exists());
    assert!(root.path().join("manager-marker").exists());
    let reloaded = Service::load(root.path(), 1000).unwrap();
    assert!(reloaded.db.plugins.is_empty());
    core.handle("system.uninstall.prepare", json!({"approve":true}))
        .unwrap();
}

#[test]
fn manager_uninstall_failure_keeps_plugin_and_persists_disabling() {
    let root = tempfile::tempdir().unwrap();
    let mut core = accepted_service(root.path(), 1000).unwrap();
    let bytes = fixture(root.path(), "test.retained", "1.0.0", 72, None);
    install(&mut core, &bytes, json!({})).unwrap();
    let _tools = fake_backend_tools(root.path());
    core.db
        .plugins
        .get_mut("test.retained")
        .unwrap()
        .manifest
        .lifecycle = Some(
        serde_json::from_value(
            json!({"runAs":"steamos","onUninstall":{"entry":"missing-hook.sh"},"timeoutSeconds":1}),
        )
        .unwrap(),
    );
    assert!(core
        .handle("system.uninstall.prepare", json!({"approve":true}))
        .is_err());
    assert!(core.db.plugins.contains_key("test.retained"));
    assert!(!core.db.plugins["test.retained"].enabled);
    assert!(core.db.safe_mode);
    assert!(root.path().join("plugins/test.retained").exists());
    let reloaded = Service::load(root.path(), 1000).unwrap();
    assert!(!reloaded.db.plugins["test.retained"].enabled);
}

#[test]
fn disk_packages_stream_validation_and_review_hash() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    let bytes = fixture(root.path(), "test.disk", "1.0.0", 51, None);
    let staged = package::Staged::from_bytes(&bytes).unwrap();
    let path = staged.path.clone();
    assert!(path.starts_with("/tmp"));
    assert_eq!(staged.manifest().unwrap().id, "test.disk");
    // A different non-root session UID must still be rejected.
    let owner = unsafe { libc::geteuid() };
    if owner != 0 {
        assert!(package::open_staged(&staged.request(), owner + 1).is_err());
    }
    assert_eq!(
        package::verify_manifest(
            package::open_staged(&staged.request(), unsafe { libc::getuid() }).unwrap()
        )
        .unwrap()
        .id,
        "test.disk"
    );
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    std::fs::write(&path, b"replacement").unwrap();
    assert!(package::open_staged(&staged.request(), unsafe { libc::getuid() }).is_err());
    drop(staged);
    assert!(!path.exists());
}

#[test]
fn visibility_manifest_and_reserved_methods() {
    let dir = tempfile::tempdir().unwrap();
    let bytes = fixture(dir.path(), "test.visibility", "1", 1, Some("steamos"));
    let mut manifest = package::verify(&bytes).unwrap().manifest;
    assert!(!manifest.backend.as_ref().unwrap().ui_visibility_events);
    manifest.backend.as_mut().unwrap().ui_visibility_events = true;
    let encoded = serde_json::to_value(&manifest).unwrap();
    assert_eq!(encoded["backend"]["uiVisibilityEvents"], true);
    assert!(serde_json::from_value::<Manifest>({
        let mut value = encoded;
        value["backend"]["uiVisibilityEvents"] = json!(1);
        value
    })
    .is_err());
    let mut core = accepted_service(dir.path(), 1000).unwrap();
    assert_eq!(
        core.handle("ui.visibility.get", json!({})).unwrap()["known"],
        false
    );
    core.handle(
        "host.ui.visibility",
        json!({"menu":false,"framely.manager":true}),
    )
    .unwrap();
    assert_eq!(
        core.handle("ui.visibility.get", json!({})).unwrap()["captureObscured"],
        true
    );
    assert!(core
        .handle("host.ui.visibility", json!({"browser":true}))
        .is_err());
    assert_eq!(
        core.handle("ui.visibility.get", json!({})).unwrap()["sequence"],
        1
    );
    let events = core.handle("events", json!({})).unwrap();
    assert!(events
        .as_array()
        .unwrap()
        .iter()
        .any(|e| e["kind"] == "ui.visibility.changed"));
}

#[test]
fn visibility_backend_opt_in_restart_and_expiry() {
    let temp = tempfile::tempdir().unwrap();
    let _tools = fake_backend_tools(temp.path());
    let mut core =
        accepted_service(&temp.path().join("state"), unsafe { libc::geteuid() }).unwrap();
    for (id, opt_in) in [("test.visible", true), ("test.legacy", false)] {
        let payload = temp.path().join(id);
        fs::create_dir_all(&payload).unwrap();
        let backend = r#"#!/usr/bin/python3
import sys,json
states=[]
fail_next=False
for line in sys.stdin:
 r=json.loads(line)
 if r['method']=='fail-next':fail_next=True
 if r['method']=='framely.ui.visibility':
  if fail_next:
   fail_next=False;print(json.dumps({'id':r['id'],'error':'temporary visibility failure'}),flush=True);continue
  states.append(r['params'])
 print(json.dumps({'id':r['id'],'result':states}),flush=True)
"#;
        fs::write(payload.join("backend.py"), backend).unwrap();
        let manifest = json!({"schemaVersion":1,"apiVersion":1,"id":id,"name":id,"author":"test","version":"1","backend":{"entry":"backend.py","runAs":"steamos","uiVisibilityEvents":opt_in},"files":{}});
        let path = temp.path().join(format!("{id}.json"));
        fs::write(&path, serde_json::to_vec(&manifest).unwrap()).unwrap();
        let package = temp.path().join(format!("{id}.framely"));
        crate::package::pack(&path, &payload, &package).unwrap();
        install(&mut core, &fs::read(package).unwrap(), json!({})).unwrap();
        core.handle("plugin.open", json!({"plugin":id})).unwrap();
    }
    let read = |core: &mut Service, id: &str| {
        core.handle("plugin.call", json!({"plugin":id,"method":"states"}))
            .unwrap()
    };
    assert_eq!(read(&mut core, "test.visible")[0]["known"], false);
    assert_eq!(read(&mut core, "test.legacy"), json!([]));
    core.handle("host.ui.visibility", json!({"menu":true}))
        .unwrap();
    assert_eq!(read(&mut core, "test.visible")[1]["captureObscured"], true);
    core.handle("plugin.restart", json!({"plugin":"test.visible"}))
        .unwrap();
    assert_eq!(read(&mut core, "test.visible")[0]["captureObscured"], true);
    assert!(core.handle("plugin.call",json!({"plugin":"test.visible","method":"framely.ui.visibility","params":{"known":true,"captureObscured":false}})).is_err());
    core.handle(
        "plugin.call",
        json!({"plugin":"test.visible","method":"fail-next"}),
    )
    .unwrap();
    core.handle("host.ui.visibility", json!({"menu":false}))
        .unwrap();
    assert_eq!(read(&mut core, "test.visible").as_array().unwrap().len(), 1);
    core.maintenance();
    assert_eq!(read(&mut core, "test.visible")[1]["captureObscured"], false);
    assert!(core.db.plugins["test.visible"].error.is_none());
    std::thread::sleep(std::time::Duration::from_millis(2100));
    core.maintenance();
    assert_eq!(read(&mut core, "test.visible")[2]["known"], false);
    assert_eq!(read(&mut core, "test.legacy"), json!([]));
    core.shutdown();
}

#[test]
fn launcher_manifest_validation_and_old_package_compatibility() {
    let root = tempfile::tempdir().unwrap();
    let bytes = fixture(root.path(), "launch.test", "1.0.0", 1, None);
    let old = package::verify(&bytes).unwrap().manifest;
    assert!(
        old.engines.is_none() && old.ui.launch.is_empty() && old.ui.launcher_actions.is_empty()
    );
    let mut value = serde_json::to_value(&old).unwrap();
    value["engines"] = json!({"framely":">=0.4.3-preview.2 <0.5.0"});
    value["ui"]["launch"] = json!({"launcher":{"type":"quickPage"}});
    let m: Manifest = serde_json::from_value(value.clone()).unwrap();
    m.validate().unwrap();
    value["ui"]["launch"]["manager"] = json!({"type":"window","window":"missing"});
    assert!(serde_json::from_value::<Manifest>(value.clone())
        .unwrap()
        .validate()
        .is_err());
    value["ui"]["launch"] = json!({"launcher":{"type":"quickPage"}});
    value["engines"] = json!({"framely":"*"});
    assert!(serde_json::from_value::<Manifest>(value)
        .unwrap()
        .validate()
        .is_err());
}
#[test]
fn engines_ranges_and_incompatible_install_are_non_mutating() {
    let e = |r: &str| Engines {
        framely: Some(r.into()),
    };
    assert!(e(">=0.4.3 <0.5.0").matches("0.4.4+build"));
    assert!(!e(">=0.4.3 <0.5.0").matches("0.4.4-preview.1"));
    assert!(e(">=0.4.3-preview.2 <0.5.0").matches("0.4.3-preview.3"));
    assert!(e(">=0.4.3-preview.2 <0.5.0").matches("0.4.3"));
    assert!(!e(">=0.4.3-preview.2 <0.5.0").matches("0.4.3-preview.1"));
    assert!(e("=0.4.3").matches("0.4.3+abc"));
    assert!(e("^0.4.3").matches("0.4.9"));
    assert!(!e("^0.4.3").matches("0.5.0"));
    assert!(e("~0.4.3").matches("0.4.4"));
    assert!(e("0.4.*").matches("0.4.4"));
    assert!(e("nonsense").validate().is_err());
    assert!(e("0.4.3 || 0.5.0").validate().is_err());
    let root = tempfile::tempdir().unwrap();
    let mut core =
        accepted_service(&root.path().join("state"), unsafe { libc::geteuid() }).unwrap();
    install(
        &mut core,
        &fixture(root.path(), "launch.compat", "1.0.0", 1, None),
        json!({}),
    )
    .unwrap();
    let old = serde_json::to_value(&core.db).unwrap();
    let future = rewrite(
        &fixture(root.path(), "launch.compat", "2.0.0", 2, None),
        |name, data| {
            if name == "manifest.json" {
                let mut m: Value = serde_json::from_slice(data).unwrap();
                m["engines"] = json!({"framely":">=999.0.0"});
                *data = serde_json::to_vec(&m).unwrap();
            }
        },
    );
    assert!(install(&mut core, &future, json!({}))
        .unwrap_err()
        .to_string()
        .contains("requires Framely"));
    assert_eq!(serde_json::to_value(&core.db).unwrap(), old);
    assert!(!root
        .path()
        .join("state/plugins/launch.compat/versions/2.0.0")
        .exists());
    core.db
        .plugins
        .get_mut("launch.compat")
        .unwrap()
        .manifest
        .engines = Some(e(">=999.0.0"));
    assert!(core
        .handle("plugin.open", json!({"plugin":"launch.compat"}))
        .is_err());
}
#[test]
fn launcher_dispatch_preserves_old_open_semantics_and_window_context() {
    let root = tempfile::tempdir().unwrap();
    let mut core =
        accepted_service(&root.path().join("state"), unsafe { libc::geteuid() }).unwrap();
    install(
        &mut core,
        &fixture(root.path(), "launch.routes", "1.0.0", 1, None),
        json!({}),
    )
    .unwrap();
    core.events.lock().unwrap().clear();
    core.handle("plugin.open", json!({"plugin":"launch.routes"}))
        .unwrap();
    assert!(!core
        .handle("events", json!({}))
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .any(|v| v["kind"] == "menu.open"));
    for source in ["launcher", "quickPanel", "manager"] {
        core.handle(
            "plugin.launch",
            json!({"plugin":"launch.routes","source":source}),
        )
        .unwrap();
        let events = core.handle("events", json!({})).unwrap();
        assert!(events
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["kind"] == "menu.open" && v["context"]["source"] == source));
        assert_eq!(
            core.handle(
                "plugin.launch.context",
                json!({"plugin":"launch.routes","entry":"quick"})
            )
            .unwrap()["source"],
            source
        );
    }
    assert!(core
        .handle(
            "plugin.launch",
            json!({"plugin":"launch.routes","source":"unknown"})
        )
        .is_err());
    let plugin = core.db.plugins.get_mut("launch.routes").unwrap();
    plugin.manifest.ui.windows.insert(
        "main".into(),
        serde_json::from_value(json!({"entry":"page.js","title":"Main"})).unwrap(),
    );
    plugin.manifest.ui.launch.insert(
        "launcher".into(),
        LaunchTarget::Window {
            window: "main".into(),
        },
    );
    plugin.manifest.ui.launcher_actions = serde_json::from_value(json!([
      {"id":"window","label":"Open","target":{"type":"window","window":"main"}},
      {"id":"front","label":"Front","target":{"type":"frontend","entry":"page.js"}}
    ]))
    .unwrap();
    core.handle(
        "plugin.launch",
        json!({"plugin":"launch.routes","source":"launcher"}),
    )
    .unwrap();
    assert_eq!(
        core.handle(
            "plugin.launch.context",
            json!({"plugin":"launch.routes","entry":"main"})
        )
        .unwrap()["source"],
        "launcher"
    );
    core.handle(
        "plugin.launcher.action",
        json!({"plugin":"launch.routes","action":"window"}),
    )
    .unwrap();
    assert_eq!(
        core.handle(
            "plugin.launch.context",
            json!({"plugin":"launch.routes","entry":"main"})
        )
        .unwrap()["actionId"],
        "window"
    );
    let frontend = core
        .handle(
            "plugin.launcher.action",
            json!({"plugin":"launch.routes","action":"front"}),
        )
        .unwrap();
    assert_eq!(frontend["frontend"], "page.js");
    assert_eq!(frontend["context"]["actionId"], "front");
    assert!(core
        .handle(
            "plugin.launcher.action",
            json!({"plugin":"launch.routes","action":"unknown"})
        )
        .is_err());

    core.handle(
        "launcher.settings.save",
        json!({"primaryTrigger":true,"menuAutoClose":false,"menuTimeoutSeconds":25}),
    )
    .unwrap();
    let loaded = Service::load(&root.path().join("state"), unsafe { libc::geteuid() }).unwrap();
    assert!(loaded.db.launcher.primary_trigger);
    assert!(!loaded.db.launcher.menu_auto_close);
    assert_eq!(loaded.db.launcher.menu_timeout_seconds, 25);
    let order = vec![
        "framely",
        "plugin:launch.routes",
        "desktop:terminal.desktop",
        "lepton:test/com.example.app",
    ];
    core.handle("launcher.order.save", json!({"order":order}))
        .unwrap();
    for invalid in [
        json!(["framely", "framely"]),
        json!(["unknown:app"]),
        json!(["plugin:"]),
        json!(["desktop:bad\nentry"]),
    ] {
        assert!(core
            .handle("launcher.order.save", json!({"order":invalid}))
            .is_err());
    }
    let loaded = Service::load(&root.path().join("state"), unsafe { libc::geteuid() }).unwrap();
    assert_eq!(loaded.db.launcher_order, order);
}
