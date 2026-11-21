//! Walk and rewrite the Cargo manifest closure of a flat native mod.
use super::api::{self, Project};
use std::collections::{BTreeMap, btree_map::Entry};
use std::fs;
use std::path::{Component, Path, PathBuf};
use sunrise_mod_package::native_route::PACKAGES;

/// Where native builds stage the mods' crates.
pub const DIR: &str = "native-api";

/// A crate a mod's crate asks for in its `Cargo.toml`: under its own name or `package`, in any
/// dependency table (dev and build ones too: the lock has them all).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Wanted {
    pub mod_id: String,
    pub name: String,
    pub version: String,
    pub requirement: semver::VersionReq,
}

/// Rewrite each reachable manifest once and collect its registry requirements. The root was
/// already parsed by `Project`; nested manifests are read from `root` after checking their
/// authored paths. Generated edges (to other mods and SDK facades) belong to the caller.
pub fn rewrite_manifests(
    root: &Path,
    project: &Project<'_>,
    mut write: impl FnMut(&Path, toml::Table) -> Result<(), String>,
) -> Result<Vec<Wanted>, String> {
    let mod_id = &project.bundle.manifest.id;
    let source = &project.bundle.dir;
    let mut wanted = Vec::new();
    let mut packages = BTreeMap::from([(PathBuf::new(), project.package.clone())]);
    let mut pending = vec![(PathBuf::new(), project.cargo.clone())];
    while let Some((relative, mut manifest)) = pending.pop() {
        let is_root = relative.as_os_str().is_empty();
        let path = root.join(&relative).join("Cargo.toml");
        let error = |reason| format!("{}: {reason}", path.display());
        if manifest.contains_key("workspace") {
            return Err(error("[workspace] is refused".to_owned()));
        }
        if !is_root {
            for section in ["patch", "replace"] {
                if manifest.contains_key(section) {
                    return Err(error(format!("[{section}] is refused")));
                }
            }
            let input = source.join(&relative);
            if [".cargo/config", ".cargo/config.toml"]
                .iter()
                .any(|config| input.join(config).exists())
            {
                return Err(error("Cargo source configuration is refused".to_owned()));
            }
        }
        let mut rewrite = |table: &mut toml::Table| -> Result<(), String> {
            for kind in api::DEPENDENCY_KINDS {
                let Some(deps) = table.get_mut(kind) else {
                    continue;
                };
                let deps = deps
                    .as_table_mut()
                    .ok_or_else(|| error(format!("[{kind}] must be a table")))?;
                let mut sdk_keys = Vec::new();
                for (key, dep) in deps.iter() {
                    let package = if is_root {
                        dep.get("package").map_or(key.as_str(), |name| {
                            name.as_str().expect("validated package")
                        })
                    } else {
                        api::validate_dependency(mod_id, kind, key, dep)
                            .map_err(|err| error(err.to_string()))?
                    };
                    if PACKAGES.iter().any(|(sdk, _)| *sdk == package) {
                        sdk_keys.push(key.clone());
                    } else if let Some(local) = dep.get("path") {
                        let local = local.as_str().ok_or_else(|| {
                            error(format!("dependency {key} has a non-string path"))
                        })?;
                        let inside = local_path(source, &relative, local)
                            .map_err(|err| error(format!("dependency {key}: {err}")))?;
                        let name = match packages.entry(inside.clone()) {
                            Entry::Occupied(entry) => entry.into_mut(),
                            Entry::Vacant(entry) => {
                                let original = source.join(&inside).join("Cargo.toml");
                                let metadata = fs::symlink_metadata(&original)
                                    .map_err(|err| format!("{}: {err}", original.display()))?;
                                if is_link_or_junction(&metadata) || !metadata.is_file() {
                                    return Err(format!(
                                        "{}: dependency manifest must be an ordinary file",
                                        original.display()
                                    ));
                                }
                                let nested = root.join(&inside).join("Cargo.toml");
                                let table = api::read(&nested)?;
                                let name = table
                                    .get("package")
                                    .and_then(|p| p.get("name"))
                                    .and_then(toml::Value::as_str)
                                    .ok_or_else(|| {
                                        format!("{}: missing package name", nested.display())
                                    })?
                                    .to_owned();
                                pending.push((inside, table));
                                entry.insert(name)
                            }
                        };
                        if name.as_str() != package {
                            return Err(error(format!(
                                "path dependency {key} has the wrong package name"
                            )));
                        }
                    } else {
                        let version = dep
                            .as_str()
                            .or_else(|| dep.get("version").and_then(toml::Value::as_str))
                            .ok_or_else(|| {
                                error(format!("registry dependency {key} must declare a version"))
                            })?;
                        let requirement = semver::VersionReq::parse(version)
                            .map_err(|err| error(format!("dependency {key}: {err}")))?;
                        wanted.push(Wanted {
                            mod_id: mod_id.clone(),
                            name: package.to_owned(),
                            version: version.to_owned(),
                            requirement,
                        });
                    }
                }
                for key in sdk_keys {
                    deps.remove(&key);
                }
            }
            Ok(())
        };
        rewrite(&mut manifest)?;
        if let Some(targets) = manifest.get_mut("target") {
            let targets = targets
                .as_table_mut()
                .ok_or_else(|| error("[target] must be a table".to_owned()))?;
            for (target, value) in targets.iter_mut() {
                let table = value
                    .as_table_mut()
                    .ok_or_else(|| error(format!("[target.{target}] must be a table")))?;
                rewrite(table)?;
            }
        }
        write(&relative, manifest)?;
    }
    Ok(wanted)
}

/// Resolve a path within one mod without following links or traversing outside its root.
fn local_path(root: &Path, relative: &Path, path: &str) -> Result<PathBuf, String> {
    if path.contains('\\') {
        return Err("dependency path must use portable separators".into());
    }
    let mut inside = relative.to_path_buf();
    for component in Path::new(path).components() {
        match component {
            Component::Normal(part) => inside.push(part),
            Component::CurDir => {}
            Component::ParentDir if inside.pop() => {}
            _ => return Err("path outside the mod is refused".into()),
        }
    }
    let mut checked = root.to_path_buf();
    for part in inside.components() {
        checked.push(part);
        let meta = fs::symlink_metadata(&checked).map_err(|e| e.to_string())?;
        if is_link_or_junction(&meta) {
            return Err("linked dependency path is refused".into());
        }
    }
    Ok(inside)
}

/// `symlink_metadata` reports links without following them. Windows junctions are reparse points
/// as well, so reject the reparse attribute even if the standard file type is not a symlink.
fn is_link_or_junction(metadata: &fs::Metadata) -> bool {
    let link = metadata.file_type().is_symlink();
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        link || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }
    #[cfg(not(windows))]
    link
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Bundle;

    fn cargo(body: &str) -> String {
        format!(
            "[package]\nname='bake-engine'\nversion='0.1.0'\n[dependencies]\n\
             api={{package='sunrise-mod-api',path='.sunrise/sdk/api/crates/sunrise-mod-api'}}\n{body}"
        )
    }

    fn fixture(root: &Path, deps: &str) -> Bundle {
        crate::tests::write_bundle(root, "bake", "");
        fs::write(root.join("Cargo.toml"), cargo(deps)).unwrap();
        crate::bundle_in(root).unwrap().unwrap()
    }

    #[test]
    fn staged_manifest_closure_strips_sdk_paths_and_collects_registry_requirements() {
        let source = tempfile::tempdir().unwrap();
        let stage = tempfile::tempdir().unwrap();
        let bundle = fixture(
            source.path(),
            "helper={path='src/helper'}\n\
             [target.'cfg(unix)'.dev-dependencies]\nhelper={path='src/helper'}\n",
        );
        let helper = source.path().join("src/helper");
        fs::create_dir_all(helper.join("src")).unwrap();
        fs::write(helper.join("src/lib.rs"), "").unwrap();
        fs::write(
            helper.join("Cargo.toml"),
            "[package]\nname='helper'\nversion='0.1.0'\n[dependencies]\n\
             sdk={package='sunrise-mod-api',path='.sunrise/sdk/api/crates/sunrise-mod-api'}\n\
             [build_dependencies]\nbuild={package='cc',version='1'}\n\
             [target.'cfg(windows)'.dev-dependencies]\nitoa='=0.4.8'\n",
        )
        .unwrap();
        let project = Project::read(&bundle).unwrap();
        crate::copy_bundle(&bundle, stage.path()).unwrap();
        let mut requested = rewrite_manifests(stage.path(), &project, |relative, manifest| {
            fs::write(
                stage.path().join(relative).join("Cargo.toml"),
                manifest.to_string(),
            )
            .map_err(|e| e.to_string())
        })
        .unwrap();
        requested.sort_by(|a, b| a.name.cmp(&b.name));
        assert_eq!(
            requested,
            [
                Wanted {
                    mod_id: "bake".into(),
                    name: "cc".into(),
                    version: "1".into(),
                    requirement: semver::VersionReq::parse("^1").unwrap(),
                },
                Wanted {
                    mod_id: "bake".into(),
                    name: "itoa".into(),
                    version: "=0.4.8".into(),
                    requirement: semver::VersionReq::parse("=0.4.8").unwrap(),
                },
            ]
        );
        let root = api::read(&stage.path().join("Cargo.toml")).unwrap();
        assert_eq!(
            root["dependencies"]["helper"]["path"].as_str(),
            Some("src/helper")
        );
        assert!(root["dependencies"].get("api").is_none());
        let nested = api::read(&stage.path().join("src/helper/Cargo.toml")).unwrap();
        assert!(nested["dependencies"].get("sdk").is_none());
        assert_eq!(
            nested["build_dependencies"]["build"]["version"].as_str(),
            Some("1")
        );
        assert!(
            api::read(&helper.join("Cargo.toml")).unwrap()["dependencies"]
                .get("sdk")
                .is_some()
        );
    }

    #[test]
    fn authored_paths_must_stay_inside_the_mod_and_match_the_package() {
        let workspace = tempfile::tempdir().unwrap();
        let root = workspace.path().join("native-api/bake");
        let bundle = fixture(&root, "");
        for (path, package) in [
            (root.join("src/helper"), "helper"),
            (workspace.path().join("native-api/other"), "other"),
            (
                workspace.path().join("crates/sunrise-mod-api"),
                "sunrise-mod-api",
            ),
        ] {
            fs::create_dir_all(&path).unwrap();
            fs::write(
                path.join("Cargo.toml"),
                format!("[package]\nname={package:?}\nversion='0.1.0'\n"),
            )
            .unwrap();
        }
        fs::create_dir_all(root.join("src/not_a_crate/Cargo.toml")).unwrap();
        let absolute = format!(
            "helper={{path={:?}}}",
            root.join("src/helper")
                .display()
                .to_string()
                .replace('\\', "/")
        );
        for (entry, reason) in [
            ("other={path='../other'}", "outside the mod"),
            (absolute.as_str(), "outside the mod"),
            ("other={path='../../outside'}", "outside the mod"),
            (
                "old={package='sunrise-mod-api',path='../../crates/sunrise-mod-api'}",
                "E_SDK_SOURCE",
            ),
            (
                "helper={path='src/helper',package='different-package'}",
                "wrong package name",
            ),
            (
                "helper={path='src/not_a_crate'}",
                "must be an ordinary file",
            ),
            ("helper={path=7}", "non-string path"),
        ] {
            let error = Project::parse(&bundle, &cargo(entry))
                .map_err(|err| err.to_string())
                .and_then(|project| rewrite_manifests(&root, &project, |_, _| Ok(())))
                .unwrap_err();
            assert!(error.contains(reason), "{entry}: {error}");
        }
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(root.join("src/helper"), root.join("src/link")).unwrap();
            let project = Project::parse(&bundle, &cargo("helper={path='src/link'}")).unwrap();
            let error = rewrite_manifests(&root, &project, |_, _| Ok(())).unwrap_err();
            assert!(
                error.contains("linked dependency path is refused"),
                "{error}"
            );
        }
    }

    #[test]
    fn nested_manifests_do_not_supply_sources_or_override_the_workspace() {
        let source = tempfile::tempdir().unwrap();
        let bundle = fixture(source.path(), "helper={path='src/helper'}");
        let helper = source.path().join("src/helper");
        fs::create_dir_all(&helper).unwrap();
        let project = Project::read(&bundle).unwrap();
        let nested = "[package]\nname='helper'\nversion='0.1.0'\n";
        for section in ["patch", "replace", "workspace"] {
            fs::write(helper.join("Cargo.toml"), format!("{nested}[{section}]\n")).unwrap();
            let error = rewrite_manifests(source.path(), &project, |_, _| Ok(())).unwrap_err();
            assert!(
                error.contains(&format!("[{section}] is refused")),
                "{error}"
            );
        }
        for (kind, value) in [
            ("git", "'https://example.invalid/repo'"),
            ("registry", "'another'"),
            ("registry-index", "'https://example.invalid/index'"),
            ("branch", "'main'"),
            ("tag", "'v1'"),
            ("rev", "'abcdef'"),
            ("workspace", "true"),
        ] {
            fs::write(
                helper.join("Cargo.toml"),
                format!("{nested}[dependencies]\nx={{path='.', {kind}={value}}}\n"),
            )
            .unwrap();
            let error = rewrite_manifests(source.path(), &project, |_, _| Ok(())).unwrap_err();
            assert!(
                error.contains(&format!("{kind} source is refused")),
                "{error}"
            );
        }
        fs::write(helper.join("Cargo.toml"), nested).unwrap();
        fs::create_dir(helper.join(".cargo")).unwrap();
        fs::write(helper.join(".cargo/config"), "").unwrap();
        let error = rewrite_manifests(source.path(), &project, |_, _| Ok(())).unwrap_err();
        assert!(
            error.contains("Cargo source configuration is refused"),
            "{error}"
        );
    }
}
