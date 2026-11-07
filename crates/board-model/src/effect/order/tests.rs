#![expect(
    clippy::disallowed_methods,
    reason = "the owner's tests build the order statement to check its layout"
)]
use super::*;
use crate::effect::{APPROVAL_HEADER, WRITE_HEADER, signature_namespace};

fn order(pinned: Pinned, context_text: &str) -> Order {
    Order {
        order: "Do not push to main.".to_owned(),
        context_path: "/orders/main.context.md".to_owned(),
        expires: Expiry {
            year: 2026,
            month: 3,
            day: 7,
        },
        pinned,
        context_text: context_text.to_owned(),
    }
}

#[test]
fn an_order_statement_labels_the_order_its_expiry_whether_it_is_pinned_and_its_context() {
    assert_eq!(
        order(Pinned::Yes, "Asked in review.\n").statement(),
        format!(
            "{ORDER_TYPE}\nexpires: 2026-03-07\npinned: yes\n\nOrder (binding):\nDo not push to main.\n\n\
             Context (non-binding, from /orders/main.context.md):\nAsked in review.\n"
        )
    );
}

#[test]
fn an_order_that_does_not_say_it_is_pinned_is_signed_as_not_pinned() {
    let statement = order(Pinned::default(), "").statement();
    assert!(statement.contains("\npinned: no\n"), "{statement}");
    assert_eq!(Pinned::from(false), Pinned::No);
    assert_eq!(Pinned::from(true), Pinned::Yes);
}

#[test]
fn an_order_statement_ends_with_the_context_files_last_byte() {
    for text in ["no newline", "one newline\n", "two\n\n", ""] {
        let statement = order(Pinned::No, text).statement();
        assert!(statement.ends_with(&format!(":\n{text}")), "{statement:?}");
    }
}

#[test]
fn an_expiry_is_written_as_iso_8601_with_a_four_digit_year() {
    let expiry = |year, month, day| Expiry { year, month, day }.to_string();
    assert_eq!(expiry(2026, 12, 31), "2026-12-31");
    assert_eq!(expiry(1, 1, 1), "0001-01-01");
}

#[test]
fn only_a_statement_whose_whole_first_line_is_the_order_type_is_signed_as_an_order() {
    let statement = order(Pinned::No, "").statement();
    assert_eq!(signature_namespace(&statement), ORDER_TYPE);
    let approval = signature_namespace(&format!("{ORDER_TYPE}x\nexpires: 2026-03-07\n"));
    assert_eq!(approval, APPROVAL_HEADER);
    assert_eq!(signature_namespace(ORDER_TYPE), APPROVAL_HEADER);
    assert_ne!(ORDER_TYPE, WRITE_HEADER);
}
