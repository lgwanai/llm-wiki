//! Dream consolidation: non-blocking, cancellable memory maintenance.
//!
//! The worker follows the llm-wiki-skill four-phase dream model:
//! light sleep: query-driven entity/concept metadata optimization;
//! audit: multi-day query trend analysis;
//! purify: search simulation for duplicates and low-density pages;
//! enrich: research tasks for low-density high-frequency pages.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use crate::config::{get_pages_dir, get_wiki_dir};
use crate::error::{WikiError, WikiResult};
use crate::search;
use crate::types::{QueryAnswer, SearchResult};

const LOW_VALUE_QUERIES: &[&str] = &[
    "嗯", "哦", "好的", "好", "ok", "OK", "yes", "no", "thanks", "谢谢",
];
const AUDIT_WINDOW_DAYS: i64 = 7;
const TOP_N_AUDIT: usize = 50;
const TOP_N_PURIFY: usize = 10;
const TOP_N_ENRICH: usize = 10;
const MIN_QUERY_COUNT: usize = 2;
const LOW_DENSITY_THRESHOLD: usize = 1200;
const RANK_PRESERVATION_WEIGHT: f64 = 0.40;
const DENSITY_IMPROVEMENT_WEIGHT: f64 = 0.30;
const COVERAGE_SCORE_WEIGHT: f64 = 0.30;
const ROLLBACK_THRESHOLD: f64 = -0.15;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DreamOptions {
    pub foreground: bool,
    pub worker: bool,
    pub auto: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct DreamStatus {
    state: String,
    stage: String,
    pid: u32,
    started_at: String,
    updated_at: String,
    message: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct QueryLogEntry {
    timestamp: String,
    question: String,
    format: String,
    synthesis: bool,
    answer_chars: usize,
    sources: Vec<QuerySourceLog>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct QuerySourceLog {
    id: String,
    name: String,
    path: String,
    page_type: String,
    relevance: f64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct LightQuery {
    normalized: String,
    count: usize,
    examples: Vec<String>,
    sources: Vec<QuerySourceLog>,
    actions: Vec<String>,
}

#[derive(Debug, Clone)]
struct PageParts {
    frontmatter: serde_yaml::Mapping,
    body: String,
}

#[derive(Debug, Clone)]
struct PhaseRun {
    artifact: Option<PathBuf>,
    modified_paths: Vec<PathBuf>,
    summary: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct SearchSnapshot {
    id: String,
    rank: usize,
    score: f64,
}

#[derive(Debug, Clone)]
struct QualityBaseline {
    queries: Vec<String>,
    results: BTreeMap<String, Vec<SearchSnapshot>>,
    densities: BTreeMap<String, usize>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct QualityReport {
    overall_score: f64,
    recommendation: String,
    summary: String,
    rank_score: f64,
    density_score: f64,
    coverage_score: f64,
    per_query_scores: BTreeMap<String, f64>,
    rank_changes: BTreeMap<String, BTreeMap<String, (isize, isize)>>,
    density_changes: BTreeMap<String, isize>,
}

pub fn cancel_active_dream(reason: &str) {
    let dir = dream_dir();
    if fs::create_dir_all(&dir).is_ok() {
        let payload = serde_json::json!({
            "reason": reason,
            "timestamp": chrono::Utc::now().to_rfc3339(),
        });
        let _ = fs::write(
            dir.join("cancel.flag"),
            serde_json::to_string_pretty(&payload).unwrap(),
        );
    }
    terminate_running_worker(&dir);
}

pub fn log_query(answer: &QueryAnswer, synthesis: bool) -> WikiResult<()> {
    let audit_dir = get_wiki_dir().join("audit");
    fs::create_dir_all(&audit_dir)?;
    let date = chrono::Local::now().format("%Y%m%d").to_string();
    let path = audit_dir.join(format!("query-log-{date}.jsonl"));
    let entry = QueryLogEntry {
        timestamp: chrono::Utc::now().to_rfc3339(),
        question: answer.question.clone(),
        format: answer.format.clone(),
        synthesis,
        answer_chars: answer.answer.chars().count(),
        sources: answer
            .sources
            .iter()
            .map(|s| QuerySourceLog {
                id: s.id.clone(),
                name: s.name.clone(),
                path: s.path.clone(),
                page_type: s.page_type.clone(),
                relevance: s.relevance,
            })
            .collect(),
    };
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;
    writeln!(file, "{}", serde_json::to_string(&entry)?)?;
    Ok(())
}

pub fn start_dream(options: DreamOptions) -> WikiResult<String> {
    if options.worker || options.foreground {
        run_worker(options.auto)
    } else {
        start_background_worker(options.auto)
    }
}

fn start_background_worker(auto: bool) -> WikiResult<String> {
    let dir = dream_dir();
    fs::create_dir_all(&dir)?;
    let _ = fs::remove_file(dir.join("cancel.flag"));
    let exe = std::env::current_exe()?;
    let stdout = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("dream.out.log"))?;
    let stderr = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("dream.err.log"))?;
    let mut cmd = Command::new(exe);
    cmd.arg("dream").arg("--worker");
    if auto {
        cmd.arg("--auto");
    }
    let child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .spawn()?;
    Ok(format!(
        "Dream started in background (pid {}). Status: {}",
        child.id(),
        dir.join("status.json").display()
    ))
}

fn run_worker(auto: bool) -> WikiResult<String> {
    let dir = dream_dir();
    fs::create_dir_all(&dir)?;
    let _ = fs::remove_file(dir.join("cancel.flag"));
    let _ = fs::remove_file(dir.join("status.json"));
    write_status("running", "start", "Dream worker started")?;
    ensure_wiki_git_repo()?;
    let experience = read_dream_experience();
    write_dream_context(&experience)?;
    let quality_queries = collect_quality_queries(TOP_N_AUDIT);

    check_cancelled()?;
    let light_run = run_guarded_phase("light", &quality_queries, || {
        let (queries, modified_paths) = light_sleep()?;
        Ok(PhaseRun {
            artifact: Some(dream_dir().join(format!("{}-light.json", today()))),
            modified_paths,
            summary: format!("{} query themes", queries.len()),
        })
    })?;
    pause_for_interrupt_window()?;

    check_cancelled()?;
    let audit_path = phase_audit()?;
    pause_for_interrupt_window()?;

    check_cancelled()?;
    let purify_run = run_guarded_phase("purify", &quality_queries, || phase_purify(auto))?;
    pause_for_interrupt_window()?;

    check_cancelled()?;
    let enrich_run = run_guarded_phase("enrich", &quality_queries, || phase_enrich(auto))?;
    create_snapshot("post-run")?;

    write_status(
        "complete",
        "done",
        &format!(
            "Dream complete. Light {}, audit: {}, purify: {}, enrich: {}",
            light_run.summary,
            audit_path.display(),
            purify_run.summary,
            enrich_run.summary
        ),
    )?;
    Ok("Dream complete".to_string())
}

fn light_sleep() -> WikiResult<(Vec<LightQuery>, Vec<PathBuf>)> {
    write_status(
        "running",
        "light",
        "Light Sleep: optimize entity/concept metadata from query behavior",
    )?;
    let entries = read_today_query_log()?;
    let mut grouped: BTreeMap<String, LightQuery> = BTreeMap::new();
    for entry in entries {
        check_cancelled()?;
        let normalized = normalize_query(&entry.question);
        if normalized.is_empty() || is_low_value_query(&normalized) {
            continue;
        }
        let item = grouped.entry(normalized.clone()).or_insert(LightQuery {
            normalized,
            count: 0,
            examples: Vec::new(),
            sources: Vec::new(),
            actions: Vec::new(),
        });
        item.count += 1;
        if item.examples.len() < 5 && !item.examples.contains(&entry.question) {
            item.examples.push(entry.question);
        }
        item.sources.extend(entry.sources);
    }

    let mut queries: Vec<LightQuery> = grouped.into_values().collect();
    queries.sort_by(|a, b| {
        b.count
            .cmp(&a.count)
            .then_with(|| a.normalized.cmp(&b.normalized))
    });
    for q in &mut queries {
        dedupe_sources(&mut q.sources);
    }
    let modified_paths = apply_light_optimizations(&mut queries)?;

    let path = dream_dir().join(format!("{}-light.json", today()));
    fs::write(&path, serde_json::to_string_pretty(&queries)?)?;
    Ok((queries, modified_paths))
}

fn apply_light_optimizations(queries: &mut [LightQuery]) -> WikiResult<Vec<PathBuf>> {
    let streams: HashSet<String> = ["metadata", "bm25", "graph"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let mut touched_pages: HashSet<PathBuf> = HashSet::new();
    let mut unresolved = Vec::new();

    for query in queries {
        check_cancelled()?;
        if query.count == 0 {
            continue;
        }

        let mut candidates = query.sources.clone();
        if candidates.is_empty() {
            candidates = search::search(&query.normalized, &streams, 5)
                .into_iter()
                .map(source_from_search_result)
                .collect();
        }
        dedupe_sources(&mut candidates);

        let Some(target) = candidates
            .iter()
            .filter(|s| page_path_is_editable(&s.path))
            .max_by(|a, b| {
                a.relevance
                    .partial_cmp(&b.relevance)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .cloned()
        else {
            unresolved.push(query.normalized.clone());
            query
                .actions
                .push("no editable target page found; queued for split concept review".into());
            continue;
        };

        let added = optimize_page_for_query(&target, query)?;
        if added > 0 {
            touched_pages.insert(PathBuf::from(&target.path));
            query.actions.push(format!(
                "updated {} metadata with {} query-derived terms",
                target.id, added
            ));
        } else {
            query
                .actions
                .push(format!("{} already covered this query", target.id));
        }

        if should_create_split_concept(query, &target, &candidates) {
            let path = create_split_concept(query, &target, &candidates)?;
            touched_pages.insert(path.clone());
            query
                .actions
                .push(format!("created split concept {}", path.display()));
        }
    }

    let report_path = dream_dir().join(format!("{}-light-actions.md", today()));
    let mut report = String::from("# Dream Light Sleep Actions\n\n");
    report.push_str("## Metadata Updates\n\n");
    if touched_pages.is_empty() {
        report.push_str("- No entity/concept metadata updates were needed.\n");
    } else {
        let mut pages: Vec<_> = touched_pages.iter().collect();
        pages.sort();
        for page in pages {
            report.push_str(&format!("- `{}`\n", page.display()));
        }
    }
    if !unresolved.is_empty() {
        report.push_str("\n## Unresolved Query Themes\n\n");
        for query in unresolved {
            report.push_str(&format!("- {query}\n"));
        }
    }
    fs::write(report_path, report)?;
    Ok(touched_pages.into_iter().collect())
}

fn phase_audit() -> WikiResult<PathBuf> {
    write_status(
        "running",
        "audit",
        &format!("Phase 2/4: Audit — analysing {AUDIT_WINDOW_DAYS}d query trends"),
    )?;
    let entries = read_recent_query_logs(AUDIT_WINDOW_DAYS)?;
    let output = dream_dir().join(format!("{}-audit.md", today()));
    if entries.is_empty() {
        fs::write(
            &output,
            format!("# Dream Audit\n\nNo queries recorded in the last {AUDIT_WINDOW_DAYS} days.\n"),
        )?;
        return Ok(output);
    }

    let mut query_counts: BTreeMap<String, (usize, String, Vec<QuerySourceLog>, Vec<String>)> =
        BTreeMap::new();
    for entry in entries {
        check_cancelled()?;
        let raw = entry.question.trim().to_string();
        let norm = normalize_query(&raw);
        if norm.is_empty() || is_low_value_query(&norm) {
            continue;
        }
        let slot = query_counts
            .entry(norm)
            .or_insert((0, String::new(), Vec::new(), Vec::new()));
        slot.0 += 1;
        slot.1 = entry.timestamp;
        slot.2.extend(entry.sources);
        if slot.3.len() < 5 && !slot.3.contains(&raw) {
            slot.3.push(raw);
        }
    }

    let mut ranked: Vec<_> = query_counts.into_iter().collect();
    ranked.sort_by(|(a_query, (a_count, ..)), (b_query, (b_count, ..))| {
        b_count.cmp(a_count).then_with(|| a_query.cmp(b_query))
    });
    let top = ranked.into_iter().take(TOP_N_AUDIT).collect::<Vec<_>>();

    let mut blocks = Vec::new();
    for (idx, (query, (count, latest, sources, examples))) in top.iter().enumerate() {
        let mut source_ids = sources
            .iter()
            .map(|s| s.id.clone())
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>();
        source_ids.sort();
        source_ids.dedup();
        blocks.push(format!(
            "### Q{}: [{}x] {}\n\nLast seen: {}\nExamples: {}\nTop sources: {}\n",
            idx + 1,
            count,
            query,
            latest,
            examples
                .iter()
                .map(|s| format!("`{s}`"))
                .collect::<Vec<_>>()
                .join(", "),
            if source_ids.is_empty() {
                "none".to_string()
            } else {
                source_ids
                    .iter()
                    .take(10)
                    .map(|s| format!("[[{s}]]"))
                    .collect::<Vec<_>>()
                    .join(", ")
            }
        ));
    }

    fs::write(
        &output,
        format!(
            "# Dream Audit — {} ({AUDIT_WINDOW_DAYS}d window)\n\n> Phase 2/4: {} query themes shown.\n\n## Agent Task: Semantic Query Analysis\n\n1. Merge semantically similar queries into canonical information needs.\n2. Identify the top 10 recurring user needs by frequency and business value.\n3. Flag knowledge gaps where results depend on a single thin page or miss obvious entities.\n4. Do not modify pages in this phase; write analysis to `{}-audit-analysis.md`.\n\n## Raw Query Data\n\n{}\n",
            today(),
            top.len(),
            today(),
            blocks.join("\n")
        ),
    )?;
    Ok(output)
}

fn phase_purify(auto: bool) -> WikiResult<PhaseRun> {
    write_status(
        "running",
        "purify",
        "Phase 3/4: Purify — simulating searches for duplicates and low-density pages",
    )?;
    let entries = read_recent_query_logs(AUDIT_WINDOW_DAYS)?;
    let candidates = recurring_queries(&entries, TOP_N_PURIFY);
    let output = dream_dir().join(format!("{}-purify.md", today()));
    if candidates.is_empty() {
        fs::write(
            &output,
            "# Dream Purify\n\nInsufficient recurring queries to analyse.\n",
        )?;
        return Ok(PhaseRun {
            artifact: Some(output),
            modified_paths: Vec::new(),
            summary: "no recurring queries".into(),
        });
    }

    let streams: HashSet<String> = ["metadata", "bm25", "graph"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let mut duplicate_sections = Vec::new();
    let mut duplicate_pairs: Vec<(QuerySourceLog, QuerySourceLog, String, usize)> = Vec::new();
    let mut low_density: BTreeMap<String, (QuerySourceLog, usize, Vec<String>)> = BTreeMap::new();

    for (query, count) in &candidates {
        check_cancelled()?;
        let results = search::search(query, &streams, 8);
        let mut signatures: HashMap<String, QuerySourceLog> = HashMap::new();
        for result in results {
            let source = source_from_search_result(result);
            if !page_path_is_editable(&source.path) {
                continue;
            }
            let density = page_density(&source.path);
            if density > 0 && density < LOW_DENSITY_THRESHOLD {
                let slot = low_density.entry(source.id.clone()).or_insert((
                    source.clone(),
                    density,
                    Vec::new(),
                ));
                slot.1 = slot.1.min(density);
                slot.2.push(query.clone());
            }
            if let Some(sig) = page_signature(&source.path) {
                if let Some(original) = signatures.get(&sig) {
                    duplicate_sections.push(format!(
                        "- Query `{query}` ({count}x): [[{}]] appears duplicate of [[{}]]",
                        source.id, original.id
                    ));
                    duplicate_pairs.push((source.clone(), original.clone(), query.clone(), *count));
                } else {
                    signatures.insert(sig, source.clone());
                }
            }
        }
    }

    let mut modified_paths = Vec::new();
    let mut merged_count = 0usize;
    for (duplicate, survivor, query, _count) in &duplicate_pairs {
        check_cancelled()?;
        let changed = merge_duplicate_pages(duplicate, survivor, query)?;
        if !changed.is_empty() {
            merged_count += 1;
            modified_paths.extend(changed);
        }
    }
    dedupe_paths(&mut modified_paths);

    let mut report = format!(
        "# Dream Purify — {}\n\nAnalysed {} recurring queries.\n\n",
        today(),
        candidates.len()
    );
    report.push_str("## Duplicate Content Detected\n\n");
    if duplicate_sections.is_empty() {
        report.push_str("- No duplicate content detected by local simulation.\n\n");
    } else {
        report.push_str("> Dream performed mechanical merges for duplicates that shared the same local body signature. Quality gate may rollback if retrieval degrades.\n\n");
        report.push_str(&duplicate_sections.join("\n"));
        report.push_str("\n\n");
    }

    report.push_str("## Low-Density Pages\n\n");
    if low_density.is_empty() {
        report.push_str("- No low-density pages found among recurring query results.\n\n");
    } else {
        report.push_str("| Page | Density | Queried By |\n|------|---------|------------|\n");
        for (source, density, queries) in low_density.values() {
            let mut qs = queries.clone();
            qs.sort();
            qs.dedup();
            report.push_str(&format!(
                "| [[{}]] {} | {} | {} |\n",
                source.id,
                source.name,
                density,
                qs.into_iter().take(3).collect::<Vec<_>>().join(", ")
            ));
        }
        report.push('\n');
    }

    report.push_str("## Executed Changes\n\n");
    report.push_str(&format!(
        "- Mechanical duplicate merges applied: {merged_count}\n"
    ));
    if modified_paths.is_empty() {
        report.push_str("- No page content changed in this phase.\n");
    } else {
        for path in &modified_paths {
            report.push_str(&format!("- `{}`\n", path.display()));
        }
    }
    report.push_str("\n## Safety\n\nQuality gate runs immediately after this phase. Significant retrieval degradation triggers git rollback and experience logging.\n");
    if auto {
        report.push_str("\n## Auto Mode\n\nAuto mode is enabled; Purify still limits itself to deterministic duplicate redirects and merge markers.\n");
    }
    fs::write(&output, report)?;
    Ok(PhaseRun {
        artifact: Some(output),
        modified_paths,
        summary: format!("{merged_count} merges"),
    })
}

fn phase_enrich(auto: bool) -> WikiResult<PhaseRun> {
    write_status(
        "running",
        "enrich",
        "Phase 4/4: Enrich — identifying research targets",
    )?;
    let entries = read_recent_query_logs(AUDIT_WINDOW_DAYS)?;
    let mut page_stats: BTreeMap<String, (usize, QuerySourceLog, Vec<String>)> = BTreeMap::new();
    for entry in entries {
        let norm = normalize_query(&entry.question);
        for source in entry.sources {
            if source.path.is_empty() {
                continue;
            }
            let slot =
                page_stats
                    .entry(source.path.clone())
                    .or_insert((0, source.clone(), Vec::new()));
            slot.0 += 1;
            if !norm.is_empty() && !slot.2.contains(&norm) {
                slot.2.push(norm.clone());
            }
        }
    }

    let mut candidates = Vec::new();
    for (count, source, queries) in page_stats.into_values() {
        if count < MIN_QUERY_COUNT {
            continue;
        }
        let density = page_density(&source.path);
        if density > 0 && density < LOW_DENSITY_THRESHOLD {
            candidates.push((count, density, source, queries));
        }
    }
    candidates.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    candidates.truncate(TOP_N_ENRICH);

    let output = dream_dir().join(format!("{}-enrich.md", today()));
    if candidates.is_empty() {
        fs::write(
            &output,
            "# Dream Enrich\n\nNo low-density + high-frequency pages found.\n",
        )?;
        return Ok(PhaseRun {
            artifact: Some(output),
            modified_paths: Vec::new(),
            summary: "no enrichment candidates".into(),
        });
    }

    let mut blocks = Vec::new();
    let mut modified_paths = Vec::new();
    let queue_path = dream_dir().join("research-queue.jsonl");
    let mut queue = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&queue_path)?;
    for (idx, (count, density, source, queries)) in candidates.iter().enumerate() {
        check_cancelled()?;
        blocks.push(format!(
            "### E{}: [[{}]]\n\n- Page: {}\n- Queried: {}x in {}d\n- Density: {} chars\n- Query intents: {}\n- Path: `{}`\n",
            idx + 1,
            source.id,
            source.name,
            count,
            AUDIT_WINDOW_DAYS,
            density,
            queries.join(", "),
            source.path
        ));
        let task = serde_json::json!({
            "timestamp": chrono::Utc::now().to_rfc3339(),
            "phase": "enrich",
            "reason": "low-density high-frequency page",
            "query_count": count,
            "density_chars": density,
            "source": source,
            "query_intents": queries,
            "suggested_tool": "deerflow-skill or deep-research",
            "research_query": format!("Research and enrich {}", source.name),
        });
        writeln!(queue, "{}", serde_json::to_string(&task)?)?;
        let changed = enrich_page_from_queries(source, *count, *density, queries)?;
        if let Some(path) = changed {
            modified_paths.push(path);
        }
        if auto {
            maybe_run_research_hook(&task)?;
        }
    }
    dedupe_paths(&mut modified_paths);

    fs::write(
        &output,
        format!(
            "# Dream Enrich — {}\n\n> Phase 4/4: {} candidates. Dream directly enriched {} pages, then queued deeper research.\n\n## Executed Page Enrichment\n\n{}\n\n## Research Queue\n\nFor each candidate below, deerflow-skill or deep-research can add NEW draft pages with `confidence: 0.5`, `status: draft`, and `source: web-research`.\n\n{}\n\n## Constraints\n\n- Max {} enrichment targets per dream run.\n- Existing page edits are deterministic query-derived metadata/body maintenance and are protected by the quality gate.\n",
            today(),
            candidates.len(),
            modified_paths.len(),
            modified_paths
                .iter()
                .map(|p| format!("- `{}`", p.display()))
                .collect::<Vec<_>>()
                .join("\n"),
            blocks.join("\n"),
            TOP_N_ENRICH
        ),
    )?;
    Ok(PhaseRun {
        artifact: Some(output),
        modified_paths,
        summary: format!("{} enriched pages", candidates.len()),
    })
}

fn run_guarded_phase<F>(label: &str, quality_queries: &[String], run: F) -> WikiResult<PhaseRun>
where
    F: FnOnce() -> WikiResult<PhaseRun>,
{
    write_status(
        "running",
        label,
        &format!("Dream phase `{label}`: creating git snapshot and search baseline"),
    )?;
    let snapshot = create_snapshot(&format!("pre-{label}"))?;
    let baseline = QualityBaseline::capture(quality_queries);
    let phase_run = run()?;
    if phase_run.modified_paths.is_empty() {
        append_experience(label, "noop", "no page content changed", None)?;
        return Ok(phase_run);
    }

    let report = assess_quality(&baseline, &phase_run.modified_paths);
    write_quality_report(label, &report, &phase_run)?;
    if report.recommendation == "rollback" {
        rollback_to_snapshot(&snapshot, label)?;
        append_experience(label, "rollback", &report.summary, Some(&report))?;
        write_status(
            "running",
            label,
            &format!("Dream phase `{label}` rolled back: {}", report.summary),
        )?;
        return Ok(PhaseRun {
            artifact: phase_run.artifact,
            modified_paths: Vec::new(),
            summary: format!("rolled back: {}", report.summary),
        });
    }

    create_snapshot(&format!("post-{label}"))?;
    append_experience(
        label,
        &report.recommendation,
        &report.summary,
        Some(&report),
    )?;
    Ok(phase_run)
}

impl QualityBaseline {
    fn capture(queries: &[String]) -> Self {
        Self {
            queries: queries.to_vec(),
            results: run_search_snapshots(queries),
            densities: collect_page_densities(),
        }
    }
}

fn assess_quality(baseline: &QualityBaseline, modified_paths: &[PathBuf]) -> QualityReport {
    if baseline.queries.is_empty() {
        return QualityReport {
            overall_score: 0.0,
            recommendation: "keep".into(),
            summary: "No recurring queries available; kept deterministic dream changes.".into(),
            rank_score: 0.0,
            density_score: 0.0,
            coverage_score: 0.0,
            per_query_scores: BTreeMap::new(),
            rank_changes: BTreeMap::new(),
            density_changes: BTreeMap::new(),
        };
    }

    let current = run_search_snapshots(&baseline.queries);
    let (rank_score, rank_changes, per_query_scores) =
        compute_rank_score(&baseline.queries, &baseline.results, &current);
    let coverage_score = compute_coverage_score(&baseline.queries, &baseline.results, &current);
    let (density_score, density_changes) =
        compute_density_score(&baseline.densities, modified_paths);
    let overall = RANK_PRESERVATION_WEIGHT * rank_score
        + DENSITY_IMPROVEMENT_WEIGHT * density_score
        + COVERAGE_SCORE_WEIGHT * coverage_score;
    let overall_score = round3(overall);
    let (recommendation, summary) = if overall_score >= 0.0 {
        (
            "keep".to_string(),
            format!("Quality stable or improved (score {overall_score:+.3})."),
        )
    } else if overall_score >= ROLLBACK_THRESHOLD {
        (
            "warn".to_string(),
            format!("Minor retrieval degradation kept for now (score {overall_score:+.3})."),
        )
    } else {
        (
            "rollback".to_string(),
            format!("Significant retrieval degradation (score {overall_score:+.3})."),
        )
    };

    QualityReport {
        overall_score,
        recommendation,
        summary,
        rank_score: round3(rank_score),
        density_score: round3(density_score),
        coverage_score: round3(coverage_score),
        per_query_scores,
        rank_changes,
        density_changes,
    }
}

fn run_search_snapshots(queries: &[String]) -> BTreeMap<String, Vec<SearchSnapshot>> {
    let streams: HashSet<String> = ["metadata", "bm25", "graph"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let mut snapshots = BTreeMap::new();
    for query in queries {
        let rows = search::search(query, &streams, 10)
            .into_iter()
            .enumerate()
            .map(|(idx, result)| SearchSnapshot {
                id: result.id,
                rank: idx + 1,
                score: result.rrf_score.unwrap_or(result.score),
            })
            .collect();
        snapshots.insert(query.clone(), rows);
    }
    snapshots
}

fn compute_rank_score(
    queries: &[String],
    baseline: &BTreeMap<String, Vec<SearchSnapshot>>,
    current: &BTreeMap<String, Vec<SearchSnapshot>>,
) -> (
    f64,
    BTreeMap<String, BTreeMap<String, (isize, isize)>>,
    BTreeMap<String, f64>,
) {
    let mut all_deltas = Vec::new();
    let mut rank_changes = BTreeMap::new();
    let mut per_query_scores = BTreeMap::new();

    for query in queries {
        let old = baseline.get(query).cloned().unwrap_or_default();
        let new = current.get(query).cloned().unwrap_or_default();
        let mut new_ranks: HashMap<String, usize> = HashMap::new();
        for item in &new {
            new_ranks.insert(item.id.clone(), item.rank);
        }
        let mut query_changes = BTreeMap::new();
        let mut query_deltas = Vec::new();
        for item in old {
            let old_rank = item.rank as isize;
            let new_rank = new_ranks.get(&item.id).map(|r| *r as isize).unwrap_or(-1);
            let delta = if new_rank < 0 {
                -1.0
            } else if new_rank < old_rank {
                1.0
            } else if new_rank > old_rank {
                -1.0
            } else {
                0.0
            };
            all_deltas.push(delta);
            query_deltas.push(delta);
            query_changes.insert(item.id, (old_rank, new_rank));
        }
        if !query_changes.is_empty() {
            rank_changes.insert(query.clone(), query_changes);
        }
        let score = if query_deltas.is_empty() {
            0.0
        } else {
            query_deltas.iter().sum::<f64>() / query_deltas.len() as f64
        };
        per_query_scores.insert(query.clone(), round3(score));
    }

    let score = if all_deltas.is_empty() {
        0.0
    } else {
        (all_deltas.iter().sum::<f64>() / all_deltas.len() as f64).clamp(-1.0, 1.0)
    };
    (score, rank_changes, per_query_scores)
}

fn compute_coverage_score(
    queries: &[String],
    baseline: &BTreeMap<String, Vec<SearchSnapshot>>,
    current: &BTreeMap<String, Vec<SearchSnapshot>>,
) -> f64 {
    let mut old_ids = HashSet::new();
    let mut new_ids = HashSet::new();
    for query in queries {
        for item in baseline.get(query).into_iter().flatten() {
            old_ids.insert(item.id.clone());
        }
        for item in current.get(query).into_iter().flatten() {
            new_ids.insert(item.id.clone());
        }
    }
    if old_ids.is_empty() {
        return 0.0;
    }
    let still_findable = old_ids.intersection(&new_ids).count() as f64;
    ((still_findable / old_ids.len() as f64) * 2.0) - 1.0
}

fn compute_density_score(
    baseline: &BTreeMap<String, usize>,
    modified_paths: &[PathBuf],
) -> (f64, BTreeMap<String, isize>) {
    let mut deltas = Vec::new();
    let mut changes = BTreeMap::new();
    for path in modified_paths {
        let key = canonical_string(path);
        let old = baseline.get(&key).copied().unwrap_or(0) as isize;
        let new = page_density(&key) as isize;
        let delta = new - old;
        changes.insert(key, delta);
        let score = if delta > 0 {
            (delta as f64 / LOW_DENSITY_THRESHOLD as f64).min(1.0)
        } else if delta < 0 {
            -((-delta) as f64 / LOW_DENSITY_THRESHOLD as f64).min(1.0)
        } else {
            0.0
        };
        deltas.push(score);
    }
    let score = if deltas.is_empty() {
        0.0
    } else {
        deltas.iter().sum::<f64>() / deltas.len() as f64
    };
    (score.clamp(-1.0, 1.0), changes)
}

fn write_quality_report(label: &str, report: &QualityReport, phase: &PhaseRun) -> WikiResult<()> {
    let path = dream_dir().join(format!("{}-{label}-quality.json", today()));
    let payload = serde_json::json!({
        "phase": label,
        "artifact": phase.artifact.as_ref().map(|p| p.display().to_string()),
        "modified_paths": phase.modified_paths.iter().map(|p| p.display().to_string()).collect::<Vec<_>>(),
        "quality": report,
    });
    fs::write(path, serde_json::to_string_pretty(&payload)?)?;
    Ok(())
}

fn collect_quality_queries(limit: usize) -> Vec<String> {
    let entries = read_recent_query_logs(AUDIT_WINDOW_DAYS).unwrap_or_default();
    recurring_queries(&entries, limit)
        .into_iter()
        .map(|(query, _)| query)
        .collect()
}

fn collect_page_densities() -> BTreeMap<String, usize> {
    let mut densities = BTreeMap::new();
    for path in collect_page_paths() {
        let key = canonical_string(&path);
        densities.insert(key.clone(), page_density(&key));
    }
    densities
}

fn collect_page_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let roots = [
        "concepts",
        "entities",
        "models",
        "techniques",
        "frameworks",
        "benchmarks",
        "papers",
        "decisions",
        "sessions",
        "patterns",
    ];
    for root in roots {
        let dir = get_pages_dir().join(root);
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("md") {
                    paths.push(path);
                }
            }
        }
    }
    paths
}

fn merge_duplicate_pages(
    duplicate: &QuerySourceLog,
    survivor: &QuerySourceLog,
    query: &str,
) -> WikiResult<Vec<PathBuf>> {
    let duplicate_path = PathBuf::from(&duplicate.path);
    let survivor_path = PathBuf::from(&survivor.path);
    if duplicate_path == survivor_path {
        return Ok(Vec::new());
    }
    let Some(mut dup_parts) = read_page_parts(&duplicate_path)? else {
        return Ok(Vec::new());
    };
    let Some(mut survivor_parts) = read_page_parts(&survivor_path)? else {
        return Ok(Vec::new());
    };

    let survivor_paragraphs: HashSet<String> =
        split_paragraphs(&survivor_parts.body).into_iter().collect();
    let new_paragraphs = split_paragraphs(&dup_parts.body)
        .into_iter()
        .filter(|p| !survivor_paragraphs.contains(p))
        .collect::<Vec<_>>();
    if !new_paragraphs.is_empty() {
        survivor_parts.body = format!(
            "{}\n\n<!-- dream auto-merged from [[{}]] for query `{}` -->\n\n{}",
            survivor_parts.body.trim_end(),
            duplicate.id,
            query,
            new_paragraphs.join("\n\n")
        );
    }
    merge_yaml_list_field(
        &mut survivor_parts.frontmatter,
        &dup_parts.frontmatter,
        "aliases",
        24,
    );
    merge_yaml_list_field(
        &mut survivor_parts.frontmatter,
        &dup_parts.frontmatter,
        "keywords",
        32,
    );
    merge_yaml_list_field(
        &mut survivor_parts.frontmatter,
        &dup_parts.frontmatter,
        "questions",
        24,
    );
    add_yaml_list_values(
        &mut survivor_parts.frontmatter,
        "facts",
        &[format!(
            "Dream merged duplicate page [[{}]] into [[{}]] for query '{}'",
            duplicate.id, survivor.id, query
        )],
        24,
    );
    set_yaml_scalar(
        &mut survivor_parts.frontmatter,
        "dream_last_merged",
        &chrono::Utc::now().to_rfc3339(),
    );

    set_yaml_scalar(&mut dup_parts.frontmatter, "status", "redirect");
    set_yaml_scalar(&mut dup_parts.frontmatter, "redirect", &survivor.id);
    set_yaml_scalar(
        &mut dup_parts.frontmatter,
        "dream_merged_date",
        &chrono::Utc::now().to_rfc3339(),
    );
    if !dup_parts.body.contains("## Dream Redirect") {
        dup_parts.body = format!(
            "# Redirect to [[{}]]\n\n## Dream Redirect\n\nThis page was marked as a duplicate during dream Purify and now redirects to [[{}]].\n\n{}",
            survivor.id,
            survivor.id,
            dup_parts.body.trim_start()
        );
    }

    write_page_parts(&survivor_path, &survivor_parts)?;
    write_page_parts(&duplicate_path, &dup_parts)?;
    update_edges_redirect(&duplicate.id, &survivor.id)?;
    Ok(vec![survivor_path, duplicate_path])
}

fn enrich_page_from_queries(
    source: &QuerySourceLog,
    query_count: usize,
    density: usize,
    queries: &[String],
) -> WikiResult<Option<PathBuf>> {
    let path = PathBuf::from(&source.path);
    if !page_path_is_editable(&source.path) {
        return Ok(None);
    }
    let Some(mut parts) = read_page_parts(&path)? else {
        return Ok(None);
    };
    set_yaml_scalar(&mut parts.frontmatter, "dream_enrich", "true");
    set_yaml_scalar(
        &mut parts.frontmatter,
        "dream_enrich_date",
        &chrono::Utc::now().to_rfc3339(),
    );
    let dream_query_count =
        yaml_scalar_usize(&parts.frontmatter, "dream_query_count") + query_count;
    set_yaml_scalar(
        &mut parts.frontmatter,
        "dream_query_count",
        &dream_query_count.to_string(),
    );
    let terms = queries
        .iter()
        .flat_map(|q| split_query_terms(q))
        .collect::<Vec<_>>();
    add_yaml_list_values(&mut parts.frontmatter, "keywords", &terms, 32);
    add_yaml_list_values(&mut parts.frontmatter, "questions", queries, 24);
    add_yaml_list_values(
        &mut parts.frontmatter,
        "facts",
        &[format!(
            "Dream selected this low-density page for enrichment after {query_count} query hits in {AUDIT_WINDOW_DAYS} days"
        )],
        24,
    );

    let section = format!(
        "## Dream Maintenance\n\n- Observed query intents: {}\n- Query hits in the last {} days: {}\n- Density before enrichment: {} non-whitespace chars\n- Next action: expand this page with source-backed details or create linked draft research notes.\n",
        queries.join(", "),
        AUDIT_WINDOW_DAYS,
        query_count,
        density
    );
    if let Some(start) = parts.body.find("\n## Dream Maintenance\n") {
        parts.body.truncate(start);
        parts.body = format!("{}\n\n{}", parts.body.trim_end(), section);
    } else if parts.body.trim().is_empty() {
        parts.body = section;
    } else {
        parts.body = format!("{}\n\n{}", parts.body.trim_end(), section);
    }
    write_page_parts(&path, &parts)?;
    Ok(Some(path))
}

fn split_paragraphs(body: &str) -> Vec<String> {
    body.split("\n\n")
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty())
        .collect()
}

fn merge_yaml_list_field(
    target: &mut serde_yaml::Mapping,
    source: &serde_yaml::Mapping,
    key: &str,
    max_len: usize,
) {
    let values = yaml_list_strings(source.get(serde_yaml::Value::String(key.to_string())));
    add_yaml_list_values(target, key, &values, max_len);
}

fn update_edges_redirect(from_id: &str, to_id: &str) -> WikiResult<()> {
    let edges_file = get_wiki_dir().join("graph").join("edges.json");
    if !edges_file.exists() {
        return Ok(());
    }
    let text = fs::read_to_string(&edges_file)?;
    let mut value: serde_json::Value =
        serde_json::from_str(&text).unwrap_or(serde_json::Value::Null);
    let edges = if let Some(edges) = value.get_mut("edges").and_then(|v| v.as_array_mut()) {
        edges
    } else if let Some(edges) = value.as_array_mut() {
        edges
    } else {
        return Ok(());
    };
    let mut changed = false;
    for edge in edges {
        if edge.get("source").and_then(|v| v.as_str()) == Some(from_id) {
            edge["source"] = serde_json::Value::String(to_id.to_string());
            changed = true;
        }
        if edge.get("target").and_then(|v| v.as_str()) == Some(from_id) {
            edge["target"] = serde_json::Value::String(to_id.to_string());
            changed = true;
        }
    }
    if changed {
        fs::write(edges_file, serde_json::to_string_pretty(&value)?)?;
    }
    Ok(())
}

fn ensure_wiki_git_repo() -> WikiResult<()> {
    let wiki_dir = get_wiki_dir();
    fs::create_dir_all(&wiki_dir)?;
    if wiki_dir.join(".git").is_dir() {
        return Ok(());
    }
    let status = Command::new("git")
        .arg("-C")
        .arg(&wiki_dir)
        .arg("init")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?;
    if !status.success() {
        return Err(WikiError::Internal(
            "failed to initialise .wiki git repo".into(),
        ));
    }
    let gitignore = wiki_dir.join(".gitignore");
    if !gitignore.exists() {
        fs::write(
            gitignore,
            "# Auto-generated by llm-wiki dream\n\
             dream/*.out.log\n\
             dream/*.err.log\n\
             __pycache__/\n\
             *.pyc\n",
        )?;
    }
    run_git(["config", "user.name", "llm-wiki-dream"])?;
    run_git(["config", "user.email", "dream@llm-wiki.local"])?;
    Ok(())
}

fn create_snapshot(label: &str) -> WikiResult<String> {
    ensure_wiki_git_repo()?;
    run_git(["add", "-A"])?;
    let message = format!("dream: {label} [{}]", chrono::Utc::now().to_rfc3339());
    run_git(["commit", "--allow-empty", "-m", &message])?;
    let output = git_output(["rev-parse", "HEAD"])?;
    Ok(output.trim().to_string())
}

fn rollback_to_snapshot(snapshot: &str, label: &str) -> WikiResult<()> {
    run_git(["checkout", snapshot, "--", "."])?;
    run_git(["add", "-A"])?;
    let message = format!(
        "dream: rollback {label} to {} [{}]",
        &snapshot[..snapshot.len().min(12)],
        chrono::Utc::now().to_rfc3339()
    );
    run_git(["commit", "--allow-empty", "-m", &message])?;
    Ok(())
}

fn run_git<const N: usize>(args: [&str; N]) -> WikiResult<()> {
    let status = Command::new("git")
        .arg("-C")
        .arg(get_wiki_dir())
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(WikiError::Internal("git command failed in .wiki".into()))
    }
}

fn git_output<const N: usize>(args: [&str; N]) -> WikiResult<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(get_wiki_dir())
        .args(args)
        .output()?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        Err(WikiError::Internal(
            "git output command failed in .wiki".into(),
        ))
    }
}

fn read_dream_experience() -> String {
    fs::read_to_string(experience_path()).unwrap_or_default()
}

fn write_dream_context(experience: &str) -> WikiResult<()> {
    let path = dream_dir().join(format!("{}-context.md", today()));
    let body = if experience.trim().is_empty() {
        "# Dream Context\n\nNo prior dream experience recorded.\n".to_string()
    } else {
        format!(
            "# Dream Context\n\n## Prior Experience\n\n{}\n",
            experience.trim()
        )
    };
    fs::write(path, body)?;
    Ok(())
}

fn append_experience(
    phase: &str,
    outcome: &str,
    summary: &str,
    report: Option<&QualityReport>,
) -> WikiResult<()> {
    fs::create_dir_all(dream_dir())?;
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(experience_path())?;
    writeln!(
        file,
        "\n## {} — {} / {}\n\n{}",
        chrono::Utc::now().to_rfc3339(),
        phase,
        outcome,
        summary
    )?;
    if let Some(report) = report {
        writeln!(
            file,
            "\n- overall_score: {:+.3}\n- rank_score: {:+.3}\n- density_score: {:+.3}\n- coverage_score: {:+.3}",
            report.overall_score, report.rank_score, report.density_score, report.coverage_score
        )?;
        if report.recommendation == "rollback" {
            writeln!(
                file,
                "\n### Diagnosis\n\nRetrieval quality crossed the rollback threshold. Next dream should prefer smaller changes, avoid redirecting pages that still rank for active queries, and enrich before merging when density is low.\n"
            )?;
        }
    }
    Ok(())
}

fn experience_path() -> PathBuf {
    dream_dir().join("experience.md")
}

fn dedupe_paths(paths: &mut Vec<PathBuf>) {
    let mut seen = HashSet::new();
    paths.retain(|p| seen.insert(canonical_string(p)));
}

fn canonical_string(path: &Path) -> String {
    path.canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .to_string()
}

fn round3(value: f64) -> f64 {
    (value * 1000.0).round() / 1000.0
}

fn optimize_page_for_query(source: &QuerySourceLog, query: &LightQuery) -> WikiResult<usize> {
    let path = PathBuf::from(&source.path);
    let Some(mut parts) = read_page_parts(&path)? else {
        return Ok(0);
    };

    let mut added = 0usize;
    let representative = query
        .examples
        .first()
        .cloned()
        .unwrap_or_else(|| query.normalized.clone());
    added += add_yaml_list_values(
        &mut parts.frontmatter,
        "questions",
        &[representative.clone(), query.normalized.clone()],
        12,
    );

    let terms = query_terms(query);
    added += add_yaml_list_values(&mut parts.frontmatter, "keywords", &terms, 24);

    let aliases = alias_candidates(query, &parts.frontmatter);
    added += add_yaml_list_values(&mut parts.frontmatter, "aliases", &aliases, 16);

    let fact = format!(
        "Dream observed query intent '{}' {} time(s)",
        query.normalized, query.count
    );
    added += add_yaml_list_values(&mut parts.frontmatter, "facts", &[fact], 16);

    set_yaml_scalar(
        &mut parts.frontmatter,
        "dream_last_touched",
        &chrono::Utc::now().to_rfc3339(),
    );
    let query_count = yaml_scalar_usize(&parts.frontmatter, "dream_query_count") + query.count;
    set_yaml_scalar(
        &mut parts.frontmatter,
        "dream_query_count",
        &query_count.to_string(),
    );

    if added > 0 {
        write_page_parts(&path, &parts)?;
    }
    Ok(added)
}

fn should_create_split_concept(
    query: &LightQuery,
    target: &QuerySourceLog,
    candidates: &[QuerySourceLog],
) -> bool {
    if query.count < 2 || candidates.len() < 2 {
        return false;
    }
    let strong_alternatives = candidates
        .iter()
        .filter(|c| c.path != target.path && c.relevance >= target.relevance * 0.75)
        .count();
    strong_alternatives >= 1 && query.normalized.chars().count() >= 4
}

fn create_split_concept(
    query: &LightQuery,
    target: &QuerySourceLog,
    candidates: &[QuerySourceLog],
) -> WikiResult<PathBuf> {
    let page_dir = get_pages_dir().join("concepts");
    fs::create_dir_all(&page_dir)?;
    let page_id = format!("dream-split-{}-{}", today(), stable_slug(&query.normalized));
    let page_path = page_dir.join(format!("{page_id}.md"));
    if page_path.exists() {
        return Ok(page_path);
    }

    let mut body = format!(
        "# {}\n\n## Query Intent\n\nUsers repeatedly asked: `{}`.\n\n## Current Best Target\n\n- [[{}]] `{}`\n\n## Related Candidates\n\n",
        query.normalized, query.normalized, target.id, target.path
    );
    for candidate in candidates.iter().take(5) {
        body.push_str(&format!(
            "- [[{}]] `{}` relevance {:.3}\n",
            candidate.id, candidate.path, candidate.relevance
        ));
    }
    body.push_str("\n## Split Guidance\n\nKeep this page only if the query intent spans several existing concepts. Otherwise merge its aliases/questions back into the best target.\n");

    let output = format!(
        "---\nid: {page_id}\ntype: concept\nname: {}\nconfidence: 0.58\naliases: [{}]\nkeywords: [{}]\nquestions: [{}]\nfacts: [Dream created this page because repeated queries matched multiple existing concepts]\nsource: dream-light-sleep\n---\n\n{}",
        yaml_inline_string(&query.normalized),
        yaml_inline_string(&query.normalized),
        query_terms(query)
            .into_iter()
            .map(|s| yaml_inline_string(&s))
            .collect::<Vec<_>>()
            .join(", "),
        query
            .examples
            .iter()
            .take(3)
            .map(|s| yaml_inline_string(s))
            .collect::<Vec<_>>()
            .join(", "),
        body
    );
    fs::write(&page_path, output)?;
    Ok(page_path)
}

fn read_page_parts(path: &Path) -> WikiResult<Option<PageParts>> {
    let content = fs::read_to_string(path)?;
    if !content.starts_with("---\n") {
        return Ok(None);
    }
    let Some(end) = content[4..].find("\n---") else {
        return Ok(None);
    };
    let fm = content[4..4 + end].trim();
    let body = content[4 + end + 4..].trim_start().to_string();
    let frontmatter = serde_yaml::from_str::<serde_yaml::Mapping>(fm).unwrap_or_default();
    Ok(Some(PageParts { frontmatter, body }))
}

fn write_page_parts(path: &Path, parts: &PageParts) -> WikiResult<()> {
    let fm = serde_yaml::to_string(&parts.frontmatter)?;
    fs::write(path, format!("---\n{}---\n\n{}", fm, parts.body))?;
    Ok(())
}

fn add_yaml_list_values(
    fm: &mut serde_yaml::Mapping,
    key: &str,
    values: &[String],
    max_len: usize,
) -> usize {
    let yaml_key = serde_yaml::Value::String(key.to_string());
    let mut existing = yaml_list_strings(fm.get(&yaml_key));
    let mut seen: HashSet<String> = existing.iter().map(|s| normalize_query(s)).collect();
    let mut added = 0usize;
    for value in values {
        let cleaned = value.trim();
        if cleaned.is_empty() || is_low_value_query(cleaned) {
            continue;
        }
        let normalized = normalize_query(cleaned);
        if normalized.is_empty() || !seen.insert(normalized) {
            continue;
        }
        existing.push(cleaned.to_string());
        added += 1;
        if existing.len() >= max_len {
            break;
        }
    }
    fm.insert(
        yaml_key,
        serde_yaml::Value::Sequence(
            existing
                .into_iter()
                .take(max_len)
                .map(serde_yaml::Value::String)
                .collect(),
        ),
    );
    added
}

fn set_yaml_scalar(fm: &mut serde_yaml::Mapping, key: &str, value: &str) {
    fm.insert(
        serde_yaml::Value::String(key.to_string()),
        serde_yaml::Value::String(value.to_string()),
    );
}

fn yaml_list_strings(value: Option<&serde_yaml::Value>) -> Vec<String> {
    match value {
        Some(serde_yaml::Value::Sequence(items)) => items
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect(),
        Some(serde_yaml::Value::String(s)) => vec![s.clone()],
        _ => Vec::new(),
    }
}

fn yaml_scalar_usize(fm: &serde_yaml::Mapping, key: &str) -> usize {
    fm.get(serde_yaml::Value::String(key.to_string()))
        .and_then(|v| {
            v.as_u64()
                .map(|n| n as usize)
                .or_else(|| v.as_str().and_then(|s| s.parse::<usize>().ok()))
        })
        .unwrap_or(0)
}

fn query_terms(query: &LightQuery) -> Vec<String> {
    let mut terms = Vec::new();
    for text in std::iter::once(&query.normalized).chain(query.examples.iter()) {
        for term in split_query_terms(text) {
            if term.chars().count() >= 2 && !terms.contains(&term) {
                terms.push(term);
            }
        }
    }
    if terms.is_empty() {
        terms.push(query.normalized.clone());
    }
    terms.truncate(8);
    terms
}

fn alias_candidates(query: &LightQuery, fm: &serde_yaml::Mapping) -> Vec<String> {
    let mut aliases = Vec::new();
    let name = fm
        .get(serde_yaml::Value::String("name".into()))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    for term in query_terms(query) {
        if term.chars().count() >= 2
            && term.chars().count() <= 32
            && !term.eq_ignore_ascii_case(name)
            && !aliases.contains(&term)
        {
            aliases.push(term);
        }
    }
    aliases
}

fn split_query_terms(text: &str) -> Vec<String> {
    let mut cleaned = text.to_string();
    for noise in [
        "是什么",
        "怎么",
        "如何",
        "为什么",
        "请问",
        "查询",
        "介绍",
        "what is",
        "how to",
        "why",
        "?",
        "？",
    ] {
        cleaned = cleaned.replace(noise, " ");
    }
    let terms: Vec<String> = cleaned
        .split(|c: char| c.is_whitespace() || c == ',' || c == '，' || c == '。')
        .map(|s| s.trim_matches(|c: char| c.is_ascii_punctuation()).trim())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect();
    if terms.is_empty() && !cleaned.trim().is_empty() {
        vec![cleaned.trim().to_string()]
    } else {
        terms
    }
}

fn source_from_search_result(result: SearchResult) -> QuerySourceLog {
    QuerySourceLog {
        id: result.id,
        name: result.title.unwrap_or_default(),
        path: result.path.to_string_lossy().to_string(),
        page_type: result.entity_type.unwrap_or_else(|| "unknown".into()),
        relevance: result.rrf_score.unwrap_or(result.score),
    }
}

fn page_path_is_editable(path: &str) -> bool {
    let path = Path::new(path);
    if !path.exists() || path.extension().and_then(|e| e.to_str()) != Some("md") {
        return false;
    }
    let Ok(path) = path.canonicalize() else {
        return false;
    };
    let Ok(pages_dir) = get_pages_dir().canonicalize() else {
        return false;
    };
    path.starts_with(pages_dir)
}

fn stable_slug(input: &str) -> String {
    let slug = input
        .to_lowercase()
        .replace([' ', '_'], "-")
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .collect::<String>()
        .trim_matches('-')
        .to_string();
    if !slug.is_empty() {
        return slug;
    }
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    input.hash(&mut h);
    format!("q-{:x}", h.finish())
}

fn yaml_inline_string(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "\"\"".to_string())
}

fn maybe_run_research_hook(task: &serde_json::Value) -> WikiResult<()> {
    let Ok(cmd) = std::env::var("LLM_WIKI_RESEARCH_CMD") else {
        return Ok(());
    };
    if cmd.trim().is_empty() {
        return Ok(());
    }
    let mut child = Command::new("sh")
        .arg("-lc")
        .arg(cmd)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    if let Some(stdin) = child.stdin.as_mut() {
        stdin.write_all(serde_json::to_string(task)?.as_bytes())?;
    }
    let _ = child.wait();
    Ok(())
}

fn terminate_running_worker(dir: &Path) {
    let Ok(status_text) = fs::read_to_string(dir.join("status.json")) else {
        return;
    };
    let Ok(mut status) = serde_json::from_str::<DreamStatus>(&status_text) else {
        return;
    };
    if status.state != "running" || status.pid == std::process::id() || status.pid == 0 {
        return;
    }
    #[cfg(unix)]
    {
        let _ = Command::new("kill")
            .arg("-TERM")
            .arg(status.pid.to_string())
            .status();
    }
    #[cfg(windows)]
    {
        let _ = Command::new("taskkill")
            .arg("/PID")
            .arg(status.pid.to_string())
            .arg("/T")
            .arg("/F")
            .status();
    }
    status.state = "cancelled".to_string();
    status.stage = "cancelled".to_string();
    status.updated_at = chrono::Utc::now().to_rfc3339();
    status.message = "Dream terminated by query or compile".to_string();
    let _ = fs::write(
        dir.join("status.json"),
        serde_json::to_string_pretty(&status).unwrap_or_default(),
    );
}

fn read_today_query_log() -> WikiResult<Vec<QueryLogEntry>> {
    let path = get_wiki_dir()
        .join("audit")
        .join(format!("query-log-{}.jsonl", today()));
    if !path.exists() {
        return Ok(Vec::new());
    }
    let content = fs::read_to_string(path)?;
    let mut entries = Vec::new();
    for line in content.lines().filter(|l| !l.trim().is_empty()) {
        if let Ok(entry) = serde_json::from_str::<QueryLogEntry>(line) {
            entries.push(entry);
        }
    }
    Ok(entries)
}

fn read_recent_query_logs(days: i64) -> WikiResult<Vec<QueryLogEntry>> {
    let mut entries = Vec::new();
    for offset in (0..days).rev() {
        let date = (chrono::Local::now() - chrono::Duration::days(offset))
            .format("%Y%m%d")
            .to_string();
        let path = get_wiki_dir()
            .join("audit")
            .join(format!("query-log-{date}.jsonl"));
        if !path.exists() {
            continue;
        }
        let content = fs::read_to_string(path)?;
        for line in content.lines().filter(|l| !l.trim().is_empty()) {
            if let Ok(entry) = serde_json::from_str::<QueryLogEntry>(line) {
                entries.push(entry);
            }
        }
    }
    Ok(entries)
}

fn recurring_queries(entries: &[QueryLogEntry], limit: usize) -> Vec<(String, usize)> {
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for entry in entries {
        let raw = entry.question.trim();
        let norm = normalize_query(raw);
        if norm.is_empty() || is_low_value_query(&norm) {
            continue;
        }
        *counts.entry(norm).or_insert(0) += 1;
    }
    let mut ranked: Vec<_> = counts
        .into_iter()
        .filter(|(_, count)| *count >= MIN_QUERY_COUNT)
        .collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    ranked.truncate(limit);
    ranked
}

fn page_signature(path: &str) -> Option<String> {
    let content = fs::read_to_string(path).ok()?;
    let body = strip_frontmatter(&content);
    let comparable = body
        .lines()
        .take_while(|line| line.trim() != "## Dream Maintenance")
        .filter(|line| {
            let trimmed = line.trim_start();
            !trimmed.starts_with('#')
                && !trimmed.starts_with("<!-- dream auto-merged")
                && !trimmed.starts_with("This page was marked as a duplicate")
        })
        .collect::<Vec<_>>()
        .join("\n");
    let signature = comparable
        .chars()
        .filter(|c| !c.is_whitespace())
        .take(200)
        .collect::<String>();
    if signature.chars().count() < 80 {
        None
    } else {
        Some(signature)
    }
}

fn normalize_query(query: &str) -> String {
    let mut q = query
        .trim()
        .trim_matches(|c: char| c.is_ascii_punctuation() || c.is_whitespace())
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    for pattern in [
        "我喜欢",
        "我偏好",
        "我的首选是",
        "是我的首选",
        "是首选",
        "首选",
        "i like",
        "my preferred",
        "my favorite is",
        "is my favorite",
    ] {
        q = q.replace(pattern, " ");
    }
    q.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn is_low_value_query(query: &str) -> bool {
    query.chars().count() <= 1
        || LOW_VALUE_QUERIES
            .iter()
            .any(|q| q.eq_ignore_ascii_case(query))
}

fn dedupe_sources(sources: &mut Vec<QuerySourceLog>) {
    let mut seen = HashSet::new();
    sources.retain(|s| seen.insert(s.path.clone()));
}

fn page_density(path: &str) -> usize {
    fs::read_to_string(path)
        .map(|s| {
            strip_frontmatter(&s)
                .chars()
                .filter(|c| !c.is_whitespace())
                .count()
        })
        .unwrap_or(0)
}

fn strip_frontmatter(content: &str) -> String {
    if content.starts_with("---") {
        if let Some(end) = content[4..].find("\n---") {
            return content[4 + end + 4..].to_string();
        }
    }
    content.to_string()
}

fn check_cancelled() -> WikiResult<()> {
    if dream_dir().join("cancel.flag").exists() {
        write_status(
            "cancelled",
            "cancelled",
            "Dream cancelled by query or compile",
        )?;
        return Err(WikiError::Internal("Dream cancelled".into()));
    }
    Ok(())
}

fn pause_for_interrupt_window() -> WikiResult<()> {
    for _ in 0..5 {
        check_cancelled()?;
        std::thread::sleep(Duration::from_millis(100));
    }
    Ok(())
}

fn write_status(state: &str, stage: &str, message: &str) -> WikiResult<()> {
    let dir = dream_dir();
    fs::create_dir_all(&dir)?;
    let now = chrono::Utc::now().to_rfc3339();
    let started_at = fs::read_to_string(dir.join("status.json"))
        .ok()
        .and_then(|s| serde_json::from_str::<DreamStatus>(&s).ok())
        .map(|s| s.started_at)
        .unwrap_or_else(|| now.clone());
    let status = DreamStatus {
        state: state.to_string(),
        stage: stage.to_string(),
        pid: std::process::id(),
        started_at,
        updated_at: now,
        message: message.to_string(),
    };
    fs::write(
        dir.join("status.json"),
        serde_json::to_string_pretty(&status)?,
    )?;
    Ok(())
}

fn dream_dir() -> PathBuf {
    get_wiki_dir().join("dream")
}

fn today() -> String {
    chrono::Local::now().format("%Y%m%d").to_string()
}

#[allow(dead_code)]
fn _is_under(path: &Path, parent: &Path) -> bool {
    path.starts_with(parent)
}
