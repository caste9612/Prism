//! Reset of the catalog (F21-AC4): every volume and its data go, settings and the actions log
//! stay, the files on the disks are untouched.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use common::{Sandbox, TestCatalog};
use prism_core::cancel::CancelToken;
use prism_core::hashing::{HashProgress, hash_volume};
use prism_core::store::{self, ActionRecord, StoreEvent};

#[test]
fn f21_ac4_reset_removes_volumes_and_their_data_only() {
    let a = Sandbox::new();
    a.file(r"Foto\mare.jpg", 70_000, 10);
    a.file("nota.txt", 10, 20);
    let b = Sandbox::new();
    b.file("copia.txt", 10, 20);
    let cat = TestCatalog::new();
    let va = cat.add_volume(&a.root().to_string_lossy());
    let vb = cat.add_volume(&b.root().to_string_lossy());
    cat.index(va, a.root());
    cat.index(vb, b.root());
    hash_volume(
        &cat.store,
        va,
        a.root(),
        &CancelToken::new(),
        &|| false,
        &HashProgress::default(),
    )
    .unwrap();
    let photo = cat
        .store
        .read(|c| store::resolve_relative_path(c, va, r"Foto\mare.jpg"))
        .unwrap()
        .unwrap()
        .id;
    cat.store
        .write(move |w| {
            store::set_setting(w, "app.settings", "{\"language\":\"it\"}")?;
            store::log_actions(
                w,
                &[ActionRecord {
                    id: 0,
                    at: 1,
                    action: "recycle".into(),
                    volume: Some(va),
                    path: "X".into(),
                    size: 1,
                    outcome: "done".into(),
                    detail: None,
                }],
            )
        })
        .unwrap();
    let events = cat.store.subscribe();

    let removed = cat.store.write(store::reset_catalog).unwrap();
    assert_eq!(removed, 2);
    assert!(cat.store.read(store::volumes).unwrap().is_empty());
    assert!(
        !cat.store
            .read(|c| store::volume_has_entries(c, va))
            .unwrap()
    );
    assert!(
        cat.store
            .read(|c| store::entry(c, photo))
            .unwrap()
            .is_none()
    );
    assert!(
        cat.store
            .read(|c| store::valid_hashes(c, photo))
            .unwrap()
            .is_none()
    );
    assert!(
        cat.store
            .read(|c| store::duplicate_files(c, 1))
            .unwrap()
            .is_empty()
    );
    // What is not the catalog stays.
    assert_eq!(
        cat.store
            .read(|c| store::setting(c, "app.settings"))
            .unwrap()
            .as_deref(),
        Some("{\"language\":\"it\"}")
    );
    assert_eq!(
        cat.store.read(|c| store::actions_log(c, 10)).unwrap().len(),
        1
    );
    // The search indexes learn about it like for a removed volume.
    let gone: Vec<StoreEvent> = events.try_iter().collect();
    assert!(gone.contains(&StoreEvent::VolumeRemoved { volume: va }));
    assert!(gone.contains(&StoreEvent::VolumeRemoved { volume: vb }));
    // And the disks are untouched.
    assert!(a.path(r"Foto\mare.jpg").is_file() && b.path("copia.txt").is_file());
    cat.assert_consistent();
}
