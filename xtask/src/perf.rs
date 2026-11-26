use crate::features::{Binary, Feature, resolve};
use crate::{Result, fixture, parse_option, resolve_base, root, run, take_flag};
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const BASELINE: &str = "baselines/bench.json";
// The frame-callback benches stop getting frames while another window covers theirs.
const BENCH_TIMEOUT: Duration = Duration::from_secs(180);

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Limit {
    Ratio(f64),
    Bytes(f64),
    Report,
}

// Maxima, over-budget counts and interval counts swing 20-50% between runs, so they have no limit.
// Peak RSS of one diff pane scroll phase lands on 195 MB or 237 MB between runs of the same build, so it is only reported.
const LIMITS: [(&str, Limit); 6] = [
    ("_median_ms", Limit::Ratio(0.05)),
    ("_p95_ms", Limit::Ratio(0.05)),
    ("_allocs", Limit::Ratio(0.05)),
    ("_alloc_bytes", Limit::Ratio(0.05)),
    ("_peak_rss_bytes", Limit::Report),
    ("_retained_bytes", Limit::Bytes(64.0 * 1024.0)),
];

pub fn limit(name: &str) -> Option<Limit> {
    LIMITS
        .iter()
        .find(|(suffix, _)| name.ends_with(suffix))
        .map(|(_, limit)| *limit)
}

impl Limit {
    fn exceeded(self, base: f64, head: f64) -> bool {
        match self {
            Limit::Ratio(ratio) => base > 0.0 && (head - base) / base > ratio,
            Limit::Bytes(bytes) => head - base > bytes,
            Limit::Report => false,
        }
    }

    fn describe(self) -> String {
        match self {
            Limit::Ratio(ratio) => format!("{:.0}%", ratio * 100.0),
            Limit::Bytes(bytes) => format!("{bytes} bytes"),
            Limit::Report => "none".into(),
        }
    }
}

type Metrics = BTreeMap<String, f64>;

fn flatten(value: &Value, prefix: &str, out: &mut Metrics) {
    match value {
        Value::Number(n) => {
            if let Some(n) = n.as_f64() {
                out.insert(prefix.to_owned(), n);
            }
        }
        Value::Object(map) => {
            for (key, item) in map {
                let name = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{prefix}_{key}")
                };
                flatten(item, &name, out);
            }
        }
        _ => {}
    }
}

/// The metrics in a bench's stdout that have a limit. Nested objects join their names with `_`.
pub fn parse_metrics(stdout: &str) -> std::result::Result<Metrics, String> {
    let value: Value =
        serde_json::from_str(stdout).map_err(|_| "DIFFPILOT_BENCH=1 did not print a JSON object on stdout")?;
    let mut all = Metrics::new();
    flatten(&value, "", &mut all);
    let kept: Metrics = all.into_iter().filter(|(name, _)| limit(name).is_some()).collect();
    if kept.is_empty() {
        let suffixes: Vec<_> = LIMITS.iter().map(|(s, _)| *s).collect();
        return Err(format!(
            "DIFFPILOT_BENCH=1 printed no metric with a limit (names ending {})",
            suffixes.join(", ")
        ));
    }
    Ok(kept)
}

struct Benched {
    feature: &'static str,
    bench: &'static Binary,
}

fn benched(features: Vec<&'static Feature>) -> Vec<Benched> {
    features
        .into_iter()
        .filter_map(|f| f.bench.as_ref().map(|bench| Benched { feature: f.name, bench }))
        .collect()
}

/// Builds one binary in release mode in `cwd` and returns its executable.
pub fn build_target(label: &str, target: &Binary, cwd: &Path) -> Result<PathBuf> {
    let mut args = vec!["build", "--release", "--message-format=json-render-diagnostics"];
    args.extend(target.cargo_args());
    eprintln!("+ cargo {}", args.join(" "));
    let out = Command::new("cargo")
        .args(&args)
        .current_dir(cwd)
        .stderr(Stdio::inherit())
        .output()?;
    if !out.status.success() {
        return Err(format!("{label}: the build failed in {}", cwd.display()).into());
    }
    Ok(String::from_utf8(out.stdout)?
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter_map(|m| m["executable"].as_str().map(PathBuf::from))
        .next_back()
        .ok_or_else(|| format!("{label}: cargo built no executable"))?)
}

fn build(benches: &[Benched], cwd: &Path) -> Result<Vec<PathBuf>> {
    benches.iter().map(|b| build_target(b.feature, b.bench, cwd)).collect()
}

fn run_once(feature: &str, exe: &Path, cwd: &Path) -> Result<Metrics> {
    let mut child = Command::new(exe)
        .current_dir(cwd)
        .env("DIFFPILOT_BENCH", "1")
        .env("DIFFPILOT_REPO", fixture::repo())
        .env("DIFFPILOT_VIEWED_SEED", fixture::viewed_seed())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let drain = |mut pipe: Box<dyn Read + Send>| {
        std::thread::spawn(move || {
            let mut text = String::new();
            let _ = pipe.read_to_string(&mut text);
            text
        })
    };
    let stdout = drain(Box::new(child.stdout.take().ok_or("no stdout")?));
    let stderr = drain(Box::new(child.stderr.take().ok_or("no stderr")?));
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if started.elapsed() > BENCH_TIMEOUT {
            let _ = child.kill();
            let _ = child.wait();
            let stderr = stderr.join().unwrap_or_default();
            return Err(format!(
                "{feature}: the bench stalled for {} s. Its window gets no frames while another window covers it; \
                 keep the bench window visible and run again. Its last output:\n{}",
                BENCH_TIMEOUT.as_secs(),
                tail(&stderr, 600)
            )
            .into());
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    let stdout = stdout.join().unwrap_or_default();
    let stderr = stderr.join().unwrap_or_default();
    if !status.success() {
        return Err(format!(
            "{feature}: bench run failed ({status}). Does it support DIFFPILOT_BENCH=1?\n{}",
            tail(&stderr, 2000)
        )
        .into());
    }
    parse_metrics(&stdout).map_err(|e| format!("{feature}: {e}").into())
}

fn tail(text: &str, bytes: usize) -> &str {
    let text = text.trim();
    &text[text.floor_char_boundary(text.len().saturating_sub(bytes))..]
}

fn refresh_rate() -> Result<String> {
    let out: Value = serde_json::from_str(&run(
        Command::new("system_profiler").args(["SPDisplaysDataType", "-json"])
    )?)?;
    let mut rates: Vec<String> = out["SPDisplaysDataType"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|gpu| gpu["spdisplays_ndrvs"].as_array().into_iter().flatten())
        .filter_map(|display| display["_spdisplays_resolution"].as_str())
        .filter_map(rate_of)
        .collect();
    rates.sort();
    rates.dedup();
    Ok(if rates.is_empty() {
        "unknown".into()
    } else {
        rates.join(", ")
    })
}

/// `"1470 x 956 @ 60.00Hz"` gives `"60.00Hz"`.
fn rate_of(resolution: &str) -> Option<String> {
    let after = resolution.split_once('@')?.1.trim_start();
    let number: String = after.chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
    let rest = &after[number.len()..];
    let space = if rest.starts_with(' ') { " " } else { "" };
    (!number.is_empty() && rest.trim_start().starts_with("Hz")).then(|| format!("{number}{space}Hz"))
}

fn host() -> Result<Map<String, Value>> {
    let text = |cmd: &mut Command| run(cmd).map(|s| s.trim().to_owned());
    let mut out = Map::new();
    out.insert(
        "cpu".into(),
        text(Command::new("sysctl").args(["-n", "machdep.cpu.brand_string"]))?.into(),
    );
    out.insert(
        "macos".into(),
        text(Command::new("sw_vers").arg("-productVersion"))?.into(),
    );
    out.insert("refresh_rate".into(), refresh_rate()?.into());
    out.insert("rustc".into(), text(Command::new("rustc").arg("-V"))?.into());
    out.insert("profile".into(), "release".into());
    Ok(out)
}

/// Timings taken on a busy machine are noise, so a busy machine skips the run instead.
fn busy(what: &str, force: bool) -> bool {
    let cores = std::thread::available_parallelism().map_or(1, usize::from);
    let load = run(Command::new("sysctl").args(["-n", "vm.loadavg"]))
        .ok()
        .and_then(|s| {
            s.trim_matches(|c: char| c == '{' || c == '}' || c.is_whitespace())
                .split_whitespace()
                .next()
                .and_then(|n| n.parse::<f64>().ok())
        });
    match load.filter(|load| *load > cores as f64 / 2.0) {
        Some(load) if !force => {
            eprintln!(
                "{what}: skipped, load average {load:.1} on {cores} cores. Run it on a quiet machine, or pass --force."
            );
            true
        }
        _ => false,
    }
}

fn chosen(names: &[String]) -> Result<Vec<Benched>> {
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    Ok(benched(resolve(&names)?))
}

pub fn baseline(rest: &[String]) -> Result {
    let (force, names) = take_flag(rest, "--force");
    let benches = chosen(&names)?;
    if benches.is_empty() {
        eprintln!("No selected feature has a bench.");
        return Ok(());
    }
    if busy("baseline", force) {
        return Ok(());
    }
    fixture::ensure(false)?;
    let exes = build(&benches, &root())?;
    let mut fresh: BTreeMap<&str, Metrics> = BTreeMap::new();
    for (b, exe) in benches.iter().zip(&exes) {
        eprintln!("== baseline {}", b.feature);
        fresh.insert(b.feature, run_once(b.feature, exe, &root())?);
    }
    let now = host()?;
    let path = root().join(BASELINE);
    let old: Value = std::fs::read_to_string(&path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or(Value::Null);
    let mut metrics: BTreeMap<String, Value> = BTreeMap::new();
    if old["host"] == Value::Object(now.clone())
        && let Some(kept) = old["metrics"].as_object()
    {
        let current: Vec<&str> = benched(resolve(&[])?).iter().map(|b| b.feature).collect();
        metrics.extend(
            kept.iter()
                .filter(|(name, _)| current.contains(&name.as_str()))
                .map(|(name, value)| (name.clone(), value.clone())),
        );
    }
    for (feature, values) in fresh {
        let values: BTreeMap<&String, Value> = values
            .iter()
            .map(|(name, v)| (name, json!((v * 1e4).round() / 1e4)))
            .collect();
        metrics.insert(feature.to_owned(), json!(values));
    }
    let data = json!({"host": now, "metrics": metrics});
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&path, serde_json::to_string_pretty(&data)? + "\n")?;
    eprintln!("wrote {BASELINE}; commit it with the PR");
    Ok(())
}

fn committed_baseline(reference: &str) -> std::result::Result<Value, String> {
    let out = Command::new("git")
        .args(["show", &format!("{reference}:{BASELINE}")])
        .current_dir(root())
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(format!("{reference} has no {BASELINE}"));
    }
    let data: Value = serde_json::from_slice(&out.stdout).map_err(|e| e.to_string())?;
    let now = host().map_err(|e| e.to_string())?;
    let differs: Vec<String> = now
        .iter()
        .filter(|(key, value)| &data["host"][key.as_str()] != *value)
        .map(|(key, value)| format!("{key} {} != {value}", data["host"][key.as_str()]))
        .collect();
    if !differs.is_empty() {
        return Err(format!(
            "the baseline on {reference} comes from another host or toolchain: {}",
            differs.join("; ")
        ));
    }
    Ok(data)
}

/// Prints one line per metric and returns how many are worse than their limit allows.
fn compare(feature: &str, head: &Metrics, base: &Metrics) -> usize {
    let mut worse = 0;
    for (name, &h) in head {
        let Some(&b) = base.get(name) else {
            eprintln!("  {feature} {name}: {h:.4} (new, no baseline)");
            continue;
        };
        let limit = limit(name).unwrap_or(Limit::Report);
        let change = if b != 0.0 { (h - b) / b.abs() } else { 0.0 };
        let regressed = limit.exceeded(b, h);
        worse += usize::from(regressed);
        let verdict = match (regressed, limit) {
            (true, _) => format!("REGRESSION (limit {})", limit.describe()),
            (false, Limit::Report) => "reported, no limit".into(),
            (false, _) => "ok".into(),
        };
        eprintln!(
            "  {feature} {name}: base {b:.4} -> head {h:.4} ({:+.1}%) {verdict}",
            change * 100.0
        );
    }
    worse
}

pub fn perf(rest: &[String]) -> Result {
    let (base, rest) = parse_option(rest, "--base")?;
    let (force, names) = take_flag(&rest, "--force");
    let benches = chosen(&names)?;
    if benches.is_empty() {
        eprintln!("No selected feature has a bench.");
        return Ok(());
    }
    if busy("perf", force) {
        return Ok(());
    }
    let reference = resolve_base(base);
    let data = match committed_baseline(&reference) {
        Ok(data) => data,
        Err(why) => {
            eprintln!("perf: skipped, {why}. Run cargo xtask baseline on this machine first.");
            return Ok(());
        }
    };
    fixture::ensure(false)?;
    eprintln!("== perf baseline: {BASELINE} on {reference}");
    let exes = build(&benches, &root())?;
    let mut regressions = 0;
    for (b, exe) in benches.iter().zip(&exes) {
        let Some(saved) = data["metrics"][b.feature].as_object() else {
            eprintln!(
                "perf {}: skipped, the baseline on {reference} has no {}",
                b.feature, b.feature
            );
            continue;
        };
        let base: Metrics = saved
            .iter()
            .filter_map(|(name, m)| Some((name.clone(), m.as_f64().or_else(|| m["median"].as_f64())?)))
            .collect();
        eprintln!("== perf {}", b.feature);
        regressions += compare(b.feature, &run_once(b.feature, exe, &root())?, &base);
    }
    if regressions > 0 {
        return Err(format!("{regressions} metrics are worse than their limit").into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_suffix_has_its_limit() {
        assert_eq!(limit("scroll_flick_nowrap_cpu_median_ms"), Some(Limit::Ratio(0.05)));
        assert_eq!(limit("idle_frame_p95_ms"), Some(Limit::Ratio(0.05)));
        assert_eq!(limit("scroll_flick_wrap_allocs"), Some(Limit::Ratio(0.05)));
        assert_eq!(limit("scroll_flick_wrap_alloc_bytes"), Some(Limit::Ratio(0.05)));
        assert_eq!(limit("stream_frame_peak_rss_bytes"), Some(Limit::Report));
        assert_eq!(limit("pane_retained_bytes"), Some(Limit::Bytes(65536.0)));
        assert_eq!(limit("idle_frame_max_ms"), None);
        assert_eq!(limit("scroll_frame_cpu_frames_over_8_33_ms"), None);
    }

    #[test]
    fn ratio_limits_are_relative_and_byte_limits_absolute() {
        let five = Limit::Ratio(0.05);
        assert!(!five.exceeded(100.0, 105.0));
        assert!(five.exceeded(100.0, 105.1));
        assert!(!five.exceeded(100.0, 50.0));
        let bytes = Limit::Bytes(1024.0);
        assert!(!bytes.exceeded(1_000_000.0, 1_001_024.0));
        assert!(bytes.exceeded(0.0, 1025.0));
        assert!(!Limit::Report.exceeded(1.0, 1e9));
    }

    #[test]
    fn metrics_flatten_with_underscores_and_keep_only_limited_names() {
        let stdout = r#"{"window_shown_ms": 12.5, "sidebar_rows": 10,
            "scroll_frame_cpu": {"frames": 3, "median_ms": 1.5, "p95_ms": 2.0, "max_ms": 9.0},
            "scroll_allocs": 42, "scroll_alloc_bytes": 4096, "scroll_peak_rss_bytes": 1e8,
            "pane_retained_bytes": -16, "sidebar_loaded_ms": null, "flag": true}"#;
        let metrics = parse_metrics(stdout).unwrap();
        let names: Vec<&str> = metrics.keys().map(String::as_str).collect();
        assert_eq!(
            names,
            [
                "pane_retained_bytes",
                "scroll_alloc_bytes",
                "scroll_allocs",
                "scroll_frame_cpu_median_ms",
                "scroll_frame_cpu_p95_ms",
                "scroll_peak_rss_bytes",
            ]
        );
        assert_eq!(metrics["pane_retained_bytes"], -16.0);
    }

    #[test]
    fn output_without_limited_metrics_is_an_error() {
        assert!(parse_metrics("not json").is_err());
        assert!(parse_metrics(r#"{"max_ms": 3}"#).is_err());
    }

    #[test]
    fn refresh_rate_comes_from_the_resolution_string() {
        assert_eq!(rate_of("1470 x 956 @ 60.00Hz").as_deref(), Some("60.00Hz"));
        assert_eq!(rate_of("3024 x 1964 @120 Hz").as_deref(), Some("120 Hz"));
        assert_eq!(rate_of("1920 x 1080"), None);
    }
}
