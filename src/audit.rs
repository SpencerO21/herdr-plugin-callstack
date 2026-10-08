use crate::{
    host,
    model::{Flow, Frame, Record},
    store::{hash, read_limited},
};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs::File,
    path::{Path, PathBuf},
    sync::Arc,
};

const SOURCE_LIMIT: u64 = 4_000_000;

pub fn frames(items: &[Frame]) -> Vec<&Frame> {
    fn walk<'a>(items: &'a [Frame], out: &mut Vec<&'a Frame>) {
        for f in items {
            out.push(f);
            walk(&f.calls, out);
        }
    }
    let mut out = vec![];
    walk(items, &mut out);
    out
}

fn content(root: &Path, path: &str) -> Option<String> {
    let source = host::source_location(root, path).ok()?;
    String::from_utf8(read_limited(File::open(source.file).ok()?, SOURCE_LIMIT).ok()?).ok()
}

pub fn publish_checks(
    flow: &Flow,
    root: Option<&Path>,
    others: &[Arc<Record>],
) -> (Option<BTreeMap<String, Option<String>>>, Vec<String>) {
    let mut warnings = vec![];
    let mut files = BTreeMap::new();
    let all = frames(&flow.frames);
    for f in &all {
        if let Some(loc) = &f.loc {
            match host::location_parts(loc) {
                Ok((path, _)) => {
                    files
                        .entry(path.to_owned())
                        .or_insert_with(|| root.and_then(|r| content(r, path)));
                }
                Err(e) => warnings.push(format!("{}: invalid source location: {e}", f.function)),
            }
        }
    }
    if root.is_none() && !files.is_empty() {
        warnings
            .push("No project folder. Source checks and change tracking are unavailable.".into());
    }
    if root.is_some() {
        for (path, value) in &files {
            if value.is_none() {
                warnings.push(format!("Cannot read {path}. It may be missing, outside the project, non-text, or over 4 MB."));
            }
        }
        for f in &all {
            if let Some((loc, (path, line))) = f
                .loc
                .as_ref()
                .and_then(|loc| host::location_parts(loc).ok().map(|p| (loc, p)))
                && let Some(Some(text)) = files.get(path)
            {
                if line as usize > text.split('\n').count() {
                    warnings.push(format!("{loc}: line is past the end of the file."));
                }
                let name = f
                    .function
                    .rsplit(['.', '#', ':'])
                    .next()
                    .unwrap_or(&f.function);
                if identifier(name) && !text.contains(name) {
                    warnings.push(format!("{}: function name not found in {path}. This is a text check, not proof of a call.", f.function));
                }
            }
        }
    }
    let changed = all.iter().any(|f| f.marker() != '=');
    if flow.status() == "current" && changed {
        warnings.push("Current flow has change labels. Remove them or use proposed status.".into());
    }
    if flow.status() == "proposed" && !changed {
        warnings.push("Proposed flow has no added, modified, or removed calls.".into());
    }
    let edges = all
        .iter()
        .flat_map(|f| {
            [
                f.input.as_deref().unwrap_or(""),
                f.output.as_deref().unwrap_or(""),
            ]
        })
        .collect::<Vec<_>>()
        .join(" ");
    for name in flow.types.keys() {
        if !edges.contains(name) {
            warnings.push(format!("Type {name} is not used by any input or output."));
        }
    }
    fn siblings(items: &[Frame], warnings: &mut Vec<String>) {
        for (i, f) in items.iter().enumerate() {
            if f.concurrent == Some(true)
                && (i == 0 || items[i - 1].concurrent != Some(true))
                && items
                    .get(i + 1)
                    .is_none_or(|next| next.concurrent != Some(true))
            {
                warnings.push(format!(
                    "{}: parallel call has no adjacent parallel sibling.",
                    f.function
                ));
            }
            siblings(&f.calls, warnings);
        }
    }
    siblings(&flow.frames, &mut warnings);
    let mut names: HashMap<String, Vec<(&str, &str)>> = HashMap::new();
    for record in others.iter().filter(|r| r.flow.name != flow.name) {
        for other in frames(&record.flow.frames) {
            names
                .entry(normalize(&other.function))
                .or_default()
                .push((&other.function, &record.flow.name));
        }
    }
    for f in &all {
        if let Some(matches) = names.get(&normalize(&f.function)) {
            for (other, flow_name) in matches {
                if f.function != *other {
                    warnings.push(format!(
                        "{} nearly matches {} in flow {}. Check the name.",
                        f.function, other, flow_name
                    ));
                }
            }
        }
    }
    warnings.sort();
    warnings.dedup();
    if warnings.len() > 200 {
        warnings.truncate(200);
        warnings.push("More warnings were omitted. Fix these warnings and republish.".into());
    }
    let hashes = root.map(|_| {
        files
            .into_iter()
            .map(|(p, c)| (p, c.map(|c| hash(&c))))
            .collect()
    });
    (hashes, warnings)
}
fn normalize(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}
fn identifier(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '$')
}

#[derive(Default)]
pub struct SourceCache {
    entries: HashMap<(PathBuf, String), (Option<crate::store::Stamp>, Option<String>)>,
    pub reads: usize,
}
impl SourceCache {
    pub fn refresh(&mut self, records: &[Arc<Record>]) -> Vec<Arc<Record>> {
        let mut used = HashSet::new();
        let mut results = vec![];
        for record in records {
            let mut drifted = vec![];
            if let (Some(root), Some(hashes)) = (&record.project_root, &record.source_hashes) {
                for (path, baseline) in hashes {
                    let key = (PathBuf::from(root), path.clone());
                    used.insert(key.clone());
                    let source = host::source_location(Path::new(root), path).ok();
                    let stamp = source
                        .as_ref()
                        .and_then(|s| crate::store::Stamp::read(&s.file).ok());
                    if self.entries.get(&key).is_none_or(|(old, _)| old != &stamp) {
                        let value = content(Path::new(root), path).map(|c| hash(&c));
                        self.reads += 1;
                        self.entries.insert(key.clone(), (stamp, value));
                    }
                    if &self.entries[&key].1 != baseline {
                        drifted.push(path.clone());
                    }
                }
            }
            if drifted == record.drifted_paths {
                results.push(record.clone());
            } else {
                let mut next = record.as_ref().clone();
                next.drifted_paths = drifted;
                results.push(Arc::new(next));
            }
        }
        self.entries.retain(|key, _| used.contains(key));
        results
    }
}

pub fn watch_directories(records: &[Arc<Record>]) -> HashSet<PathBuf> {
    let mut dirs = HashSet::new();
    for record in records {
        if let (Some(root), Some(hashes)) = (&record.project_root, &record.source_hashes) {
            for path in hashes.keys() {
                if let Ok(source) = host::source_path(Path::new(root), path) {
                    let mut parent = source.file.parent();
                    while let Some(dir) = parent {
                        if dir.is_dir() {
                            dirs.insert(dir.to_path_buf());
                            break;
                        }
                        parent = dir.parent();
                    }
                }
            }
        }
    }
    dirs
}
