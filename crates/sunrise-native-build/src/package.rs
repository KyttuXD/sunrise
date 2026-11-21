//! Validated flat packages and launcher-owned horizon-mixins input.
use super::{Bundle, MANIFEST};
use std::{fs, path::Path};
use sunrise_mod_package::Package;
use sunrise_mod_package::manifest::{self, NativeStateRefusal};
use sunrise_mod_package::mod_surface::uses::USES_FILE;

/// Return the validated native project, if this directory has a package manifest.
/// A present but invalid manifest is reported by the canonical package reader.
pub fn read(dir: &Path) -> Result<Option<Bundle>, String> {
    let path = dir.join(manifest::MANIFEST);
    match fs::symlink_metadata(&path) {
        Ok(_) => {}
        Err(e)
            if matches!(
                e.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
            ) =>
        {
            return Ok(None);
        }
        Err(e) => return Err(format!("{}: {e}", path.display())),
    }
    let package = read_package(dir)?;
    if !package.is_native() {
        return Ok(None);
    }
    package.directory_files(dir)?;
    Ok(Some(Bundle {
        manifest: package.manifest,
        dir: dir.to_owned(),
    }))
}

/// Generate tool input in the build's incoming copy, never in a modder's project.
pub fn tool_manifest(bundle: &Bundle) -> Result<(), String> {
    let mut manifest = toml::Table::new();
    manifest.insert(
        "mod".into(),
        toml::Value::Table(toml::Table::from_iter([
            ("id".into(), bundle.manifest.id.clone().into()),
            ("version".into(), bundle.manifest.version.to_string().into()),
        ])),
    );
    manifest.insert(
        "compatibility".into(),
        toml::Value::Table(toml::Table::from_iter([(
            "horizon_revision".into(),
            bundle.native().sunrise.clone().into(),
        )])),
    );
    for (name, rows) in [
        ("injection", toml::Value::try_from(&bundle.native().mixin)),
        ("bridge", toml::Value::try_from(&bundle.native().bridge)),
    ] {
        let rows = rows.map_err(|error| error.to_string())?;
        if rows.as_array().is_some_and(|rows| !rows.is_empty()) {
            manifest.insert(name.into(), rows);
        }
    }
    if !bundle.native().module.is_empty() {
        manifest.insert(
            "module".into(),
            toml::Value::try_from(&bundle.native().module).map_err(|e| e.to_string())?,
        );
    }
    fs::write(
        bundle.dir.join(MANIFEST),
        toml::to_string(&manifest).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}

fn read_package(path: &Path) -> Result<Package, String> {
    Package::read_with_diagnostics(path, |error| {
        crate::host::trf(
            NativeStateRefusal::TEMPLATE,
            &[("mod", error.mod_id), ("state", error.row)],
        )
    })
}

fn checked_files(bundle: &Bundle) -> Result<std::collections::BTreeMap<String, Vec<u8>>, String> {
    read_package(&bundle.dir)?.directory_files(&bundle.dir)
}

pub fn files(bundle: &Bundle) -> Result<Vec<(String, std::path::PathBuf)>, String> {
    Ok(checked_files(bundle)?
        .into_keys()
        .map(|name| {
            let path = bundle.dir.join(&name);
            (name, path)
        })
        .collect())
}

/// The package's files and its uses table, which is no source file: a copy is judged against
/// another game build as the package was.
pub fn copy(bundle: &Bundle, to: &Path) -> Result<(), String> {
    let mut files = checked_files(bundle)?;
    if let Ok(table) = fs::read(bundle.dir.join(USES_FILE)) {
        files.insert(USES_FILE.to_owned(), table);
    }
    copy_files(to, files)
}

/// Only code enters the tool; runtime resource packs stay with the package.
pub fn copy_tool_input(bundle: &Bundle, to: &Path) -> Result<(), String> {
    copy_files(
        to,
        checked_files(bundle)?.into_iter().filter(|(name, _)| {
            manifest::source_file(name)
                || bundle.native().mixin.iter().any(|row| row.helper == *name)
        }),
    )
}

fn copy_files(to: &Path, files: impl IntoIterator<Item = (String, Vec<u8>)>) -> Result<(), String> {
    for (name, bytes) in files {
        let path = to.join(name);
        fs::create_dir_all(path.parent().ok_or("invalid destination")?)
            .map_err(|e| e.to_string())?;
        fs::write(path, bytes).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::write_bundle;

    #[test]
    fn scan_skips_non_packages_and_reads_native_projects_with_unrelated_folders() {
        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().join("workspace");
        fs::create_dir_all(workspace.join("rust")).unwrap();
        fs::write(workspace.join("horizon-mixin.toml"), "not a package").unwrap();
        let project = workspace.join("example");
        write_bundle(&project, "example", "");
        fs::create_dir(project.join("native")).unwrap();
        fs::write(project.join("sunrise-native.json"), "not a manifest").unwrap();

        let bundles = crate::scan_bundles(&workspace).unwrap();
        assert_eq!(bundles.len(), 1);
        assert_eq!(bundles[0].manifest.id, "example");
        assert_eq!(bundles[0].dir, project);
        let names: Vec<_> = files(&bundles[0])
            .unwrap()
            .into_iter()
            .map(|(name, _)| name)
            .collect();
        assert_eq!(names, ["Cargo.toml", "src/lib.rs", manifest::MANIFEST]);
    }

    #[test]
    fn a_present_malformed_manifest_is_not_skipped() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join(manifest::MANIFEST)).unwrap();
        let error = read(root.path()).unwrap_err();
        assert_eq!(
            error,
            "sunrise-mod.toml: unsupported file type in a content mod"
        );
    }
}
