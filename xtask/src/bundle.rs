use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::{Result, root, run, take_flag};

const APP: &str = "diffpilot";
const DISPLAY_NAME: &str = "Diffpilot";
const BUNDLE_ID: &str = "com.adamward459.diffpilot";
const BINS: [&str; 3] = ["diffpilot", "diffpilot-guard", "diffpilot-mcp"];
const TARGETS: [&str; 2] = ["aarch64-apple-darwin", "x86_64-apple-darwin"];
const ICNS: &str = "gpui-app/assets/icon/AppIcon.icns";
// install.sh downloads this name from releases/latest, so keep it stable across versions.
const ZIP: &str = "diffpilot-macos.zip";

pub fn bundle(rest: &[String]) -> Result {
    let (universal, rest) = take_flag(rest, "--universal");
    let (zip, rest) = take_flag(&rest, "--zip");
    if let Some(arg) = rest.first() {
        return Err(format!("bundle: unknown option {arg}").into());
    }
    let meta: serde_json::Value = serde_json::from_str(&run(Command::new("cargo").args([
        "metadata",
        "--format-version",
        "1",
        "--no-deps",
    ]))?)?;
    let target_dir = PathBuf::from(
        meta["target_directory"]
            .as_str()
            .ok_or("cargo metadata has no target_directory")?,
    );
    let version = meta["packages"]
        .as_array()
        .and_then(|ps| ps.iter().find(|p| p["name"] == APP))
        .and_then(|p| p["version"].as_str())
        .ok_or("cargo metadata has no diffpilot package")?
        .to_owned();
    let out = target_dir.join("bundle");
    fs::create_dir_all(&out)?;

    let targets: &[&str] = if universal { &TARGETS } else { &[] };
    cargo_build(targets)?;
    let bins: Vec<PathBuf> = if universal {
        BINS.iter()
            .map(|bin| lipo(&target_dir, &out, bin))
            .collect::<Result<_>>()?
    } else {
        BINS.iter().map(|bin| target_dir.join("release").join(bin)).collect()
    };

    let app = out.join(format!("{APP}.app"));
    if app.exists() {
        fs::remove_dir_all(&app)?;
    }
    let macos = app.join("Contents/MacOS");
    fs::create_dir_all(&macos)?;
    let resources = app.join("Contents/Resources");
    fs::create_dir_all(&resources)?;
    fs::copy(root().join(ICNS), resources.join("AppIcon.icns"))?;
    for bin in &bins {
        let dest = macos.join(bin.file_name().ok_or("binary has no name")?);
        fs::copy(bin, &dest)?;
        if dest.file_name() != Some(APP.as_ref()) {
            codesign(&dest)?;
        }
    }
    fs::write(app.join("Contents/Info.plist"), info_plist(&version))?;
    codesign(&app)?;
    println!("{}", app.display());

    if zip {
        let zip = out.join(ZIP);
        if zip.exists() {
            fs::remove_file(&zip)?;
        }
        run(Command::new("ditto")
            .args(["-c", "-k", "--keepParent"])
            .arg(&app)
            .arg(&zip))?;
        println!("{}", zip.display());
    }
    Ok(())
}

fn cargo_build(targets: &[&str]) -> Result {
    let mut cmd = Command::new("cargo");
    cmd.args([
        "build",
        "--release",
        "-p",
        "diffpilot",
        "-p",
        "diffpilot-core",
        "--bins",
    ]);
    for target in targets {
        cmd.args(["--target", target]);
    }
    if !cmd.current_dir(root()).status()?.success() {
        return Err("cargo build failed".into());
    }
    Ok(())
}

fn lipo(target_dir: &Path, out: &Path, bin: &str) -> Result<PathBuf> {
    let dir = out.join("universal");
    fs::create_dir_all(&dir)?;
    let dest = dir.join(bin);
    let mut cmd = Command::new("lipo");
    cmd.arg("-create").arg("-output").arg(&dest);
    for target in TARGETS {
        cmd.arg(target_dir.join(target).join("release").join(bin));
    }
    run(&mut cmd)?;
    Ok(dest)
}

fn codesign(path: &Path) -> Result {
    run(Command::new("codesign").args(["--force", "--sign", "-"]).arg(path)).map(drop)
}

fn info_plist(version: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key><string>{DISPLAY_NAME}</string>
    <key>CFBundleDisplayName</key><string>{DISPLAY_NAME}</string>
    <key>CFBundleIdentifier</key><string>{BUNDLE_ID}</string>
    <key>CFBundleIconFile</key><string>AppIcon</string>
    <key>CFBundleExecutable</key><string>{APP}</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>{version}</string>
    <key>CFBundleVersion</key><string>{version}</string>
    <key>LSMinimumSystemVersion</key><string>13.0</string>
    <key>LSApplicationCategoryType</key><string>public.app-category.developer-tools</string>
    <key>NSHighResolutionCapable</key><true/>
    <key>NSSupportsAutomaticGraphicsSwitching</key><true/>
</dict>
</plist>
"#
    )
}
