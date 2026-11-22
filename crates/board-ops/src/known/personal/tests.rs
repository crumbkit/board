//! Naming the personal board in the known boards file, as `board init
//! --personal` does (docs/architecture/paths/personal.md).

use std::io::{self, ErrorKind};
use std::path::{Path, PathBuf};

use board_scenario::{Item, Scenario};
use serde_json::{Value, json};

use super::*;

/// The board's root and an XDG config directory, which holds the known
/// boards file once a test writes it.
struct Place {
    xdg: Scenario,
    _board: Scenario,
    file: PathBuf,
    root: PathBuf,
}

/// The known boards file, from the XDG config directory.
const FILE: &str = "board/config.json";

fn place() -> Place {
    let (xdg, board) = (Scenario::new(), Scenario::new());
    let file = xdg.workdir().canonicalize().unwrap().join(FILE);
    let root = board.workdir().canonicalize().unwrap();
    Place {
        xdg,
        _board: board,
        file,
        root,
    }
}

/// The root as a path relative to the known boards file's directory: both
/// temporary directories share a parent.
fn relative(place: &Place) -> PathBuf {
    Path::new("../..").join(place.root.file_name().unwrap())
}

/// What naming `place`'s root does to a file holding `text`.
fn named(place: &Place, text: &Value) -> Result<Naming, Box<Refusal>> {
    let bytes = serde_json::to_vec(text).unwrap();
    Naming::plan(&place.file, Ok(bytes), &place.root)
}

/// The JSON the naming would write.
fn written(naming: &Naming) -> Value {
    serde_json::from_slice(naming.rewritten().unwrap()).unwrap()
}

#[test]
fn a_missing_or_empty_file_becomes_one_naming_the_root_in_boards_and_personal() {
    let place = place();
    let root = place.root.display().to_string();
    let missing = Err(io::Error::from(ErrorKind::NotFound));
    for read in [missing, Ok(Vec::new()), Ok(b"  \n".to_vec())] {
        let naming = Naming::plan(&place.file, read, &place.root).unwrap();
        assert_eq!(
            written(&naming),
            json!({"boards": [root], "personal": root})
        );
        assert_eq!(naming.file(), place.file);
    }
}

#[test]
fn the_rewrite_keeps_every_other_key_and_board_and_appends_the_root() {
    let place = place();
    let root = place.root.display().to_string();
    let known = json!({"agent": ["claude", "-p"], "day_start": "07:00", "boards": ["/elsewhere"]});
    let naming = named(&place, &known).unwrap();
    let expected = json!({
        "boards": ["/elsewhere", root],
        "personal": root,
        "day_start": "07:00",
        "agent": ["claude", "-p"],
    });
    assert_eq!(written(&naming), expected);
    let text = std::str::from_utf8(naming.rewritten().unwrap()).unwrap();
    let keys: Vec<usize> = ["\"boards\"", "\"personal\"", "\"day_start\"", "\"agent\""]
        .iter()
        .map(|key| text.find(key).unwrap())
        .collect();
    assert!(keys.is_sorted(), "{text}");
    assert!(text.starts_with("{\n  \""), "pretty JSON: {text}");
    assert!(text.ends_with("}\n"), "a trailing newline: {text}");
}

#[test]
fn a_root_boards_lists_by_a_relative_path_is_not_listed_twice() {
    let place = place();
    let relative = relative(&place);
    let naming = named(&place, &json!({"boards": [relative]})).unwrap();
    let root = place.root.display().to_string();
    assert_eq!(
        written(&naming),
        json!({"boards": [relative], "personal": root})
    );
}

#[test]
fn a_file_naming_this_board_already_is_left_unchanged() {
    let place = place();
    let root = place.root.display().to_string();
    let known = json!({"boards": [root], "personal": root, "day_start": "07:00"});
    let naming = named(&place, &known).unwrap();
    assert_eq!(naming.rewritten(), None);
}

#[test]
fn a_file_naming_another_personal_board_is_refused_and_its_twin_naming_this_one_is_admitted() {
    let place = place();
    let other = place.root.join("other");
    let refusal = named(&place, &json!({"personal": other})).unwrap_err();
    assert_eq!(refusal.code.as_str(), "board::config::personal_elsewhere");
    let message = format!(
        "`{}` names {} as the personal board, not {}",
        place.file.display(),
        other.display(),
        place.root.display()
    );
    assert_eq!(refusal.message, message);
    let hint = refusal.hint.unwrap();
    assert!(hint.contains("edit `personal`"), "{hint}");
    assert!(hint.contains(&place.file.display().to_string()), "{hint}");
    // The twin names this board, by a relative path.
    let relative = relative(&place);
    let naming = named(&place, &json!({"personal": relative})).unwrap();
    let root = place.root.display().to_string();
    assert_eq!(
        written(&naming),
        json!({"boards": [root], "personal": relative})
    );
}

#[test]
fn a_file_that_is_not_a_list_of_known_boards_is_refused_as_invalid() {
    let place = place();
    for text in ["{", r#"{"personal": 1}"#, r#"{"other": true}"#] {
        let read = Ok(text.as_bytes().to_vec());
        let refusal = Naming::plan(&place.file, read, &place.root).unwrap_err();
        assert_eq!(refusal.code.as_str(), "board::config::invalid", "{text}");
    }
}

#[test]
fn writing_makes_the_directory_and_replaces_the_file_whole_past_a_stray_temporary_file() {
    let place = place();
    let missing = Err(io::Error::from(ErrorKind::NotFound));
    let naming = Naming::plan(&place.file, missing, &place.root).unwrap();
    assert!(naming.write().unwrap());
    let bytes = std::fs::read(&place.file).unwrap();
    assert_eq!(bytes, naming.rewritten().unwrap());
    let directory = place.file.parent().unwrap();
    let temporary = "board/.config.json.board-tmp";
    place.xdg.write(temporary, Item::File("half"));
    let again = Naming::plan(&place.file, Ok(bytes.clone()), &place.root).unwrap();
    assert!(
        !again.write().unwrap(),
        "a file naming the board is not written"
    );
    let other = json!({"boards": ["/elsewhere"]});
    place.xdg.write(FILE, Item::File(&other.to_string()));
    let read = std::fs::read(&place.file);
    let naming = Naming::plan(&place.file, read, &place.root).unwrap();
    assert!(naming.write().unwrap());
    let names: Vec<_> = std::fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(names, ["config.json"]);
}

#[test]
fn a_root_json_cannot_hold_is_refused_and_writes_nothing_and_a_utf8_root_is_admitted() {
    use std::os::unix::ffi::OsStrExt as _;
    let place = place();
    let missing = || Err(io::Error::from(ErrorKind::NotFound));
    let bytes = place.root.as_os_str().as_bytes().to_vec();
    let unprintable = PathBuf::from(std::ffi::OsStr::from_bytes(&[&bytes[..], b"\xff"].concat()));
    let refusal = Naming::plan(&place.file, missing(), &unprintable).unwrap_err();
    assert_eq!(refusal.code.as_str(), "board::config::invalid");
    assert!(refusal.message.contains("UTF-8"), "{}", refusal.message);
    assert!(!place.file.exists());
    let naming = Naming::plan(&place.file, missing(), &place.root).unwrap();
    assert!(naming.rewritten().is_some());
}

#[test]
fn writing_through_a_symlinked_file_rewrites_its_target_and_keeps_the_link() {
    let place = place();
    place
        .xdg
        .write("dotfiles/config.json", Item::File("{}"))
        .write(FILE, Item::Symlink("../dotfiles/config.json"));
    let read = std::fs::read(&place.file);
    let naming = Naming::plan(&place.file, read, &place.root).unwrap();
    assert!(naming.write().unwrap());
    let link = std::fs::symlink_metadata(&place.file).unwrap();
    assert!(link.is_symlink(), "the link survives");
    let target = place.xdg.workdir().join("dotfiles/config.json");
    assert_eq!(std::fs::read(target).unwrap(), naming.rewritten().unwrap());
    // Its twin, a regular file, is replaced in place.
    let plain = self::place();
    let read = Err(io::Error::from(ErrorKind::NotFound));
    let naming = Naming::plan(&plain.file, read, &plain.root).unwrap();
    assert!(naming.write().unwrap());
    assert!(std::fs::symlink_metadata(&plain.file).unwrap().is_file());
}
