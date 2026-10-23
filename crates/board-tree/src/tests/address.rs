//! Addressing one directory as a pure decision over generated listings: what
//! a segment names agrees with the code each entry of the directory gets.

use std::collections::BTreeMap;

use super::*;
use crate::address::{self, Named};
use crate::tree::{Kind, RawEntry};

/// What a record beside its directory gives when read to tell whether it
/// shadows the directory: a pointer (valid or not), another record, or a
/// read that fails.
#[derive(Clone, Copy, Debug)]
enum Read {
    Shadows,
    Clear,
    Fails,
}

/// A directory listing whose entries share a few segments, with each
/// entry's kind and what reading it gives.
fn listing() -> impl Strategy<Value = Vec<(RawEntry<()>, Read)>> {
    let name = prop::sample::select(vec!["a.json", "a", "A.json", "a.md", "b.json", "b"]);
    let kind = prop_oneof![
        Just(Kind::File(ObjectId::Sha1([1; 20]))),
        Just(Kind::Directory(())),
        Just(Kind::Symlink(ObjectId::Sha1([2; 20]))),
        Just(Kind::Submodule),
        Just(Kind::Unreadable),
    ];
    let read = prop_oneof![Just(Read::Shadows), Just(Read::Clear), Just(Read::Fails)];
    prop::collection::btree_map(name, (kind, read), 0..6).prop_map(|entries: BTreeMap<_, _>| {
        entries
            .into_iter()
            .map(|(name, (kind, read))| {
                let entry = RawEntry {
                    name: name.as_bytes().to_vec(),
                    kind,
                    stat: None,
                    size: None,
                };
                (entry, read)
            })
            .collect()
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn a_segment_is_shadowed_when_named_exactly_when_its_entries_are_degraded_as_shadowed(
        listing in listing(),
    ) {
        let entries: Vec<RawEntry<()>> = listing.iter().map(|(entry, _)| entry.clone()).collect();
        let shadows = |record: &RawEntry<()>| {
            let (_, read) = listing
                .iter()
                .find(|(entry, _)| entry.name == record.name)
                .expect("a record is one of the listing's entries");
            match read {
                Read::Shadows => Ok(true),
                Read::Clear => Ok(false),
                Read::Fails => Err(()),
            }
        };
        let addressed = address::directory(&LogicalPath::root(), &entries, shadows);
        for (entry, (_, code)) in entries.iter().zip(&addressed) {
            let segment = address::segment(entry).expect("every name is UTF-8");
            let named = address::named(&entries, segment, shadows);
            match named {
                Ok(Named::Shadowed) => prop_assert_eq!(*code, Some(Code::PathShadowed)),
                Err(()) => prop_assert_eq!(*code, Some(Code::RepoUnreadable)),
                Ok(Named::Nothing) => prop_assert!(false, "{segment} names its own entry"),
                Ok(Named::One(_) | Named::RecordWithChildren { .. }) => {
                    prop_assert_ne!(*code, Some(Code::PathShadowed));
                }
            }
        }
    }
}

#[test]
fn a_record_or_symlink_beside_its_directory_names_both_in_either_order_unless_it_shadows() {
    let kinds = [
        Kind::File(ObjectId::Sha1([1; 20])),
        Kind::Symlink(ObjectId::Sha1([2; 20])),
    ];
    for kind in kinds {
        let file = RawEntry {
            name: b"a.json".to_vec(),
            kind,
            stat: None,
            size: None,
        };
        let directory = RawEntry {
            name: b"a".to_vec(),
            kind: Kind::Directory(()),
            stat: None,
            size: None,
        };
        for entries in [
            [file.clone(), directory.clone()],
            [directory.clone(), file.clone()],
        ] {
            let named = address::named(&entries, "a", |_| Ok::<_, ()>(false)).unwrap();
            let Named::RecordWithChildren { record, children } = named else {
                panic!("expected a record with children, got {named:?}");
            };
            assert_eq!(
                (&record.name, &children.name),
                (&file.name, &directory.name)
            );
            assert!(matches!(
                address::named(&entries, "a", |_| Ok::<_, ()>(true)),
                Ok(Named::Shadowed)
            ));
        }
    }
}

#[test]
fn an_entry_is_alike_to_a_name_when_its_segment_is_the_names_or_its_name_is_but_for_case() {
    let entry = |name: &str, kind| RawEntry {
        name: name.as_bytes().to_vec(),
        kind,
        stat: None,
        size: None,
    };
    let file = || Kind::File(ObjectId::Sha1([1; 20]));
    for (name, kind, alike) in [
        ("a", Kind::Directory(()), true),
        ("A", Kind::Directory(()), true),
        ("a.json", file(), true),
        ("A.json", file(), false),
        ("b.json", file(), false),
    ] {
        let found = crate::tree::is_alike(&entry(name, kind), b"a", Some("a"));
        assert_eq!(found, alike, "{name}");
    }
}
