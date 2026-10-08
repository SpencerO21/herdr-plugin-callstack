use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Scope {
    pub namespace: String,
    pub workspace: String,
    pub session: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Flow {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    pub frames: Vec<Frame>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub types: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Frame {
    #[serde(rename = "fn")]
    pub function: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loc: Option<String>,
    #[serde(default, rename = "in", skip_serializing_if = "Option::is_none")]
    pub input: Option<String>,
    #[serde(default, rename = "out", skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cond: Option<String>,
    #[serde(default, rename = "loop", skip_serializing_if = "Option::is_none")]
    pub loop_context: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub change: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub concurrent: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub module: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub calls: Vec<Frame>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Record {
    pub flow: Flow,
    pub scope: Scope,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_root: Option<String>,
    pub updated_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_hashes: Option<BTreeMap<String, Option<String>>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub drifted_paths: Vec<String>,
}

fn text(value: &str, field: &str, max: usize, required: bool) -> Result<()> {
    // Match JavaScript's UTF-16 length limit for existing documents.
    ensure!(
        value.encode_utf16().count() <= max && (!required || !value.trim().is_empty()),
        "{field} must contain {}–{max} characters.",
        if required { 1 } else { 0 }
    );
    Ok(())
}

impl Flow {
    pub fn validate(&self) -> Result<()> {
        text(&self.name, "name", 120, true)?;
        if let Some(s) = &self.description {
            text(s, "description", 1000, false)?;
        }
        ensure!(
            matches!(self.status.as_deref(), None | Some("current" | "proposed")),
            "Invalid flow status."
        );
        ensure!(!self.frames.is_empty(), "A flow needs at least one frame.");
        fn frames(items: &[Frame], depth: usize, count: &mut usize) -> Result<()> {
            ensure!(
                depth <= 20 && items.len() <= 50,
                "Too many call levels or calls in a group."
            );
            for f in items {
                *count += 1;
                ensure!(*count <= 5000, "The flow exceeds 5000 calls.");
                text(&f.function, "fn", 200, true)?;
                for (key, val, max) in [
                    ("loc", &f.loc, 300),
                    ("in", &f.input, 300),
                    ("out", &f.output, 300),
                    ("cond", &f.cond, 300),
                    ("loop", &f.loop_context, 300),
                    ("module", &f.module, 60),
                    ("note", &f.note, 500),
                    ("detail", &f.detail, 2000),
                ] {
                    if let Some(v) = val {
                        text(v, key, max, key == "module")?;
                    }
                }
                ensure!(
                    matches!(
                        f.change.as_deref(),
                        None | Some("same" | "added" | "modified" | "removed")
                    ),
                    "Invalid change label."
                );
                if !f.calls.is_empty() {
                    frames(&f.calls, depth + 1, count)?;
                }
            }
            Ok(())
        }
        frames(&self.frames, 0, &mut 0)?;
        for (name, definition) in &self.types {
            text(name, "type name", 200, true)?;
            text(definition, "type definition", 2000, true)?;
        }
        Ok(())
    }

    pub fn status(&self) -> &str {
        self.status.as_deref().unwrap_or("current")
    }
}

impl Frame {
    pub fn marker(&self) -> char {
        match self.change.as_deref() {
            Some("added") => '+',
            Some("modified") => '~',
            Some("removed") => '-',
            _ => '=',
        }
    }
    pub fn details(&self) -> Vec<String> {
        [
            ("loc", &self.loc),
            ("in", &self.input),
            ("out", &self.output),
            ("cond", &self.cond),
            ("loop", &self.loop_context),
            ("module", &self.module),
            ("note", &self.note),
            ("detail", &self.detail),
        ]
        .into_iter()
        .filter_map(|(k, v)| v.as_ref().map(|v| format!("{k}: {v}")))
        .collect()
    }
}

pub fn clean(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
}

pub fn plain(flow: &Flow) -> String {
    fn walk(frames: &[Frame], depth: usize, out: &mut Vec<String>) {
        for f in frames {
            out.push(format!(
                "{}{} {}{}",
                "  ".repeat(depth),
                f.marker(),
                f.function,
                if f.concurrent == Some(true) {
                    " [parallel]"
                } else {
                    ""
                }
            ));
            out.extend(
                f.details()
                    .into_iter()
                    .map(|s| format!("{}  {s}", "  ".repeat(depth))),
            );
            walk(&f.calls, depth + 1, out);
        }
    }
    let mut out = vec![format!("{} [{}]", flow.name, flow.status())];
    if let Some(d) = &flow.description {
        out.push(d.clone());
    }
    walk(&flow.frames, 0, &mut out);
    out.extend(flow.types.iter().map(|(k, v)| format!("type {k}: {v}")));
    out.iter().map(|s| clean(s)).collect::<Vec<_>>().join("\n")
}
