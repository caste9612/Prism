//! History of the disk health (F17-AC2): readings stored and read back newest first, old ones
//! removed, and the comparison that raises the notification (F17-AC3).
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use common::TestCatalog;
use prism_core::health::{HealthReading, HealthStatus, worsening};
use prism_core::store;

fn reading(status: HealthStatus, reallocated: u64) -> HealthReading {
    HealthReading {
        status: Some(status),
        temperature_c: Some(38),
        power_on_hours: Some(20_000),
        reallocated: Some(reallocated),
        pending: Some(0),
        reasons: if reallocated > 0 {
            vec!["reallocated"]
        } else {
            Vec::new()
        },
        ..HealthReading::default()
    }
}

#[test]
fn f17_history_newest_first_and_pruned() {
    let cat = TestCatalog::new();
    let key = "WDC WD40EFRX|WD-123";
    for (at, r) in [
        (1_000, reading(HealthStatus::Good, 0)),
        (2_000, reading(HealthStatus::Warning, 8)),
        (3_000, reading(HealthStatus::Warning, 12)),
    ] {
        cat.store
            .write(move |w| store::add_health(w, key, at, &r))
            .unwrap();
    }
    // Another disk does not mix in; a reading without a status is not stored.
    cat.store
        .write(|w| {
            store::add_health(w, "other|1", 1_500, &reading(HealthStatus::Good, 0))?;
            store::add_health(w, key, 4_000, &HealthReading::default())
        })
        .unwrap();
    let history = cat
        .store
        .read(|c| store::health_history(c, key, 10))
        .unwrap();
    assert_eq!(
        history.iter().map(|h| h.at).collect::<Vec<_>>(),
        [3_000, 2_000, 1_000]
    );
    assert_eq!(history[0].reallocated, Some(12));
    assert_eq!(history[0].reasons, ["reallocated"]);
    assert_eq!(history[2].status, HealthStatus::Good);
    // F17-AC3: from one stored reading to the next.
    assert_eq!(
        worsening(&history[2].reading(), &history[1].reading()),
        Some("status")
    );
    assert_eq!(
        worsening(&history[1].reading(), &history[0].reading()),
        Some("reallocated")
    );
    assert_eq!(
        worsening(&history[0].reading(), &history[0].reading()),
        None
    );

    let removed = cat.store.write(|w| store::prune_health(w, 2_500)).unwrap();
    assert_eq!(removed, 3, "two of this disk and the other one");
    assert_eq!(
        cat.store
            .read(|c| store::health_history(c, key, 10))
            .unwrap()
            .len(),
        1
    );
}
