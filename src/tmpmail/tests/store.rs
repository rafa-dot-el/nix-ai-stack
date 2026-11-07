use std::os::unix::fs::PermissionsExt;

use tmpmail::identities::{load, parse, save, Identities, Identity};

fn id(email: &str) -> Identity {
    Identity { email: email.into(), sid_token: format!("sid-{email}"), created_at: 1_700_000_000 }
}

#[test]
fn resolution() {
    let mut ids = Identities::default();
    assert!(ids.resolve(None).unwrap_err().contains("new-identity"));
    ids.upsert(id("a@b.c"));
    assert_eq!(ids.resolve(None).unwrap().email, "a@b.c", "the only one");
    ids.upsert(id("c@d.e"));
    assert!(ids.resolve(None).unwrap_err().contains("--email"), "two: ambiguous");
    assert_eq!(ids.resolve(Some("c@d.e")).unwrap().email, "c@d.e");
    assert!(ids.resolve(Some("x@y.z")).unwrap_err().contains("x@y.z"));
}

#[test]
fn upsert_replaces_and_remove_reports() {
    let mut ids = Identities::default();
    ids.upsert(id("a@b.c"));
    ids.upsert(Identity { sid_token: "new".into(), ..id("a@b.c") });
    assert_eq!((ids.identities.len(), ids.identities[0].sid_token.as_str()), (1, "new"));
    assert!(ids.remove("a@b.c"));
    assert!(!ids.remove("a@b.c"));
}

#[test]
fn parsing() {
    assert_eq!(parse("", "f").unwrap(), Identities::default());
    assert_eq!(parse("  \n", "f").unwrap(), Identities::default());
    let y = "identities:\n  - { email: a@b.c, sid_token: s, created_at: 1 }\n";
    assert_eq!(parse(y, "f").unwrap().identities[0].sid_token, "s");
    assert!(parse("identities: 3\n", "the-file").unwrap_err().contains("the-file"));
}

#[test]
fn the_file_is_private_atomic_and_round_trips() {
    let dir = std::env::temp_dir().join(format!("tmpmail-store-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let path = dir.join("rafa/tmpmail/identities.yaml");
    assert_eq!(load(&path).unwrap(), Identities::default(), "missing means none");
    let mut ids = Identities::default();
    ids.upsert(id("a@b.c"));
    save(&path, &ids).unwrap();
    assert_eq!(load(&path).unwrap(), ids);
    // bug 6cfdf1fe: session tokens were written with the umask's mode (0644)
    assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
    assert_eq!(std::fs::metadata(path.parent().unwrap()).unwrap().permissions().mode() & 0o777, 0o700);
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
    save(&path, &ids).unwrap();
    assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600, "an existing loose file is tightened");
    let leftovers: Vec<_> = std::fs::read_dir(path.parent().unwrap()).unwrap().filter_map(Result::ok).filter(|e| e.file_name() != "identities.yaml").collect();
    assert!(leftovers.is_empty(), "no temporary file left: {leftovers:?}");
    std::fs::write(&path, "identities: 3\n").unwrap();
    assert!(load(&path).is_err());
}

#[test]
fn io_failures_are_errors_naming_the_path() {
    let dir = std::env::temp_dir().join(format!("tmpmail-io-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("a-file"), "x").unwrap();
    let under_a_file = dir.join("a-file/identities.yaml");
    assert!(save(&under_a_file, &Identities::default()).unwrap_err().contains("a-file"));
    assert!(load(&dir).unwrap_err().contains(&dir.display().to_string()), "a directory is not a file");
    assert!(save(std::path::Path::new("/"), &Identities::default()).is_err(), "no parent");
    std::fs::create_dir_all(dir.join("taken/identities.yaml")).unwrap();
    assert!(save(&dir.join("taken/identities.yaml"), &Identities::default()).is_err(), "rename onto a directory");
    assert!(std::fs::read_dir(dir.join("taken")).unwrap().all(|e| e.unwrap().file_name() == "identities.yaml"), "the temporary file is removed on failure");
}
