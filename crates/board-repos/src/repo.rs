//! Reading and writing a file of a notes repo at its pin, through the
//! host.

use board_model::{Code, Fields, Location, LogicalPath, Object, PathError, Refusal};
use board_ops::{Host, PluginError, PutError};
use serde_json::{Value, json};

/// A file at the pin: its text, its blob, the record Board reads from it,
/// and its location. The text is none when the file is not UTF-8. All four
/// are none when there is no file.
#[derive(Clone, Debug, Default)]
pub struct File {
    pub text: Option<String>,
    pub blob: Option<String>,
    pub record: Option<Value>,
    pub location: Option<Location>,
}

/// The notes repo at `root`, `/` or a mount's path; `clone` is none for
/// the board's own tree, which is never synced.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Repo {
    pub root: String,
    pub clone: Option<String>,
}

impl Repo {
    /// The logical path of `path`, relative to the repo's root.
    pub fn logical(&self, path: &str) -> String {
        format!("{}/{path}", self.root.trim_end_matches('/'))
    }

    /// The file at `path` in the repo, at the pin. Nothing at the path, or
    /// a directory there, is no file; any other refusal of `board get` is
    /// `<plugin>::board::refused`.
    pub fn read(&self, host: &dyn Host, plugin: &str, path: &str) -> Result<File, PluginError> {
        let refused = |refusal: &Refusal| PluginError::board_refused(plugin, "get", refusal);
        let logical: LogicalPath = (self.logical(path).parse()).map_err(|error: PathError| {
            refused(&Refusal::new(error.code(), &error, &Fields::default()))
        })?;
        let answers =
            (host.get_each(&[logical])).map_err(|error| refused(&error.refusal("get")))?;
        let report = match answers.into_iter().next() {
            Some(Ok(report)) => report,
            Some(Err(error)) if error.code() != Code::PathNotFound => {
                return Err(refused(&error.refusal("get")));
            }
            _ => return Ok(File::default()),
        };
        let Object::Blob(blob) = report.location.object else {
            return Ok(File::default());
        };
        Ok(File {
            text: String::from_utf8(report.bytes).ok(),
            blob: Some(blob.to_string()),
            record: report.record.ok(),
            location: Some(report.location),
        })
    }

    /// Writes `text` at `path` if it still holds `expect`, a blob or none:
    /// the commit that holds it, none when uncommitted.
    pub fn write(
        &self,
        host: &dyn Host,
        path: &str,
        text: &str,
        expect: Option<&str>,
    ) -> Result<Option<String>, PutError> {
        let line = json!({
            "path": self.logical(path),
            "expect": expect.unwrap_or("none"),
            "text": text,
        });
        let report = host.put(&[line])?;
        Ok(report.commit.map(|commit| commit.to_string()))
    }
}

#[cfg(test)]
mod tests;
