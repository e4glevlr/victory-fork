//! Generates a distilled pure-SVG status visualization for `README.md`,
//! centered on the weighted **Playable Readiness %** metric.
//!
//! Pure text XML (no binary data, no external fonts or scripts), so it passes
//! `docs/asset-policy.md` and renders crisply on GitHub in both light and dark
//! themes. Updated automatically on every commit via `.githooks/pre-commit`.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::fs;
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result};
use walkdir::WalkDir;

pub const DEFAULT_OUTPUT: &str = "docs/repo-visualization.svg";

const RECENT_COMMIT_WINDOW: usize = 60;
const FEED_COMMIT_COUNT: usize = 4;
const TARGET_FPS: f64 = 30.0;

#[derive(Debug, Clone)]
struct CrateStat {
    name: String,
    loc: usize,
}

#[derive(Debug, Clone)]
struct CommitEntry {
    hash: String,
    date: String,
    subject: String,
    bucket: CommitBucket,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CommitBucket {
    Shaders,
    Gpu,
    Other,
}

#[derive(Debug, Clone)]
struct ShaderCoverage {
    total_ok: usize,
    total_all: usize,
    compute_ok: usize,
    compute_all: usize,
    mesh_ok: usize,
    mesh_all: usize,
    pixel_ok: usize,
    pixel_all: usize,
    vertex_ok: usize,
    vertex_all: usize,
}

impl Default for ShaderCoverage {
    fn default() -> Self {
        Self {
            total_ok: 25_682,
            total_all: 25_953,
            compute_ok: 2_729,
            compute_all: 2_734,
            mesh_ok: 8_694,
            mesh_all: 8_765,
            pixel_ok: 12_067,
            pixel_all: 12_067,
            vertex_ok: 2_192,
            vertex_all: 2_192,
        }
    }
}

#[derive(Debug, Clone)]
struct PlayablePillar {
    name: &'static str,
    weight_pct: f64,
    completion_pct: f64,
    detail: String,
}

#[derive(Debug, Clone)]
struct PlayableScore {
    overall_pct: f64,
    current_fps: f64,
    lighting_live: bool,
    pillars: Vec<PlayablePillar>,
}

#[derive(Debug, Clone)]
struct Snapshot {
    branch: String,
    commit_count: usize,
    updated_date: String,
    total_loc: usize,
    unit_tests: usize,
    switch_count: usize,
    crates: Vec<CrateStat>,
    recent_commits: Vec<CommitEntry>,
    bucket_counts: [usize; 3],
    sampled_commits: usize,
    shader_coverage: ShaderCoverage,
    playable: PlayableScore,
}

pub fn run(root: &Path, output: &Path, pre_commit: bool, stage: bool) -> Result<()> {
    let snap = collect_snapshot(root, pre_commit);
    let svg = render_svg(&snap);

    let out_path = if output.is_absolute() {
        output.to_path_buf()
    } else {
        root.join(output)
    };
    if let Some(parent) = out_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    fs::write(&out_path, &svg).with_context(|| format!("writing {}", out_path.display()))?;

    let rel = out_path
        .strip_prefix(root)
        .unwrap_or(output)
        .to_string_lossy()
        .replace('\\', "/");
    let problems = crate::policy::check(&rel, svg.as_bytes());
    anyhow::ensure!(
        problems.is_empty(),
        "generated SVG failed asset policy: {}",
        problems.join("; ")
    );

    if stage {
        let status = Command::new("git")
            .args(["add", "--", &rel])
            .current_dir(root)
            .status()
            .context("staging generated SVG with git add")?;
        anyhow::ensure!(status.success(), "git add failed for {rel}");
    }

    println!(
        "viz: wrote {rel} ({} bytes, {:.1}% playable, {} LOC, {} commits, {} tests)",
        svg.len(),
        snap.playable.overall_pct,
        snap.total_loc,
        snap.commit_count,
        snap.unit_tests,
    );
    Ok(())
}

fn collect_snapshot(root: &Path, pre_commit: bool) -> Snapshot {
    let branch = git_stdout(root, &["rev-parse", "--abbrev-ref", "HEAD"])
        .filter(|s| !s.is_empty() && s != "HEAD")
        .or_else(|| git_stdout(root, &["rev-parse", "--short", "HEAD"]))
        .unwrap_or_else(|| "detached".to_owned());

    let head_commits = git_stdout(root, &["rev-list", "--count", "HEAD"])
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(0);
    let commit_count = if pre_commit && head_commits > 0 {
        head_commits + 1
    } else {
        head_commits
    };

    let updated_date = git_stdout(root, &["log", "-1", "--format=%ad", "--date=short"])
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".to_owned());

    let (crates, total_loc, unit_tests, switch_count) = scan_workspace(root);
    let recent_all = collect_recent_commits(root, RECENT_COMMIT_WINDOW);
    let sampled_commits = recent_all.len();

    let mut bucket_counts = [0usize; 3];
    for entry in &recent_all {
        let idx = match entry.bucket {
            CommitBucket::Shaders => 0,
            CommitBucket::Gpu => 1,
            CommitBucket::Other => 2,
        };
        bucket_counts[idx] += 1;
    }

    let recent_commits = recent_all.into_iter().take(FEED_COMMIT_COUNT).collect();
    let readme = fs::read_to_string(root.join("README.md")).unwrap_or_default();
    let shader_coverage = parse_shader_coverage(&readme).unwrap_or_default();
    let playable = compute_playable_score(&readme, &shader_coverage);

    Snapshot {
        branch,
        commit_count,
        updated_date,
        total_loc,
        unit_tests,
        switch_count,
        crates,
        recent_commits,
        bucket_counts,
        sampled_commits,
        shader_coverage,
        playable,
    }
}

fn compute_playable_score(readme: &str, sc: &ShaderCoverage) -> PlayableScore {
    let shader_pct = ratio_pct(sc.total_ok, sc.total_all);
    let current_fps = parse_latest_world_fps(readme).unwrap_or(9.6);
    let fps_pct = ((current_fps / TARGET_FPS) * 100.0).clamp(0.0, 100.0);

    let lower = readme.to_ascii_lowercase();
    let lighting_live =
        !lower.contains("shadowed areas are black") && !lower.contains("nothing is lit by it yet");
    let lighting_pct = if lighting_live { 85.0 } else { 25.0 };

    let world_rendered = lower.contains("first room now renders right");
    let world_pct = if world_rendered { 75.0 } else { 45.0 };

    let stability_pct = if lower.contains("levels on demand ran on the card") {
        60.0
    } else {
        45.0
    };

    let pillars = vec![
        PlayablePillar {
            name: "Boot, Menus & Saves",
            weight_pct: 15.0,
            completion_pct: 100.0,
            detail: "30 fps menus · cutscene ok".to_owned(),
        },
        PlayablePillar {
            name: "Shaders (SM 6.6 / Wave32)",
            weight_pct: 20.0,
            completion_pct: shader_pct,
            detail: format!("{} / {}", fmt_commas(sc.total_ok), fmt_commas(sc.total_all)),
        },
        PlayablePillar {
            name: "World Geometry & Scenes",
            weight_pct: 20.0,
            completion_pct: world_pct,
            detail: "First rooms ok · streaming WIP".to_owned(),
        },
        PlayablePillar {
            name: "Lighting & RT Shadows",
            weight_pct: 20.0,
            completion_pct: lighting_pct,
            detail: if lighting_live {
                "RT bounce & shadows live".to_owned()
            } else {
                "Blocker · shadow light off".to_owned()
            },
        },
        PlayablePillar {
            name: "Frame Rate (30 FPS Goal)",
            weight_pct: 15.0,
            completion_pct: fps_pct,
            detail: format!("{current_fps:.1} / {TARGET_FPS:.0} FPS"),
        },
        PlayablePillar {
            name: "Stability & 8–12 GB VRAM",
            weight_pct: 10.0,
            completion_pct: stability_pct,
            detail: "Rooms stable · open-world WIP".to_owned(),
        },
    ];

    let overall_pct: f64 = pillars
        .iter()
        .map(|p| (p.completion_pct * p.weight_pct) / 100.0)
        .sum();

    PlayableScore {
        overall_pct: overall_pct.clamp(0.0, 100.0),
        current_fps,
        lighting_live,
        pillars,
    }
}

fn parse_latest_world_fps(readme: &str) -> Option<f64> {
    let newest_idx = readme.find("### Newest")?;
    let section = &readme[newest_idx..];
    let window = section.get(..1200).unwrap_or(section);
    let pos = window.find(" frames a second")?;
    let tok = window[..pos]
        .split(|c: char| !c.is_ascii_digit() && c != '.')
        .rfind(|t| !t.is_empty() && t.chars().any(|c| c.is_ascii_digit()))?;
    tok.parse::<f64>().ok().filter(|v| *v > 0.0 && *v <= 240.0)
}

fn git_stdout(root: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

fn scan_workspace(root: &Path) -> (Vec<CrateStat>, usize, usize, usize) {
    let mut crates = Vec::new();
    let mut total_loc = 0usize;
    let mut unit_tests = 0usize;
    let mut switches = BTreeSet::new();

    let mut targets: Vec<(String, std::path::PathBuf)> = Vec::new();
    for group in ["crates", "apps"] {
        let dir = root.join(group);
        if let Ok(entries) = fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    let dir_name = entry.file_name().to_string_lossy().into_owned();
                    let label = if group == "apps" {
                        format!("apps/{dir_name}")
                    } else {
                        dir_name
                    };
                    targets.push((label, path));
                }
            }
        }
    }
    if root.join("xtask").is_dir() {
        targets.push(("xtask".to_owned(), root.join("xtask")));
    }

    for (name, path) in targets {
        let mut loc = 0usize;
        let mut found = false;
        for entry in WalkDir::new(&path)
            .into_iter()
            .filter_entry(|e| e.file_name().to_str() != Some("target"))
            .filter_map(Result::ok)
        {
            if !entry.file_type().is_file() {
                continue;
            }
            if entry.path().extension().and_then(|s| s.to_str()) != Some("rs") {
                continue;
            }
            let Ok(text) = fs::read_to_string(entry.path()) else {
                continue;
            };
            found = true;
            loc += text.lines().count();
            unit_tests += count_unit_tests(&text);
            extract_victory_switches(&text, &mut switches);
        }
        if found {
            total_loc += loc;
            crates.push(CrateStat { name, loc });
        }
    }

    crates.sort_by(|a, b| b.loc.cmp(&a.loc).then_with(|| a.name.cmp(&b.name)));
    (crates, total_loc, unit_tests, switches.len())
}

fn count_unit_tests(src: &str) -> usize {
    src.lines()
        .filter(|line| {
            let trimmed = line.trim();
            trimmed == "#[test]" || trimmed.starts_with("#[test]") || trimmed.contains("::test]")
        })
        .count()
}

fn extract_victory_switches(src: &str, out: &mut BTreeSet<String>) {
    let bytes = src.as_bytes();
    let prefix = b"VICTORY_";
    let mut i = 0;
    while i + prefix.len() < bytes.len() {
        if &bytes[i..i + prefix.len()] == prefix {
            let start = i;
            let mut end = i + prefix.len();
            while end < bytes.len()
                && (bytes[end].is_ascii_uppercase()
                    || bytes[end].is_ascii_digit()
                    || bytes[end] == b'_')
            {
                end += 1;
            }
            if end > start + prefix.len() {
                let candidate = &src[start..end];
                if !candidate.ends_with('_') {
                    out.insert(candidate.to_owned());
                }
            }
            i = end;
        } else {
            i += 1;
        }
    }
}

fn collect_recent_commits(root: &Path, limit: usize) -> Vec<CommitEntry> {
    let limit_arg = format!("-n{limit}");
    let Some(raw) = git_stdout(
        root,
        &[
            "log",
            &limit_arg,
            "--pretty=format:%h\x1f%ad\x1f%s",
            "--date=short",
        ],
    ) else {
        return Vec::new();
    };

    raw.lines()
        .filter_map(|line| {
            let mut parts = line.splitn(3, '\x1f');
            let hash = parts.next()?.trim().to_owned();
            let date = parts.next()?.trim().to_owned();
            let subject = parts.next()?.trim().to_owned();
            let bucket = classify_commit(&subject);
            Some(CommitEntry {
                hash,
                date,
                subject,
                bucket,
            })
        })
        .collect()
}

fn classify_commit(subject: &str) -> CommitBucket {
    let lower = subject.to_ascii_lowercase();
    if lower.starts_with("shaders:")
        || lower.contains("shader")
        || lower.contains("wave64")
        || lower.contains("wave32")
        || lower.contains("dxil")
        || lower.contains("lds ")
        || lower.contains("mesh ")
        || lower.contains("pixel ")
        || lower.contains("f64")
    {
        CommitBucket::Shaders
    } else if lower.starts_with("gpu:")
        || lower.contains("d3d12")
        || lower.contains("draw window")
        || lower.contains("pipeline")
        || lower.contains("texture")
        || lower.contains("card")
        || lower.contains("shadow")
        || lower.contains("cbv")
        || lower.contains("uav")
        || lower.contains("frame")
    {
        CommitBucket::Gpu
    } else {
        CommitBucket::Other
    }
}

fn parse_shader_coverage(readme: &str) -> Option<ShaderCoverage> {
    let line = readme
        .lines()
        .find(|l| l.contains("programs translate, validate and get a pipeline"))?;

    let (total_ok, total_all) = extract_pair_before(line, " programs translate")?;
    let (compute_ok, compute_all) = extract_pair_after(line, "compute ")?;
    let (mesh_ok, mesh_all) = extract_pair_after(line, "mesh ")?;
    let pixel = extract_single_after(line, "pixel all ")?;
    let vertex = extract_single_after(line, "vertex all ")?;

    Some(ShaderCoverage {
        total_ok,
        total_all,
        compute_ok,
        compute_all,
        mesh_ok,
        mesh_all,
        pixel_ok: pixel,
        pixel_all: pixel,
        vertex_ok: vertex,
        vertex_all: vertex,
    })
}

fn extract_pair_before(line: &str, marker: &str) -> Option<(usize, usize)> {
    let idx = line.find(marker)?;
    let mut nums = line[..idx]
        .split(|c: char| !c.is_ascii_digit() && c != ',')
        .filter(|tok| !tok.is_empty() && tok.chars().any(|c| c.is_ascii_digit()))
        .filter_map(parse_comma_uint)
        .collect::<Vec<_>>();
    let b = nums.pop()?;
    let a = nums.pop()?;
    Some((a, b))
}

fn extract_pair_after(line: &str, marker: &str) -> Option<(usize, usize)> {
    let idx = line.find(marker)?;
    let mut iter = line[idx + marker.len()..]
        .split(|c: char| !c.is_ascii_digit() && c != ',')
        .filter(|tok| !tok.is_empty() && tok.chars().any(|c| c.is_ascii_digit()))
        .filter_map(parse_comma_uint);
    Some((iter.next()?, iter.next()?))
}

fn extract_single_after(line: &str, marker: &str) -> Option<usize> {
    let idx = line.find(marker)?;
    line[idx + marker.len()..]
        .split(|c: char| !c.is_ascii_digit() && c != ',')
        .filter(|tok| !tok.is_empty() && tok.chars().any(|c| c.is_ascii_digit()))
        .find_map(parse_comma_uint)
}

fn parse_comma_uint(tok: &str) -> Option<usize> {
    let cleaned: String = tok.chars().filter(char::is_ascii_digit).collect();
    if cleaned.is_empty() {
        return None;
    }
    cleaned.parse::<usize>().ok()
}

fn ratio_pct(num: usize, den: usize) -> f64 {
    if den > 0 {
        ((num as f64 / den as f64) * 100.0).clamp(0.0, 100.0)
    } else {
        0.0
    }
}

fn fmt_k(n: usize) -> String {
    if n >= 1_000 {
        format!("{:.1}k", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

fn fmt_commas(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (idx, ch) in s.chars().enumerate() {
        if idx > 0 && (s.len() - idx).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

fn escape_xml(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            c if c.is_control() => out.push(' '),
            c => out.push(c),
        }
    }
    out
}

fn truncate_chars(s: &str, max_chars: usize) -> String {
    let count = s.chars().count();
    if count <= max_chars {
        return s.to_owned();
    }
    let keep = max_chars.saturating_sub(1);
    let mut out: String = s.chars().take(keep).collect();
    out.push('\u{2026}');
    out
}

fn crate_loc(snap: &Snapshot, name: &str) -> usize {
    snap.crates
        .iter()
        .find(|c| c.name == name)
        .map_or(0, |c| c.loc)
}

fn semantic_color(pct: f64) -> &'static str {
    if pct >= 85.0 {
        "#3fb950"
    } else if pct >= 50.0 {
        "#38bdf8"
    } else if pct >= 30.0 {
        "#d29922"
    } else {
        "#f85149"
    }
}

fn render_svg(snap: &Snapshot) -> String {
    let mut s = String::with_capacity(14_000);

    let playable_pct = snap.playable.overall_pct;
    let branch_s = escape_xml(&truncate_chars(&snap.branch, 22));
    let commits_s = fmt_commas(snap.commit_count);
    let updated_s = escape_xml(&snap.updated_date);

    let _ = writeln!(
        s,
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="1080" height="520" viewBox="0 0 1080 520" role="img" aria-label="VIctory Fork Playable Progress: {playable_pct:.1}%">"##
    );
    let _ = writeln!(
        s,
        r##"  <defs>
    <style>
      .sans {{ font-family: ui-sans-serif, -apple-system, BlinkMacSystemFont, "Segoe UI", Helvetica, Arial, sans-serif; }}
      .mono {{ font-family: ui-monospace, SFMono-Regular, "SF Mono", Menlo, Consolas, monospace; font-variant-numeric: tabular-nums; }}
      .fg   {{ fill: #e6edf3; }}
      .sec  {{ fill: #c9d1d9; }}
      .mut  {{ fill: #8b949e; }}
      .dim  {{ fill: #7d8590; }}
    </style>
  </defs>
  <rect x="0.5" y="0.5" width="1079" height="519" rx="12" fill="#0d1117" stroke="#21262d"/>"##
    );

    // =========================================================================
    // Band 1: Header Strip + Monumental Playable Readout + Weighted Master Bar
    // =========================================================================
    let _ = writeln!(
        s,
        r##"  <text x="36" y="42" font-size="15" font-weight="700" class="sans fg">VIctory Fork<tspan font-weight="400" class="mut" dx="10">Xbox Series → Host D3D12</tspan></text>
  <text x="1044" y="42" text-anchor="end" font-size="11.5" class="mono mut">{branch_s} · {commits_s} commits · {updated_s}</text>"##
    );

    // Monumental typographic Playable % centerpiece
    let blocker_text = if snap.playable.lighting_live && snap.playable.current_fps >= 25.0 {
        format!(
            "Lit in-world at {:.1} / 30 FPS — polishing stability",
            snap.playable.current_fps
        )
    } else if snap.playable.lighting_live {
        format!(
            "World lit at {:.1} / 30 FPS — focus on frame pacing",
            snap.playable.current_fps
        )
    } else {
        format!(
            "In-world renders at {:.1} / 30 FPS — blocked by ",
            snap.playable.current_fps
        )
    };
    let blocker_span = if snap.playable.lighting_live {
        ""
    } else {
        r##"<tspan fill="#f85149" font-weight="600">RT shadow lighting</tspan>"##
    };

    let _ = writeln!(
        s,
        r##"  <text x="36" y="118" font-size="54" font-weight="800" letter-spacing="-1.5" class="sans fg">{playable_pct:.1}%<tspan font-size="17" font-weight="500" letter-spacing="0" class="sans mut" dx="14">to playable</tspan></text>
  <text x="36" y="146" font-size="13.5" class="sans sec">{blocker_text}{blocker_span}</text>"##
    );

    // 5-stage milestone track (clean line + dots, no enclosing card)
    let _ = writeln!(
        s,
        r##"  <g>
    <line x1="650" y1="104" x2="1026" y2="104" stroke="#21262d" stroke-width="2"/>
    <line x1="650" y1="104" x2="838" y2="104" stroke="#3fb950" stroke-width="2"/>
    <line x1="838" y1="104" x2="932" y2="104" stroke="#38bdf8" stroke-width="2"/>
    <circle cx="650" cy="104" r="5" fill="#3fb950"/>
    <circle cx="744" cy="104" r="5" fill="#3fb950"/>
    <circle cx="838" cy="104" r="5" fill="#3fb950"/>
    <circle cx="932" cy="104" r="6.5" fill="#0d1117" stroke="#38bdf8" stroke-width="2.5"/>
    <circle cx="1026" cy="104" r="5" fill="#0d1117" stroke="#484f58" stroke-width="1.5"/>
    <text x="650" y="128" text-anchor="middle" font-size="11" class="sans mut">Boot</text>
    <text x="744" y="128" text-anchor="middle" font-size="11" class="sans mut">Menus</text>
    <text x="838" y="128" text-anchor="middle" font-size="11" class="sans mut">Cutscene</text>
    <text x="932" y="128" text-anchor="middle" font-size="11" font-weight="700" fill="#38bdf8" class="sans">In-World</text>
    <text x="1026" y="128" text-anchor="middle" font-size="11" class="sans dim">Playable</text>
  </g>"##
    );

    // Weighted Proportional Master Bar: each segment's width = pillar weight,
    // its colored fill = pillar completion, empty track = exact remaining gap.
    let gap = 6.0_f64;
    let total_bars = snap.playable.pillars.len();
    let usable_w = 1008.0 - gap * (total_bars.saturating_sub(1) as f64);
    let mut cursor_x = 36.0_f64;
    for p in &snap.playable.pillars {
        let seg_w = (p.weight_pct / 100.0) * usable_w;
        let fill_w = ((p.completion_pct / 100.0) * seg_w).clamp(3.0, seg_w);
        let color = semantic_color(p.completion_pct);
        let _ = writeln!(
            s,
            r##"  <rect x="{cursor_x:.1}" y="172" width="{seg_w:.1}" height="12" rx="3" fill="#1c2330"/>
  <rect x="{cursor_x:.1}" y="172" width="{fill_w:.1}" height="12" rx="3" fill="{color}"/>"##
        );
        cursor_x += seg_w + gap;
    }

    let _ = writeln!(
        s,
        r##"  <line x1="36" y1="206" x2="1044" y2="206" stroke="#21262d"/>"##
    );

    // =========================================================================
    // Band 2: Six Playable Pillars (flat 2-column typographic layout, no cards)
    // =========================================================================
    for (idx, p) in snap.playable.pillars.iter().enumerate() {
        let col = (idx % 2) as i32;
        let row = (idx / 2) as i32;
        let x = 36 + col * 528;
        let y = 236 + row * 46;
        let w = 480.0_f64;
        let fill_w = ((p.completion_pct / 100.0) * w).clamp(4.0, w);
        let color = semantic_color(p.completion_pct);
        let name = escape_xml(p.name);
        let detail = escape_xml(&p.detail);

        let _ = writeln!(
            s,
            r##"  <g>
    <text x="{x}" y="{y}" font-size="12.5" font-weight="600" class="sans fg">{name}<tspan font-size="11.5" font-weight="400" class="mut" dx="8">{detail}</tspan></text>
    <text x="{rx}" y="{y}" text-anchor="end" font-size="12" font-weight="700" fill="{color}" class="mono">{pct:.0}%<tspan font-size="10.5" font-weight="400" class="dim" dx="5">({wt:.0}% wt)</tspan></text>
    <rect x="{x}" y="{by}" width="{w:.0}" height="5" rx="2.5" fill="#1c2330"/>
    <rect x="{x}" y="{by}" width="{fill_w:.1}" height="5" rx="2.5" fill="{color}"/>
  </g>"##,
            rx = x + 480,
            by = y + 10,
            pct = p.completion_pct,
            wt = p.weight_pct,
        );
    }

    let _ = writeln!(
        s,
        r##"  <line x1="36" y1="368" x2="1044" y2="368" stroke="#21262d"/>"##
    );

    // =========================================================================
    // Band 3: Codebase & Shaders (Left) + Recent Commits (Right)
    // =========================================================================
    let gpu_loc = crate_loc(snap, "victory-gpu");
    let shader_loc = crate_loc(snap, "victory-shader");
    let hle_loc = crate_loc(snap, "victory-hle");

    let gpu_pct = ratio_pct(gpu_loc, snap.total_loc);
    let shader_pct = ratio_pct(shader_loc, snap.total_loc);
    let hle_pct = ratio_pct(hle_loc, snap.total_loc);
    let other_pct = (100.0 - gpu_pct - shader_pct - hle_pct).max(0.0);

    let loc_s = fmt_k(snap.total_loc);
    let tests_s = fmt_commas(snap.unit_tests);
    let _ = writeln!(
        s,
        r##"  <text x="36" y="396" font-size="12.5" font-weight="600" class="sans fg">Codebase &amp; Shaders</text>
  <text x="412" y="396" text-anchor="end" font-size="11" class="mono mut">{loc_s} LOC · {tests_s} tests</text>
  <clipPath id="locClip"><rect x="36" y="408" width="376" height="8" rx="4"/></clipPath>
  <g clip-path="url(#locClip)">"##
    );

    let shares = [
        (gpu_pct, "#38bdf8"),
        (shader_pct, "#79c0ff"),
        (hle_pct, "#3b5875"),
        (other_pct, "#263242"),
    ];
    let mut lx = 36.0_f64;
    for (pct, color) in shares {
        let seg = (pct / 100.0) * 376.0;
        let _ = writeln!(
            s,
            r##"    <rect x="{lx:.1}" y="408" width="{w:.1}" height="8" fill="{color}"/>"##,
            w = seg + 0.5,
        );
        lx += seg;
    }

    let sc = &snap.shader_coverage;
    let total_sh_pct = ratio_pct(sc.total_ok, sc.total_all);
    let ps_pct = ratio_pct(sc.pixel_ok, sc.pixel_all);
    let vs_pct = ratio_pct(sc.vertex_ok, sc.vertex_all);
    let cs_pct = ratio_pct(sc.compute_ok, sc.compute_all);
    let ms_pct = ratio_pct(sc.mesh_ok, sc.mesh_all);
    let sw_s = fmt_commas(snap.switch_count);

    let gpu_r = gpu_pct.round() as usize;
    let shader_r = shader_pct.round() as usize;
    let hle_r = hle_pct.round() as usize;
    let other_r = 100usize.saturating_sub(gpu_r + shader_r + hle_r);

    let _ = writeln!(
        s,
        r##"  </g>
  <text x="36" y="434" font-size="11" class="mono mut">gpu {gpu_r}% · shader {shader_r}% · hle {hle_r}% · other {other_r}%</text>
  <text x="36" y="464" font-size="11.5" font-weight="600" class="sans sec">SM 6.6 / Wave32</text>
  <text x="412" y="464" text-anchor="end" font-size="11" font-weight="600" fill="#3fb950" class="mono">{sh_ok} / {sh_all} ({total_sh_pct:.1}%)</text>
  <text x="36" y="484" font-size="11" class="mono dim">PS {ps_pct:.0}% · VS {vs_pct:.0}% · CS {cs_pct:.1}% · MS {ms_pct:.1}% · {sw_s} flags</text>"##,
        sh_ok = fmt_commas(sc.total_ok),
        sh_all = fmt_commas(sc.total_all),
    );

    // Vertical rule separating Codebase and Commit stream
    let _ = writeln!(
        s,
        r##"  <line x1="438" y1="384" x2="438" y2="492" stroke="#21262d"/>"##
    );

    // Right side: Recent Commits
    let s_sh_r = ratio_pct(snap.bucket_counts[0], snap.sampled_commits).round() as usize;
    let s_gpu_r = ratio_pct(snap.bucket_counts[1], snap.sampled_commits).round() as usize;
    let s_oth_r = 100usize.saturating_sub(s_sh_r + s_gpu_r);
    let _ = writeln!(
        s,
        r##"  <text x="464" y="396" font-size="12.5" font-weight="600" class="sans fg">Recent Commits</text>
  <text x="1044" y="396" text-anchor="end" font-size="11" class="mono mut">Last {n}: Shaders {s_sh_r}% · GPU {s_gpu_r}% · Other {s_oth_r}%</text>"##,
        n = snap.sampled_commits,
    );

    for (idx, commit) in snap.recent_commits.iter().enumerate() {
        let y = 421 + (idx as i32) * 22;
        let short_date = commit.date.get(5..).unwrap_or(&commit.date);
        let subj = escape_xml(&truncate_chars(&commit.subject, 64));
        let hash = escape_xml(&commit.hash);
        let date = escape_xml(short_date);
        let _ = writeln!(
            s,
            r##"  <text x="464" y="{y}" font-size="11" fill="#38bdf8" class="mono">{hash}</text>
  <text x="530" y="{y}" font-size="11" class="mono dim">{date}</text>
  <text x="576" y="{y}" font-size="12" class="sans sec">{subj}</text>"##
        );
    }

    let _ = writeln!(s, "</svg>");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xml_escape_handles_special_chars() {
        assert_eq!(
            escape_xml("a & <b \"c\" 'd'>"),
            "a &amp; &lt;b &quot;c&quot; &apos;d&apos;&gt;"
        );
    }

    #[test]
    fn generated_svg_passes_asset_policy() {
        let root = crate::workspace_root();
        let snap = collect_snapshot(&root, false);
        assert!(snap.playable.overall_pct > 0.0 && snap.playable.overall_pct <= 100.0);
        let svg = render_svg(&snap);
        assert!(svg.starts_with("<svg "));
        assert!(svg.trim_end().ends_with("</svg>"));
        let problems = crate::policy::check(DEFAULT_OUTPUT, svg.as_bytes());
        assert!(problems.is_empty(), "policy problems: {problems:?}");
    }

    #[test]
    fn parses_readme_shader_and_fps_numbers() {
        let readme = fs::read_to_string(crate::workspace_root().join("README.md")).unwrap();
        let cov = parse_shader_coverage(&readme)
            .expect("README.md shader coverage line should parse");
        assert!(cov.total_ok > 0 && cov.total_ok <= cov.total_all);
        assert!(cov.pixel_ok > 0 && cov.vertex_ok > 0);

        let fps = parse_latest_world_fps(&readme).expect("README.md world fps should parse");
        assert!(fps > 0.0 && fps <= 120.0);
    }
}
