//! Package wiki skill into .tar.gz or .zip archives with secret scanning.

use std::fs;
use std::io::Write;
use std::path::Path;

use crate::error::WikiResult;

const SECRET_PATTERNS: &[(&str, &str)] = &[
    ("(?i)(?:sk|pk|rk)-(?:[a-zA-Z0-9]{20,})", "API key"),
    ("(?:ghp|gho|ghu|ghs|ghr)_[a-zA-Z0-9]{36,}", "GitHub token"),
    ("(?i)api_key[=:]\\s*\\S+", "API key assignment"),
    ("(?i)password[=:]\\s*\\S+", "Password assignment"),
];

pub fn package_tar_gz(source_dir: &Path, output: &Path) -> WikiResult<()> {
    let file = fs::File::create(output)?;
    let gz = flate2::write::GzEncoder::new(file, flate2::Compression::default());
    let mut tar = tar::Builder::new(gz);
    tar.append_dir_all(".", source_dir)?;
    tar.finish()?;
    Ok(())
}

pub fn package_zip(source_dir: &Path, output: &Path) -> WikiResult<()> {
    let file = fs::File::create(output)?;
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default();
    add_dir_to_zip(&mut zip, source_dir, source_dir, options)?;
    zip.finish()
        .map_err(|e| crate::error::WikiError::Internal(format!("zip: {e}")))?;
    Ok(())
}

fn add_dir_to_zip(
    zip: &mut zip::ZipWriter<fs::File>,
    base: &Path,
    dir: &Path,
    options: zip::write::SimpleFileOptions,
) -> WikiResult<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let name = path.strip_prefix(base).unwrap_or(&path);

        if path.is_dir() {
            if path
                .file_name()
                .map_or(false, |n| n == ".git" || n == "target")
            {
                continue;
            }
            zip.add_directory(name.to_string_lossy(), options)
                .map_err(|e| crate::error::WikiError::Internal(format!("zip dir: {e}")))?;
            add_dir_to_zip(zip, base, &path, options)?;
        } else {
            zip.start_file(name.to_string_lossy(), options)
                .map_err(|e| crate::error::WikiError::Internal(format!("zip: {e}")))?;
            zip.write_all(&fs::read(&path)?)
                .map_err(|e| crate::error::WikiError::Internal(format!("zip write: {e}")))?;
        }
    }
    Ok(())
}

pub fn scan_secrets(dir: &Path) -> Vec<(String, String, String)> {
    let mut findings = Vec::new();
    let walker = walkdir::WalkDir::new(dir).into_iter().filter_entry(|e| {
        let n = e.file_name().to_string_lossy();
        !n.starts_with('.') && n != "target" && n != "node_modules"
    });
    for entry in walker.filter_map(|e| e.ok()) {
        if !entry.file_type().is_file() {
            continue;
        }
        if let Ok(content) = fs::read_to_string(entry.path()) {
            for (pattern, label) in SECRET_PATTERNS {
                if let Ok(re) = regex::Regex::new(pattern) {
                    if re.is_match(&content) {
                        findings.push((
                            entry.path().to_string_lossy().to_string(),
                            label.to_string(),
                            "Potential secret detected".to_string(),
                        ));
                    }
                }
            }
        }
    }
    findings
}
