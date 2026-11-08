//! The scratch directory, tested on its own.
//!
//! A test writes its files into a scratch directory. Two tests handed the
//! same directory read each other's files: a models root meant to be empty
//! serves the model another test wrote into it. These tests hold the clock
//! still, because the clock is what two tests can share: every test process
//! runs its test on a thread with the same id, and a clock that reads only to
//! the microsecond gives two of them the same reading.

use std::fs;
use std::time::SystemTime;

use crate::fixtures::Scratch;

#[test]
fn directories_made_in_one_instant_do_not_share_files() {
    let instant = SystemTime::now();
    let first = Scratch::made_at("same-instant", instant);
    let second = Scratch::made_at("same-instant", instant);

    fs::write(first.path().join("model.gguf"), b"first").expect("a writable scratch directory");

    let held = fs::read_dir(second.path())
        .expect("the second directory is there")
        .count();
    assert_eq!(held, 0, "the second directory holds the first one's file");
}

#[test]
fn dropping_one_directory_leaves_another_made_in_the_same_instant() {
    let instant = SystemTime::now();
    let kept = Scratch::made_at("same-instant", instant);
    let model = kept.path().join("model.gguf");
    fs::write(&model, b"kept").expect("a writable scratch directory");

    drop(Scratch::made_at("same-instant", instant));

    let read = fs::read(&model).expect("the kept directory's file is still there");
    assert_eq!(read, b"kept");
}
