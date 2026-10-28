//! Board's pure types and decisions. Nothing here performs I/O.

mod app;
mod attachment;
mod code;
mod effect;
mod fields;
mod frontmatter;
mod json;
mod location;
mod markdown;
mod path;
mod pointer;
mod record;
mod refusal;
mod stat;
mod ui;

pub use crate::app::{BINARY_NAME, DISPLAY_NAME};
pub use crate::attachment::{ATTACHMENT_TYPE, Attachment, Digest, media_type};
pub use crate::code::Code;
pub use crate::effect::{
    APPROVAL_HEADER, Answer, Authority, Decision, EFFECT_TYPE, EFFECT_TYPES, Effect, Layer,
    PREFERENCE_TYPE, PUT_OP, Pattern, Preference, RM_OP, Request, Rule, SEEN_TYPE, Seen, Tell,
    WRITE_HEADER, decide, effect_record, is_effect_id, is_op, is_write, signature_namespace,
};
pub use crate::fields::{Fields, Found, Known, Lined};
pub use crate::frontmatter::{Feature, FrontmatterError, frontmatter};
pub use crate::json::{Canonical, JsonError, canonical_json, parse_json};
pub use crate::location::{Board, Location, Object, ObjectId, Source};
pub use crate::markdown::{links, reached};
pub use crate::path::{
    LogicalPath, MAX_PATH_BYTES, MAX_SEGMENT_BYTES, MAX_SYMLINKS, Malformation, PathError, escaped,
    is_hidden,
};
pub use crate::pointer::{
    MAX_RECORD_BYTES, Mount, POINTER_TYPE, Pointer, PointerError, pointer, repinned,
};
pub use crate::record::{
    CONFIG_TYPE, Classified, Content, Kind, MANIFEST_TYPE, RESERVED_NAMESPACES, RecordType,
    RecordTypeError, Role, TYPES_DIRECTORY, classify, is_json_file,
};
pub use crate::refusal::{Refusal, RefusalCode};
pub use crate::stat::{Change, Stat};
pub use crate::ui::{
    BINDING_TYPE, Binding, ComponentKind, DERIVED_TYPE, Fill, PatchError, Slot, UiError, VIEW_TYPE,
    View, check as check_ui,
};
