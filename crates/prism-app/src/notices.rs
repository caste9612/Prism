//! System notifications about new disks while the window is hidden (F02-AC1, REVIEW L8).
//!
//! The texts come from the interface in the user's language (`NativeTexts`, like the tray
//! labels). Disks found before the interface has sent them (at startup) wait a little; past
//! [`TEXTS_GRACE`] the English defaults are used. Several disks found together give a single
//! notification.

use std::time::{Duration, Instant};

use prism_core::model::{VolumeInfo, VolumeKind};

use crate::dto::NativeTexts;

/// How long new-disk notices wait for the interface texts.
pub const TEXTS_GRACE: Duration = Duration::from_secs(20);

impl Default for NativeTexts {
    fn default() -> Self {
        Self {
            open: "Open Prism".into(),
            search: "Search".into(),
            quit: "Quit".into(),
            new_disk_title: "New disk: {name}".into(),
            new_disks_title: "{count} new disks found".into(),
            new_disk_body: "Open Prism to index or ignore it.".into(),
            pause_hashing: "Pause content analysis".into(),
            resume_hashing: "Resume content analysis".into(),
            jobs_idle: "No jobs in progress".into(),
            jobs_running: "Jobs in progress: {count}".into(),
            health_title: "{name}: the disk health is getting worse".into(),
            health_body: "Open Prism to see its details and copy your files elsewhere.".into(),
        }
    }
}

/// A notification ready to show.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notice {
    pub title: String,
    pub body: String,
}

/// New disks waiting to be announced.
#[derive(Default)]
pub struct Notices {
    texts: Option<NativeTexts>,
    pending: Vec<String>,
    since: Option<Instant>,
}

impl Notices {
    pub fn set_texts(&mut self, texts: NativeTexts) {
        self.texts = Some(texts);
    }

    /// The notification of a disk whose health gets worse (F17-AC3).
    pub fn health_notice(&self, name: &str) -> Notice {
        let defaults = NativeTexts::default();
        let texts = self
            .texts
            .as_ref()
            .filter(|t| !t.health_title.is_empty())
            .unwrap_or(&defaults);
        Notice {
            title: texts.health_title.replace("{name}", name),
            body: texts.health_body.clone(),
        }
    }

    /// A new disk was found.
    pub fn push(&mut self, name: String, now: Instant) {
        self.since.get_or_insert(now);
        self.pending.push(name);
    }

    /// Forgets the pending disks (the window is visible: its panel shows them).
    pub fn clear(&mut self) {
        self.pending.clear();
        self.since = None;
    }

    /// The notification to show now, if any; the pending disks are then forgotten.
    pub fn take_due(&mut self, now: Instant) -> Option<Notice> {
        let since = self.since?;
        if self.pending.is_empty() {
            self.since = None;
            return None;
        }
        let defaults;
        let texts = match &self.texts {
            Some(t) => t,
            None if now.duration_since(since) >= TEXTS_GRACE => {
                defaults = NativeTexts::default();
                &defaults
            }
            None => return None,
        };
        let notice = match self.pending.as_slice() {
            [one] => Notice {
                title: texts.new_disk_title.replace("{name}", one),
                body: texts.new_disk_body.clone(),
            },
            many => Notice {
                title: texts
                    .new_disks_title
                    .replace("{count}", &many.len().to_string()),
                body: many.join(", "),
            },
        };
        self.clear();
        Some(notice)
    }
}

/// How a volume is named in a notification: its label and letter ("KINGSTON (E:)"), or its
/// letter or share alone.
pub fn notice_name(info: &VolumeInfo) -> String {
    let mount = info
        .last_mount
        .as_deref()
        .map(|m| m.trim_end_matches('\\').to_owned());
    let label = info.os_label.clone().filter(|l| !l.trim().is_empty());
    match (label, mount) {
        (Some(l), Some(m)) => format!("{l} ({m})"),
        (Some(l), None) => l,
        (None, Some(m)) => m,
        (None, None) if info.kind == VolumeKind::Network => {
            info.unc_path.clone().unwrap_or_default()
        }
        (None, None) => "?".into(),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    // F17-AC3: the notification of a disk getting worse, in the language of the interface.
    #[test]
    fn health_notice_texts() {
        let mut n = Notices::default();
        assert!(
            n.health_notice("Samsung SSD")
                .title
                .starts_with("Samsung SSD: the disk health")
        );
        n.set_texts(italian());
        assert_eq!(
            n.health_notice("Samsung SSD").title,
            "Samsung SSD: la salute del disco peggiora"
        );
    }

    fn italian() -> NativeTexts {
        NativeTexts {
            open: "Apri Prism".into(),
            search: "Cerca".into(),
            quit: "Esci".into(),
            new_disk_title: "Nuovo disco: {name}".into(),
            new_disks_title: "{count} nuovi dischi trovati".into(),
            new_disk_body: "Apri Prism per indicizzarlo.".into(),
            pause_hashing: "Sospendi l'analisi del contenuto".into(),
            resume_hashing: "Riprendi l'analisi del contenuto".into(),
            jobs_idle: "Nessuna attività in corso".into(),
            jobs_running: "Attività in corso: {count}".into(),
            health_title: "{name}: la salute del disco peggiora".into(),
            health_body: "Apri Prism per i dettagli e copia i tuoi file altrove.".into(),
        }
    }

    #[test]
    fn one_disk_one_notice_in_the_interface_language() {
        let now = Instant::now();
        let mut n = Notices::default();
        n.set_texts(italian());
        assert_eq!(n.take_due(now), None);
        n.push("KINGSTON (E:)".into(), now);
        assert_eq!(
            n.take_due(now),
            Some(Notice {
                title: "Nuovo disco: KINGSTON (E:)".into(),
                body: "Apri Prism per indicizzarlo.".into()
            })
        );
        assert_eq!(n.take_due(now), None, "announced once");
    }

    #[test]
    fn several_disks_are_grouped() {
        let now = Instant::now();
        let mut n = Notices::default();
        n.set_texts(italian());
        n.push("C:".into(), now);
        n.push("Data (D:)".into(), now);
        let notice = n.take_due(now).unwrap();
        assert_eq!(notice.title, "2 nuovi dischi trovati");
        assert_eq!(notice.body, "C:, Data (D:)");
    }

    #[test]
    fn waits_for_the_interface_texts_then_uses_english() {
        let start = Instant::now();
        let mut n = Notices::default();
        n.push("KINGSTON (E:)".into(), start);
        assert_eq!(n.take_due(start + Duration::from_secs(1)), None);
        let late = n.take_due(start + TEXTS_GRACE).unwrap();
        assert_eq!(late.title, "New disk: KINGSTON (E:)");
    }

    #[test]
    fn visible_window_clears_the_pending_disks() {
        let now = Instant::now();
        let mut n = Notices::default();
        n.set_texts(italian());
        n.push("KINGSTON (E:)".into(), now);
        n.clear();
        assert_eq!(n.take_due(now), None);
    }

    #[test]
    fn names_in_notifications() {
        let mut info = VolumeInfo {
            kind: VolumeKind::Removable,
            fs_type: None,
            fs_serial: None,
            volume_guid: None,
            unc_path: None,
            os_label: Some("KINGSTON".into()),
            capacity: None,
            free_space: None,
            bus: None,
            last_mount: Some(r"E:\".into()),
        };
        assert_eq!(notice_name(&info), "KINGSTON (E:)");
        info.os_label = Some(String::new());
        assert_eq!(notice_name(&info), "E:");
        info.last_mount = None;
        info.kind = VolumeKind::Network;
        info.unc_path = Some(r"\\nas\foto".into());
        assert_eq!(notice_name(&info), r"\\nas\foto");
    }
}
