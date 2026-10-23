//! The thread fold and its validation, one rule per test (14 §1–§4).

use scatter_threads::{
    AnyPages, Category, Check, ImportedFrom, Invalid, Limits, Minted, Pages, Range, Record,
    Statuses, SubjectError, Target, Thread, ThreadOp, WIKITEXT, Writer, mint_title,
    normalize_subject,
};
use serde_json::json;

fn rec(revid: u64, op: ThreadOp) -> Record {
    Record {
        revid,
        actor: format!("local:{revid}"),
        time: i64::try_from(revid).unwrap() * 60,
        has_text: op.has_text(),
        op,
        minted: Minted::default(),
    }
}

fn editor() -> Check<'static> {
    Check {
        writer: Writer::Editor,
        limits: Limits::default(),
        ..Check::replay()
    }
}

fn import() -> Check<'static> {
    Check {
        writer: Writer::Import,
        ..editor()
    }
}

fn create(name: &str) -> ThreadOp {
    ThreadOp::Create {
        name: name.into(),
        target: Target::Page(42),
        also: Vec::new(),
        status: None,
        pinned: false,
        media_type: None,
        imported_from: None,
    }
}

fn post(reply: Option<u64>) -> ThreadOp {
    ThreadOp::Post {
        in_reply_to: reply,
        status: None,
        media_type: None,
        heading: None,
        collapse: None,
        uncollapse: None,
        imported_from: None,
    }
}

fn with(op: ThreadOp, f: impl FnOnce(&mut ThreadOp)) -> ThreadOp {
    let mut op = op;
    f(&mut op);
    op
}

fn status(s: &str) -> impl FnOnce(&mut ThreadOp) {
    let s = s.to_string();
    move |op| {
        if let ThreadOp::Post { status, .. } | ThreadOp::Create { status, .. } = op {
            *status = Some(s);
        }
    }
}

fn collapse(first: u64, last: u64) -> impl FnOnce(&mut ThreadOp) {
    move |op| {
        if let ThreadOp::Post { collapse, .. } = op {
            *collapse = Some(Range { first, last });
        }
    }
}

/// A thread of `n` posts after the opening one (revids 1..=n+1), each replying to the one
/// before.
fn thread(n: u64) -> Thread {
    let mut t = Thread::create(&rec(1, create("Why is P31 wrong")), &editor()).unwrap();
    for k in 2..=n + 1 {
        t.apply(&rec(k, post(Some(k - 1))), &editor()).unwrap();
    }
    t
}

#[test]
fn content_parts_use_activity_streams_names() {
    let op = with(post(Some(7)), status("resolved"));
    assert_eq!(
        op.to_json(),
        json!({"op": "post", "inReplyTo": 7, "status": "resolved"})
    );
    assert_eq!(
        post(None).to_json(),
        json!({"op": "post", "inReplyTo": null})
    );
    let c = create("X");
    assert_eq!(
        c.to_json(),
        json!({"op": "create", "name": "X", "target": {"kind": "page", "id": 42}})
    );
    let m = ThreadOp::Move {
        target: Target::Actor("local:5".into()),
        keep: false,
    };
    assert_eq!(
        m.to_json(),
        json!({"op": "move", "target": {"kind": "actor", "id": "local:5"}})
    );
    for op in [op, c, m, with(post(None), collapse(3, 4))] {
        assert_eq!(ThreadOp::from_json(op.to_json()).unwrap(), op);
    }
    assert!(ThreadOp::from_json(json!({"op": "post", "imported_from": {"bogus": 1}})).is_err());
}

#[test]
fn the_default_statuses_include_closed() {
    let s = Statuses::embedded();
    assert_eq!(s.get("closed").unwrap().category, Category::Closed);
    assert_eq!(s.get("open").unwrap().category, Category::Open);
    assert!(s.iter().map(|s| s.order).is_sorted());
    assert!(
        Statuses::from_toml("version = 1\n").is_err(),
        "open is required"
    );
}

#[test]
fn subjects_are_normalized_and_titles_suffixed() {
    assert_eq!(
        normalize_subject("  why_is  P31 wrong ").unwrap(),
        "Why is P31 wrong"
    );
    assert_eq!(normalize_subject("   "), Err(SubjectError::Empty));
    assert_eq!(
        normalize_subject("a [b]"),
        Err(SubjectError::Forbidden('['))
    );
    assert_eq!(
        normalize_subject(&"x".repeat(250)),
        Err(SubjectError::TooLong)
    );
    let taken = ["2026-09-27/Merge", "2026-09-27/Merge (2)"];
    assert_eq!(
        mint_title("2026-09-27", "Merge", |t| taken.contains(&t)),
        "2026-09-27/Merge (3)"
    );
    assert_eq!(
        mint_title("2026-09-28", "Merge", |_| false),
        "2026-09-28/Merge"
    );
}

#[test]
fn posts_form_a_tree_and_the_latest_status_wins() {
    let mut t = thread(2);
    t.apply(&rec(4, with(post(Some(1)), status("resolved"))), &editor())
        .unwrap();
    assert_eq!(t.posts().len(), 4);
    assert_eq!(t.post(4).unwrap().parent, Some(1));
    assert_eq!(t.status(), "resolved");
    assert_eq!(t.category(Statuses::embedded()), Category::Closed);
    t.apply(&rec(5, with(post(None), status("open"))), &editor())
        .unwrap();
    assert_eq!(t.category(Statuses::embedded()), Category::Open);
    assert_eq!(
        t.apply(&rec(6, post(Some(99))), &editor()),
        Err(Invalid::NotAPost(99))
    );
    assert_eq!(
        t.apply(&rec(6, with(post(None), status("nonsense"))), &editor()),
        Err(Invalid::Status("nonsense".into()))
    );
}

#[test]
fn edits_replace_a_posts_text_in_its_own_media_type() {
    let mut t = thread(1);
    let edit = |object, media_type: Option<&str>| ThreadOp::Edit {
        object,
        media_type: media_type.map(str::to_string),
    };
    t.apply(&rec(3, edit(2, None)), &editor()).unwrap();
    assert_eq!(t.post(2).unwrap().text, 3);
    assert_eq!(t.post(2).unwrap().edits, [3]);
    assert_eq!(
        t.apply(&rec(4, edit(2, Some(WIKITEXT))), &editor()),
        Err(Invalid::MediaType(WIKITEXT.into()))
    );
    assert_eq!(
        t.apply(&rec(4, edit(3, None)), &editor()),
        Err(Invalid::NotAPost(3)),
        "an edit is not a post"
    );
    let mut no_text = rec(4, edit(2, None));
    no_text.has_text = false;
    assert_eq!(t.apply(&no_text, &editor()), Err(Invalid::Text));
}

#[test]
fn only_imports_write_headings_wikitext_and_imported_from() {
    let mut t = thread(0);
    let heading = with(post(None), |op| {
        if let ThreadOp::Post { heading, .. } = op {
            *heading = Some(3);
        }
    });
    let wikitext = with(post(None), |op| {
        if let ThreadOp::Post { media_type, .. } = op {
            *media_type = Some(WIKITEXT.into());
        }
    });
    let imported = with(post(None), |op| {
        if let ThreadOp::Post { imported_from, .. } = op {
            *imported_from = Some(ImportedFrom {
                signed_as: Some("Harej".into()),
                ..ImportedFrom::default()
            });
        }
    });
    for (op, what) in [
        (&heading, "heading"),
        (&wikitext, "mediaType: text/x-wiki"),
        (&imported, "imported_from"),
    ] {
        assert_eq!(
            t.validate(&rec(2, op.clone()), &editor()),
            Err(Invalid::ImportOnly(what))
        );
        t.validate(&rec(2, op.clone()), &import()).unwrap();
    }
    t.apply(&rec(2, heading), &import()).unwrap();
    assert_eq!(t.post(2).unwrap().heading, Some(3));
}

#[test]
fn a_collapse_hides_a_range_until_uncollapsed() {
    let mut t = thread(4); // posts 1..=5
    t.apply(&rec(6, with(post(None), collapse(3, 4))), &editor())
        .unwrap();
    assert!(t.is_collapsed(3) && t.is_collapsed(4));
    assert!(!t.is_collapsed(2) && !t.is_collapsed(5) && !t.is_collapsed(6));
    // Replies to collapsed posts still work.
    t.apply(&rec(7, post(Some(4))), &editor()).unwrap();
    let uncollapse = with(post(None), |op| {
        if let ThreadOp::Post { uncollapse, .. } = op {
            *uncollapse = Some(6);
        }
    });
    t.apply(&rec(8, uncollapse.clone()), &editor()).unwrap();
    assert!(!t.is_collapsed(3));
    assert_eq!(
        t.apply(&rec(9, uncollapse), &editor()),
        Err(Invalid::NotCollapsing(6))
    );
}

#[test]
fn collapses_nest_but_do_not_cross() {
    let mut t = thread(5); // posts 1..=6
    t.apply(&rec(7, with(post(None), collapse(2, 5))), &editor())
        .unwrap();
    t.apply(&rec(8, with(post(None), collapse(3, 4))), &editor())
        .unwrap();
    assert_eq!(
        t.validate(&rec(9, with(post(None), collapse(4, 6))), &editor()),
        Err(Invalid::CollapseCrosses(7))
    );
    assert_eq!(
        t.validate(&rec(9, with(post(None), collapse(5, 3))), &editor()),
        Err(Invalid::CollapseOrder)
    );
    t.validate(&rec(9, with(post(None), collapse(1, 7))), &editor())
        .unwrap();
}

#[test]
fn a_collapsing_post_is_not_a_reply_except_to_an_imported_heading() {
    let mut t = thread(1);
    assert_eq!(
        t.validate(&rec(3, with(post(Some(1)), collapse(2, 2))), &editor()),
        Err(Invalid::CollapseReply)
    );
    let heading = with(post(None), |op| {
        if let ThreadOp::Post { heading, .. } = op {
            *heading = Some(3);
        }
    });
    t.apply(&rec(3, heading), &import()).unwrap();
    t.apply(&rec(4, post(Some(3))), &import()).unwrap();
    t.apply(&rec(5, with(post(Some(3)), collapse(4, 4))), &import())
        .unwrap();
    assert!(t.is_collapsed(4));
}

#[test]
fn a_failed_record_leaves_the_thread_unchanged() {
    let mut t = thread(2);
    let before = t.clone();
    let bad = with(post(None), |op| {
        if let ThreadOp::Post {
            collapse,
            uncollapse,
            ..
        } = op
        {
            *collapse = Some(Range { first: 2, last: 3 });
            *uncollapse = Some(2);
        }
    });
    assert_eq!(
        t.apply(&rec(4, bad), &editor()),
        Err(Invalid::CollapseAndUncollapse)
    );
    assert_eq!(t, before);
    assert_eq!(
        t.apply(&rec(4, create("Again")), &editor()),
        Err(Invalid::Create)
    );
}

/// Pages for the attachment tests: 99 is missing, 50 is a board that homes nothing, and 60
/// already has three pinned threads.
struct TestPages;

impl Pages for TestPages {
    fn exists(&self, t: &Target) -> bool {
        t != &Target::Page(99)
    }
    fn takes_homes(&self, t: &Target) -> bool {
        t != &Target::Page(50)
    }
    fn pinned_on(&self, t: &Target) -> usize {
        if t == &Target::Page(60) { 3 } else { 0 }
    }
}

fn pages() -> Check<'static> {
    Check {
        pages: &TestPages,
        ..editor()
    }
}

#[test]
fn listings_attach_and_detach() {
    let mut t = thread(0);
    let attach = |id| ThreadOp::Attach {
        target: Target::Page(id),
    };
    t.apply(&rec(2, attach(50)), &pages()).unwrap();
    assert_eq!(
        t.validate(&rec(3, attach(50)), &pages()),
        Err(Invalid::Attached(Target::Page(50)))
    );
    assert_eq!(
        t.validate(&rec(3, attach(42)), &pages()),
        Err(Invalid::Attached(Target::Page(42))),
        "the home is never a listing"
    );
    assert_eq!(
        t.validate(&rec(3, attach(99)), &pages()),
        Err(Invalid::NoTarget(Target::Page(99)))
    );
    assert_eq!(
        t.validate(
            &rec(
                3,
                ThreadOp::Attach {
                    target: Target::Actor("local:1".into())
                }
            ),
            &pages()
        ),
        Err(Invalid::ActorListing)
    );
    for id in 51..54 {
        t.apply(&rec(id, attach(id)), &pages()).unwrap();
    }
    assert_eq!(
        t.validate(&rec(60, attach(54)), &pages()),
        Err(Invalid::TooManyAttachments)
    );
    let detach = |id| ThreadOp::Detach {
        target: Target::Page(id),
    };
    t.apply(&rec(61, detach(51)), &pages()).unwrap();
    assert_eq!(
        t.validate(&rec(62, detach(42)), &pages()),
        Err(Invalid::NotAListing(Target::Page(42)))
    );
    assert_eq!(t.attachments().count(), 4);
}

#[test]
fn moves_change_the_home_and_may_keep_the_old_one() {
    let mut t = thread(0);
    let mv = |target, keep| ThreadOp::Move { target, keep };
    assert_eq!(
        t.validate(&rec(2, mv(Target::Page(50), false)), &pages()),
        Err(Invalid::NoHomes(Target::Page(50)))
    );
    assert_eq!(
        t.validate(&rec(2, mv(Target::Page(42), false)), &pages()),
        Err(Invalid::SameHome)
    );
    t.apply(
        &rec(
            2,
            ThreadOp::Pin {
                target: Target::Page(42),
            },
        ),
        &pages(),
    )
    .unwrap();
    t.apply(&rec(3, mv(Target::Page(43), true)), &pages())
        .unwrap();
    assert_eq!(t.home.target, Target::Page(43));
    assert_eq!(t.listings[0].target, Target::Page(42));
    assert!(t.listings[0].pinned.is_some(), "keep keeps the pin");
    // Moving to a listing promotes it.
    t.apply(&rec(4, mv(Target::Page(42), false)), &pages())
        .unwrap();
    assert_eq!(t.home.target, Target::Page(42));
    assert!(t.listings.is_empty());
    assert!(t.home.pinned.is_some());
    // A user talk page can be a home but not kept as a listing.
    t.apply(
        &rec(5, mv(Target::Actor("local:9".into()), false)),
        &pages(),
    )
    .unwrap();
    assert_eq!(
        t.validate(&rec(6, mv(Target::Page(42), true)), &pages()),
        Err(Invalid::ActorListing)
    );
}

#[test]
fn pins_are_per_attachment_and_limited() {
    let mut t = thread(0);
    let pin = |id| ThreadOp::Pin {
        target: Target::Page(id),
    };
    let unpin = |id| ThreadOp::Unpin {
        target: Target::Page(id),
    };
    assert_eq!(
        t.validate(&rec(2, pin(43)), &pages()),
        Err(Invalid::NotAttached(Target::Page(43)))
    );
    t.apply(&rec(2, pin(42)), &pages()).unwrap();
    assert_eq!(t.home.pinned, Some(120));
    assert_eq!(
        t.validate(&rec(3, pin(42)), &pages()),
        Err(Invalid::Pinned(Target::Page(42)))
    );
    t.apply(
        &rec(
            3,
            ThreadOp::Attach {
                target: Target::Page(60),
            },
        ),
        &pages(),
    )
    .unwrap();
    assert_eq!(
        t.validate(&rec(4, pin(60)), &pages()),
        Err(Invalid::TooManyPinned(Target::Page(60)))
    );
    t.apply(&rec(4, unpin(42)), &pages()).unwrap();
    assert_eq!(
        t.validate(&rec(5, unpin(42)), &pages()),
        Err(Invalid::NotPinned(Target::Page(42)))
    );
}

#[test]
fn an_imported_thread_arrives_closed_pinned_and_listed() {
    let op = ThreadOp::Create {
        name: "Talk page header".into(),
        target: Target::Page(42),
        also: vec![Target::Page(7)],
        status: Some("archived".into()),
        pinned: true,
        media_type: Some(WIKITEXT.into()),
        imported_from: Some(ImportedFrom {
            repository: Some("enwiki".into()),
            page: Some("Talk:Gdańsk".into()),
            revision: Some(1_089_047_270),
            ..ImportedFrom::default()
        }),
    };
    let mut r = rec(1, op);
    r.minted = Minted {
        title: Some("2004-05-01/Talk page header".into()),
        talk: Some(1000),
        also: vec![1001],
    };
    assert_eq!(
        Thread::create(&r, &editor()),
        Err(Invalid::ImportOnly("mediaType: text/x-wiki"))
    );
    let t = Thread::create(&r, &import()).unwrap();
    assert_eq!(t.status(), "archived");
    assert_eq!(t.home.talk, Some(1000));
    assert_eq!(t.home.pinned, Some(60));
    assert_eq!(t.listings[0].talk, Some(1001));
    assert_eq!(t.title.as_deref(), Some("2004-05-01/Talk page header"));
    assert!(t.posts()[0].imported);
}

#[test]
fn replay_folds_the_log() {
    let records = [
        rec(1, create("Merge")),
        rec(2, post(Some(1))),
        rec(
            3,
            ThreadOp::Rename {
                name: "merge proposal".into(),
            },
        ),
        rec(4, with(post(Some(2)), status("closed"))),
    ];
    let t = Thread::replay(&records).unwrap();
    assert_eq!(t.subject, "Merge proposal");
    assert_eq!(t.status(), "closed");
    assert_eq!(t.last, 4);
    assert!(Thread::replay(&records[1..]).is_err());
    let _ = AnyPages;
}
