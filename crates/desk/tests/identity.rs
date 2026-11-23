//! The installation's id: a version-4 UUID, minted once and kept (ADR-0009, decision 2).

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use desk::identity::{self, DeviceId, IdError};

static NEXT: AtomicU64 = AtomicU64::new(0);

fn scratch() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "desk-identity-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = fs::remove_dir_all(&dir);
    dir
}

#[test]
fn a_minted_id_is_a_version_4_uuid() {
    let id = DeviceId::mint().expect("the OS has a source of chance");
    let text = id.as_str();
    assert_eq!(text.len(), 36, "{text}");
    for (i, byte) in text.bytes().enumerate() {
        if matches!(i, 8 | 13 | 18 | 23) {
            assert_eq!(byte, b'-', "hyphens are at 8, 13, 18, 23 in {text}");
        } else {
            assert!(
                byte.is_ascii_hexdigit(),
                "{text} has a non-hex character at {i}"
            );
        }
    }
    assert_eq!(&text[14..15], "4", "the version nibble says v4: {text}");
    assert!(
        matches!(&text[19..20], "8" | "9" | "a" | "b"),
        "the variant nibble is RFC 4122: {text}"
    );
    DeviceId::parse(text).expect("what we mint, we accept");
}

#[test]
fn two_minted_ids_differ() {
    let a = DeviceId::mint().expect("random");
    let b = DeviceId::mint().expect("random");
    assert_ne!(a, b, "an id two machines can share is not an identity");
}

#[test]
fn the_first_run_mints_and_every_run_after_reads() {
    let dir = scratch();
    let first = DeviceId::load_or_mint(&dir).expect("first run");
    let second = DeviceId::load_or_mint(&dir).expect("second run");
    assert_eq!(first, second, "the id must survive a restart");
    assert!(DeviceId::path(&dir).is_file(), "the id was not written");
}

#[test]
fn rotating_replaces_the_id_on_disk() {
    let dir = scratch();
    let first = DeviceId::load_or_mint(&dir).expect("first run");
    let second = DeviceId::rotate(&dir).expect("rotate");
    assert_ne!(first, second);
    assert_eq!(
        DeviceId::load(&dir).expect("read back"),
        second,
        "a rotated id is the one the next run reads"
    );
}

#[test]
fn a_corrupt_file_is_refused_rather_than_replaced() {
    let dir = scratch();
    fs::create_dir_all(&dir).expect("scratch");
    fs::write(DeviceId::path(&dir), "not a uuid\n").expect("write a damaged file");
    match DeviceId::load_or_mint(&dir) {
        Err(IdError::Shape(_)) => {}
        other => panic!("a damaged identity must be visible, got {other:?}"),
    }
}

#[test]
fn only_a_version_4_uuid_is_accepted() {
    // v1: the version nibble is 1, which is a MAC-and-clock id — a different kind of identifier.
    assert!(DeviceId::parse("d0eaf332-c9c1-1f8f-8ab5-99be8e57012a").is_err());
    // v4 with a variant outside 8..b (this one is reserved for NCS).
    assert!(DeviceId::parse("d0eaf332-c9c1-4f8f-0ab5-99be8e57012a").is_err());
    assert!(DeviceId::parse("d0eaf332c9c11f8f8ab599be8e57012a").is_err());
    assert!(DeviceId::parse("").is_err());
    // Uppercase hex is the same id, not a different one.
    let upper =
        DeviceId::parse("D0EAF332-C9C1-4F8F-8AB5-99BE8E57012A").expect("hex case is not identity");
    assert_eq!(upper.as_str(), "d0eaf332-c9c1-4f8f-8ab5-99be8e57012a");
}

#[cfg(unix)]
#[test]
fn the_id_file_is_mode_0600() {
    use std::os::unix::fs::PermissionsExt;
    let dir = scratch();
    let id = DeviceId::load_or_mint(&dir).expect("first run");
    let path = DeviceId::path(&dir);
    let mode = fs::metadata(&path).expect("metadata").permissions().mode() & 0o777;
    assert_eq!(mode, 0o600, "{} is {mode:o}, not 600", path.display());
    assert_eq!(DeviceId::load(&dir).expect("read back"), id);
}

#[test]
fn the_config_directory_is_the_walls_neighbour() {
    // Both binaries live under `arcons-cc`; the wall keeps `config.toml` there and the desk its id.
    let dir = identity::config_dir();
    if let Some(dir) = dir {
        assert!(
            dir.ends_with("arcons-cc"),
            "the desk writes beside the wall: {}",
            dir.display()
        );
    }
}
