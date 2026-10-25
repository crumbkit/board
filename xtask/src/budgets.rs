//! The word budgets: what an agent loads at session start, the documentation
//! map, the documentation itself, the plan, and the plan's milestone leaves.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::config::{ContextBudget, DirectoryBudget, FileBudget};
use crate::files::{self, FilesError, Root};
use crate::report::{Check, CheckKind, Limit, Part};
use crate::words;

/// How many imports deep Claude Code follows `@path`.
const MAX_IMPORT_HOPS: usize = 5;

/// The entry files and every file they import, each counted once.
pub(crate) fn context(root: &Root, budget: &ContextBudget) -> Result<Check, FilesError> {
    let mut pending = VecDeque::new();
    for entry in &budget.entry {
        pending.push_back((files::canonical(&root.path().join(entry))?, 0));
    }
    let mut seen = BTreeSet::new();
    let mut parts = Vec::new();
    while let Some((path, hops)) = pending.pop_front() {
        if !seen.insert(path.clone()) {
            continue;
        }
        let text = files::read(&path)?;
        parts.push(Part::new(root.display(&path), words::count(&text)));
        if hops < MAX_IMPORT_HOPS {
            for import in words::imports(&text) {
                if let Some(target) = root.import(&path, &import) {
                    pending.push_back((target, hops + 1));
                }
            }
        }
    }
    Ok(Check::total(
        CheckKind::Context,
        Limit::AtMost(budget.max_words),
        parts,
    ))
}

/// One file's words, reported as `kind`.
pub(crate) fn file(root: &Root, kind: CheckKind, budget: &FileBudget) -> Result<Check, FilesError> {
    let words = words::count(&root.read(&budget.file)?);
    Ok(Check::total(
        kind,
        Limit::AtMost(budget.max_words),
        vec![Part::new(budget.file.clone(), words)],
    ))
}

/// One check for each directory under the docs directory, itself included.
/// Each counts only the `.md` files directly inside it, so a subdirectory's
/// words never count toward its parent.
pub(crate) fn docs(root: &Root, budget: &DirectoryBudget) -> Result<Vec<Check>, FilesError> {
    let mut checks = Vec::new();
    for node in root.markdown_nodes(&budget.directory)? {
        let mut parts = Vec::new();
        for path in &node.files {
            parts.push(Part::new(
                root.display(path),
                words::count(&files::read(path)?),
            ));
        }
        checks.push(
            Check::total(CheckKind::Docs, Limit::AtMost(budget.max_words), parts)
                .in_node(root.display(&node.directory)),
        );
    }
    Ok(checks)
}

/// One check for each milestone leaf directly in the budget's directory, and
/// a last check that `index` links every leaf once, links no missing leaf,
/// and that the directory holds no subdirectory.
pub(crate) fn plan_leaves(
    root: &Root,
    index: &str,
    budget: &DirectoryBudget,
) -> Result<Vec<Check>, FilesError> {
    let mut checks = Vec::new();
    let mut leaves = BTreeSet::new();
    for path in root.files_with_extension(&budget.directory, "md")? {
        let leaf = root.display(&path);
        let words = words::count(&files::read(&path)?);
        checks.push(
            Check::total(
                CheckKind::PlanLeaf,
                Limit::AtMost(budget.max_words),
                vec![Part::new(leaf.clone(), words)],
            )
            .in_node(leaf.clone()),
        );
        leaves.insert(leaf);
    }
    let prefix = format!("{}/", budget.directory);
    let mut links: BTreeMap<String, usize> = BTreeMap::new();
    for target in words::link_targets(&root.read(index)?) {
        if target.starts_with(&prefix) {
            *links.entry(target).or_default() += 1;
        }
    }
    let mut problems = Vec::new();
    for leaf in leaves.iter().filter(|leaf| !links.contains_key(*leaf)) {
        problems.push(format!("{leaf} is not linked from {index}"));
    }
    for (link, times) in &links {
        if !leaves.contains(link) {
            problems.push(format!("{index} links {link}, which does not exist"));
        } else if *times > 1 {
            problems.push(format!("{index} links {link} {times} times"));
        }
    }
    if root.path().join(&budget.directory).is_dir() {
        for node in root.markdown_nodes(&budget.directory)?.iter().skip(1) {
            problems.push(format!(
                "{} is a subdirectory; the plan holds one flat file per milestone",
                root.display(&node.directory)
            ));
        }
    }
    let parts = problems
        .into_iter()
        .map(|problem| Part::new(problem, 1))
        .collect();
    checks.push(Check::total(CheckKind::PlanLinks, Limit::Exactly(0), parts));
    Ok(checks)
}

#[cfg(test)]
mod tests;
