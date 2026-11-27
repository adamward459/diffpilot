use crate::features::{Binary, Feature, Scene, resolve};
use crate::perf::build_target;
use crate::pixels::diff_images;
use crate::workspace::Workspace;
use crate::{Result, fixture, parse_option, resolve_base, root, take_flag};
use image::{ImageFormat, RgbaImage};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const WINDOW_TIMEOUT: Duration = Duration::from_secs(60);
const SETTLE: Duration = Duration::from_secs(3);

const WINDOW_JXA: &str = r#"
ObjC.import("CoreGraphics");
const pid = %PID%;
const list = ObjC.castRefToObject($.CGWindowListCopyWindowInfo(1, 0));
let best = 0, area = 0;
for (let i = 0; i < list.count; i++) {
  const w = ObjC.deepUnwrap(list.objectAtIndex(i));
  if (w.kCGWindowOwnerPID !== pid || w.kCGWindowLayer !== 0) continue;
  const a = w.kCGWindowBounds.Width * w.kCGWindowBounds.Height;
  if (a > area) { best = w.kCGWindowNumber; area = a; }
}
best
"#;

/// The features among `names` (all when empty) that have scenes.
pub fn with_scenes(names: &[String]) -> Result<Vec<&'static Feature>> {
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    Ok(resolve(&names)?.into_iter().filter(|f| !f.scenes.is_empty()).collect())
}

pub fn scene_path(feature: &Feature, scene: &Scene) -> String {
    format!("screenshots/{}/{}.png", feature.name, scene.name)
}

fn describe(target: &Binary) -> String {
    target.cargo_args().join(" ")
}

pub fn visual(rest: &[String]) -> Result {
    let (base, rest) = parse_option(rest, "--base")?;
    let (compare_flag, rest) = take_flag(&rest, "--compare");
    let (dry_run, names) = take_flag(&rest, "--dry-run");
    let features = with_scenes(&names)?;
    if features.is_empty() {
        eprintln!("No selected feature has scenes.");
        return Ok(());
    }
    if dry_run {
        for f in &features {
            for s in f.scenes {
                let env: Vec<String> = s.env.iter().map(|(k, v)| format!("{k}={v}")).collect();
                println!(
                    "{}  cargo build --release {}  {}",
                    scene_path(f, s),
                    describe(&s.target),
                    env.join(" ")
                );
            }
        }
        return Ok(());
    }
    capture(&features)?;
    if compare_flag {
        let reference = resolve_base(base);
        let changed = compare(&features, &reference)?;
        eprintln!("== screenshots changed vs {reference}: {}", changed.len());
        for line in changed {
            println!("  {line}");
        }
    }
    Ok(())
}

pub fn capture(features: &[&Feature]) -> Result {
    fixture::ensure(false)?;
    let mut built: Vec<(String, PathBuf)> = vec![];
    for f in features {
        for s in f.scenes {
            let key = describe(&s.target);
            let exe = match built.iter().find(|(k, _)| *k == key) {
                Some((_, exe)) => exe.clone(),
                None => {
                    let exe = build_target(&format!("{}/{}", f.name, s.name), &s.target, &root())?;
                    built.push((key, exe.clone()));
                    exe
                }
            };
            eprintln!("== visual {}/{}", f.name, s.name);
            capture_scene(&exe, s, &root().join(scene_path(f, s))).map_err(|e| {
                format!(
                    "{}/{}: {e}. screencapture needs Screen Recording permission.",
                    f.name, s.name
                )
            })?;
        }
    }
    Ok(())
}

fn window_id(pid: u32) -> u32 {
    Command::new("osascript")
        .args(["-l", "JavaScript", "-e", &WINDOW_JXA.replace("%PID%", &pid.to_string())])
        .output()
        .ok()
        .and_then(|out| String::from_utf8(out.stdout).ok())
        .and_then(|text| text.trim().parse().ok())
        .unwrap_or(0)
}

fn capture_scene(exe: &Path, scene: &Scene, out: &Path) -> Result {
    let mut app = Command::new(exe)
        .current_dir(root())
        .env("DIFFPILOT_REPO", fixture::repo())
        .env("DIFFPILOT_VIEWED_SEED", fixture::viewed_seed())
        .envs(scene.env.iter().copied())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    let shot = shoot(&mut app, out);
    let _ = app.kill();
    let _ = app.wait();
    shot
}

fn shoot(app: &mut std::process::Child, out: &Path) -> Result {
    let started = Instant::now();
    let mut window = 0;
    while window == 0 && started.elapsed() < WINDOW_TIMEOUT && app.try_wait()?.is_none() {
        std::thread::sleep(Duration::from_millis(500));
        window = window_id(app.id());
    }
    if window == 0 {
        return Err("the app showed no window within 60 s (or exited early)".into());
    }
    std::thread::sleep(SETTLE);
    if let Some(dir) = out.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let status = Command::new("screencapture")
        .args(["-x", "-o", "-l", &window.to_string()])
        .arg(out)
        .status()?;
    if !status.success() {
        return Err(format!("screencapture failed ({status})").into());
    }
    Ok(())
}

fn decode(bytes: &[u8]) -> Result<RgbaImage> {
    Ok(image::load_from_memory_with_format(bytes, ImageFormat::Png)?.to_rgba8())
}

/// One line per image that differs from its copy on `reference`, and a marked diff image for each under the build dir.
pub fn compare(features: &[&Feature], reference: &str) -> Result<Vec<String>> {
    let diff_root = Workspace::load()?.build_dir.join("screenshot-diff");
    let mut lines = vec![];
    for f in features {
        for s in f.scenes {
            let path = scene_path(f, s);
            let Ok(new) = std::fs::read(root().join(&path)) else {
                continue;
            };
            let old = Command::new("git")
                .args(["show", &format!("{reference}:{path}")])
                .current_dir(root())
                .output()?;
            if !old.status.success() {
                lines.push(format!("{path}: new, not on {reference}"));
                continue;
            }
            let Some((diff, marked)) = diff_images(&decode(&old.stdout)?, &decode(&new)?, &path) else {
                continue;
            };
            match marked {
                Some(marked) => {
                    let out = diff_root.join(f.name).join(format!("{}.png", s.name));
                    std::fs::create_dir_all(out.parent().expect("a file path has a parent"))?;
                    marked.save_with_format(&out, ImageFormat::Png)?;
                    lines.push(format!("{diff} (diff: {})", out.display()));
                }
                None => lines.push(diff.to_string()),
            }
        }
    }
    Ok(lines)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::FEATURES;

    #[test]
    fn scene_names_are_unique_per_feature_and_files_stay_under_screenshots() {
        for f in FEATURES {
            for (i, s) in f.scenes.iter().enumerate() {
                assert!(f.scenes[..i].iter().all(|o| o.name != s.name), "{}/{}", f.name, s.name);
                assert!(!s.name.contains('/') && !s.name.is_empty());
            }
        }
    }

    #[test]
    fn naming_a_feature_without_scenes_selects_nothing() {
        assert!(with_scenes(&["sidebar".to_owned()]).unwrap().is_empty());
        let names: Vec<_> = with_scenes(&[]).unwrap().iter().map(|f| f.name).collect();
        assert_eq!(names, ["diff-pane", "review", "guide", "app-shell", "shared"]);
    }
}
