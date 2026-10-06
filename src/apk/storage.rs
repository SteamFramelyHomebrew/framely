//! Resource and Mesa cache directories remain inside the backed-up context,
//! but outside data_overlay so downloads do not pay OverlayFS write costs.
use anyhow::{ensure, Result};
use std::{fs, path::Path, process::Command};

pub(super) fn prepare(baked: &Path) -> Result<()> {
    ensure!(baked.is_absolute(), "Container data path must be absolute");
    ensure!(
        !baked.is_symlink(),
        "Container data directory cannot be a link"
    );
    let cache = baked.join("shadercache");
    ensure!(
        !cache.is_symlink(),
        "Shader cache directory cannot be a link"
    );
    fs::create_dir_all(cache)?;
    Ok(())
}

pub(super) fn configure(command: &mut Command, baked: &Path, adopted: bool) {
    // An inherited Steam compat path can make Lepton relocate a development
    // context. Only adopted Steam contexts should receive that variable.
    command
        .env_remove("FRAMELY_EXTERNAL_MEDIA_DIR")
        .env_remove("STEAM_COMPAT_SHADER_PATH")
        .env("FRAMELY_SHADER_CACHE_DIR", baked.join("shadercache"));
    if !adopted {
        command
            .env_remove("STEAM_COMPAT_DATA_PATH")
            .env("FRAMELY_EXTERNAL_MEDIA_DIR", baked.join("external"));
    }
}

pub(super) const HOOKS: &str = r#"
function framely_prepare_external_media() {
    local media="$(data_mount_path)/media/0"
    local external="${FRAMELY_EXTERNAL_MEDIA_DIR:?}"
    [[ "$external" == /* && "$external" != "$media" && ! -L "$external" ]] || {
        echo "Invalid Android external storage directory" >&2; return 64;
    }
    if [[ -e "$external" && ! -d "$external" ]]; then
        echo "Android external storage is not a directory" >&2; return 64
    fi
    if [[ -L "$media" ]]; then
        [[ "$(readlink "$media")" == "$external" && -d "$external" ]] || {
            echo "Unexpected Android media link; refusing to replace it" >&2; return 64;
        }
        return 0
    fi
    if [[ -d "$media" ]]; then
        if [[ -z "$(find "$media" -mindepth 1 -maxdepth 1 -print -quit)" ]]; then
            rmdir -- "$media" || return
        else
            if [[ -d "$external" ]]; then
                [[ -z "$(find "$external" -mindepth 1 -maxdepth 1 -print -quit)" ]] || {
                    echo "Both Android media locations contain data; refusing to discard either" >&2; return 64;
                }
                rmdir -- "$external" || return
            fi
            # Both paths belong to the same context filesystem. Rename keeps
            # partial downloads, checkpoints and file ownership intact.
            mv -T -- "$media" "$external" || return
        fi
    elif [[ -e "$media" ]]; then
        echo "Invalid Android media location" >&2; return 64
    fi
    mkdir -p -- "$external" "$(dirname "$media")" || return
    ln -s -- "$external" "$media"
}
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::{fs::symlink, fs::MetadataExt};

    fn migrate(data: &Path, external: &Path) -> bool {
        Command::new("bash")
            .args(["-euc", &format!("function data_mount_path() {{ echo \"$TEST_DATA\"; }}\n{HOOKS}\nframely_prepare_external_media")])
            .env("TEST_DATA", data)
            .env("FRAMELY_EXTERNAL_MEDIA_DIR", external)
            .env("BASH_ENV", "/dev/null")
            .status()
            .unwrap()
            .success()
    }

    #[test]
    fn resource_migration_preserves_inodes_and_resumes_after_interruption() {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join("data_overlay");
        let external = dir.path().join("external");
        let resource = Path::new("Android/data/game/files/resource.part");
        let original = data.join("media/0").join(resource);
        fs::create_dir_all(original.parent().unwrap()).unwrap();
        fs::write(&original, b"unfinished download").unwrap();
        fs::write(original.with_file_name("checkpoint"), b"offset=19").unwrap();
        let inode = fs::metadata(&original).unwrap().ino();
        assert!(migrate(&data, &external));
        assert_eq!(fs::metadata(external.join(resource)).unwrap().ino(), inode);
        assert!(migrate(&data, &external));
        assert_eq!(fs::read(&original).unwrap(), b"unfinished download");
        assert_eq!(
            fs::read(original.with_file_name("checkpoint")).unwrap(),
            b"offset=19"
        );
        // Power loss after rename but before symlink creation is recoverable.
        fs::remove_file(data.join("media/0")).unwrap();
        assert!(migrate(&data, &external));
        assert_eq!(fs::metadata(&original).unwrap().ino(), inode);
    }

    #[test]
    fn migration_refuses_conflicting_data_and_unexpected_links() {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join("data_overlay");
        let media = data.join("media/0");
        let external = dir.path().join("external");
        fs::create_dir_all(&media).unwrap();
        fs::create_dir(&external).unwrap();
        fs::write(media.join("resource"), b"original").unwrap();
        fs::write(external.join("resource"), b"other").unwrap();
        assert!(!migrate(&data, &external));
        assert_eq!(fs::read(media.join("resource")).unwrap(), b"original");
        assert_eq!(fs::read(external.join("resource")).unwrap(), b"other");
        fs::remove_dir_all(&media).unwrap();
        symlink(dir.path(), &media).unwrap();
        assert!(!migrate(&data, &external));
        assert_eq!(fs::read_link(&media).unwrap(), dir.path());
        fs::remove_file(&media).unwrap();
        fs::remove_dir_all(&external).unwrap();
        symlink(dir.path(), &external).unwrap();
        assert!(!migrate(&data, &external));
    }

    #[test]
    fn cache_is_persistent_and_adopted_contexts_keep_their_compat_path() {
        let dir = tempfile::tempdir().unwrap();
        let baked = dir.path().join("baked");
        prepare(&baked).unwrap();
        let cache = baked.join("shadercache/compiled");
        fs::write(&cache, b"shader").unwrap();
        prepare(&baked).unwrap();
        assert_eq!(fs::read(&cache).unwrap(), b"shader");
        for adopted in [false, true] {
            let mut command = Command::new("true");
            command.env("STEAM_COMPAT_DATA_PATH", "/steam/compat");
            configure(&mut command, &baked, adopted);
            let env: std::collections::BTreeMap<_, _> = command.get_envs().collect();
            assert_eq!(
                env[std::ffi::OsStr::new("STEAM_COMPAT_DATA_PATH")],
                adopted.then_some(std::ffi::OsStr::new("/steam/compat"))
            );
            assert_eq!(
                env[std::ffi::OsStr::new("FRAMELY_EXTERNAL_MEDIA_DIR")],
                (!adopted).then_some(baked.join("external").as_os_str())
            );
        }
        fs::remove_dir_all(baked.join("shadercache")).unwrap();
        symlink(dir.path(), baked.join("shadercache")).unwrap();
        assert!(prepare(&baked).is_err());
    }
}
