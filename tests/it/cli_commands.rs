//! The commands a person types, run the way the binary is run.
//!
//! `README.md` promises what `check`, `launch` and `bench` print and how they
//! fail. Every other target drives the library; this drives the binary, so a
//! promise the argument table or a command's own output breaks fails here.
//!
//! The cases that start a child need the stub on the search path, which
//! `support::spawned` builds from symlinks, so they are Unix only like the
//! other targets that do.

use std::fs;
use std::path::Path;
use std::process::{Command, Output};
use std::time::Duration;

use crate::support::{MODEL, ModelsRoot, catalog_text};

/// A models root no machine has.
const NOWHERE: &str = "/somewhere/that/is/not/there";

/// Runs `model-router` with these arguments against this models root.
fn model_router(args: &[&str], root: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_model-router"))
        .args(args)
        .env("MAESTRO_MODELS_ROOT", root)
        .env_remove("MAESTRO_MEMORY_BUDGET_MIB")
        .env_remove("MAESTRO_IDLE_UNLOAD_SECONDS")
        .env_remove("MAESTRO_ADMISSION_WAIT_SECONDS")
        .output()
        .expect("the router binary is built by cargo test")
}

/// What a stream carried, as text.
fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// A catalog file inside the root, holding this text.
fn written(root: &ModelsRoot, text: &str) -> String {
    let path = root.path().join("catalog.toml");
    fs::write(&path, text).expect("a catalog in the temporary directory");
    path.display().to_string()
}

#[test]
fn a_command_nobody_knows_is_answered_with_the_usage() {
    let output = model_router(&["frobnicate"], Path::new(NOWHERE));
    assert!(!output.status.success(), "an unknown command fails");
    assert!(
        text(&output.stderr).contains("usage: model-router check <catalog>"),
        "and says what the commands are:\n{}",
        text(&output.stderr)
    );
}

#[test]
fn the_version_names_the_release_and_the_commit_it_was_built_from() {
    let output = model_router(&["--version"], Path::new(NOWHERE));
    assert!(output.status.success(), "{}", text(&output.stderr));
    let said = text(&output.stdout);
    assert!(
        said.starts_with(&format!("model-router {} (", env!("CARGO_PKG_VERSION")))
            && said.trim_end().ends_with(')'),
        "the release, then the commit in brackets -- 'unrecorded' when the \
         build was not told one -- so a running router can be traced to its \
         source:\n{said}"
    );
}

#[test]
fn check_counts_the_models_a_usable_catalog_carries_against_its_root() {
    let root = ModelsRoot::with(&[MODEL]);
    let catalog = written(&root, &catalog_text(""));

    let output = model_router(&["check", &catalog], root.path());
    assert!(output.status.success(), "{}", text(&output.stderr));
    assert!(
        text(&output.stdout).contains("is valid: 1 models, 1 declared and 0 found under the root"),
        "the one entry is counted, and read against the root:\n{}",
        text(&output.stdout)
    );
}

#[test]
fn check_without_a_models_root_says_it_checked_the_shape_only() {
    let root = ModelsRoot::with(&[MODEL]);
    let catalog = written(&root, &catalog_text(""));

    let output = model_router(&["check", &catalog], Path::new(NOWHERE));
    assert!(output.status.success(), "{}", text(&output.stderr));
    assert!(
        text(&output.stdout).contains("is valid: 1 models (shape only"),
        "a machine holding none of the models can check the shape and says \
         that is all it did:\n{}",
        text(&output.stdout)
    );
}

#[test]
fn check_without_a_configured_root_reads_models_under_the_home_directory() {
    let home = ModelsRoot::with(&[&format!("models/{MODEL}")]);
    let catalog = written(&home, &catalog_text(""));

    // Run from inside that root, where a `models` directory relative to the
    // working directory is not there, so only the home directory finds it.
    let output = Command::new(env!("CARGO_BIN_EXE_model-router"))
        .args(["check", &catalog])
        .current_dir(home.path().join("models"))
        .env_remove("MAESTRO_MODELS_ROOT")
        .env("HOME", home.path())
        .env("USERPROFILE", home.path())
        .output()
        .expect("the router binary is built by cargo test");
    assert!(output.status.success(), "{}", text(&output.stderr));
    assert!(
        text(&output.stdout).contains("is valid: 1 models, 1 declared and 0 found under the root"),
        "read against 'models' under the home directory:\n{}",
        text(&output.stdout)
    );
}

#[test]
fn check_names_every_problem_of_an_unusable_catalog_not_only_the_first() {
    let root = ModelsRoot::with(&[MODEL]);
    let catalog = written(
        &root,
        &catalog_text(
            "\n[models.alpha]\ncolour = \"red\"\n\
             \n[models.beta]\npath = \"b.gguf\"\nresidency = \"sometimes\"\n",
        ),
    );

    let output = model_router(&["check", &catalog], Path::new(NOWHERE));
    assert!(!output.status.success(), "an unusable catalog fails");
    let said = text(&output.stderr);
    assert!(said.contains("is not usable:"), "{said}");
    assert!(
        said.contains("entry 'alpha'") && said.contains("entry 'beta'"),
        "both entries' problems are listed in one run:\n{said}"
    );
}

#[test]
fn check_says_when_the_catalog_cannot_be_read() {
    let output = model_router(&["check", "/somewhere/catalog.toml"], Path::new(NOWHERE));
    assert!(!output.status.success(), "a missing catalog fails");
    assert!(
        text(&output.stderr).contains("cannot read /somewhere/catalog.toml"),
        "{}",
        text(&output.stderr)
    );
}

// The argument table decides by the command and the number of operands
// together, so a known command with the wrong number of them is not that
// command: it is the usage. The catalog named is one that cannot be read, so a
// table that took these for a command would say so instead.
#[test]
fn a_known_command_with_the_wrong_number_of_operands_is_answered_with_the_usage() {
    for args in [
        ["launch", "/somewhere/catalog.toml"].as_slice(),
        &["check", "/somewhere/catalog.toml", "gemma3"],
    ] {
        let output = model_router(args, Path::new(NOWHERE));
        assert!(!output.status.success(), "{args:?} fails");
        assert!(
            text(&output.stderr).contains("usage: model-router check <catalog>"),
            "{args:?} is answered with the usage:\n{}",
            text(&output.stderr)
        );
    }
}

#[cfg(unix)]
mod with_the_stub {
    use super::*;
    use crate::support::spawned::{RouterProcess, SearchPath};
    use crate::support::{get, request, status};

    #[test]
    fn serve_requires_the_key_its_environment_sets() {
        let root = ModelsRoot::with(&[MODEL]);
        let catalog = written(&root, &catalog_text(""));
        let search = SearchPath::with_stub();

        let mut router = RouterProcess::serve_with(
            Path::new(&catalog),
            &root,
            &search,
            &[("MAESTRO_API_KEY", "s3cret")],
        );
        let reply = request(router.address(), &get("/v1/models"));
        assert_eq!(
            status(&reply),
            Some(401),
            "the key set in the service's environment is the one it requires:\n{reply}"
        );
    }

    /// Runs `model-router` with the stub found on the search path as
    /// `llama-server`, the way a real server is found in the field.
    fn with_stub(args: &[&str], root: &ModelsRoot) -> Output {
        with_stub_and(args, root, &[])
    }

    /// Runs `model-router` as [`with_stub`] does, with these variables set in
    /// its environment as well.
    fn with_stub_and(args: &[&str], root: &ModelsRoot, variables: &[(&str, &str)]) -> Output {
        let search = SearchPath::with_stub();
        Command::new(env!("CARGO_BIN_EXE_model-router"))
            .args(args)
            .env("PATH", search.value())
            .env("MAESTRO_MODELS_ROOT", root.path())
            .env_remove("MAESTRO_MEMORY_BUDGET_MIB")
            .envs(variables.iter().copied())
            .output()
            .expect("the router binary is built by cargo test")
    }

    // `serve` and `bench` find the server binary before they read an address
    // or a model name, so these run where one is found: the stub.
    #[test]
    fn launch_and_bench_name_a_model_the_catalog_does_not_carry() {
        let root = ModelsRoot::with(&[MODEL]);
        let catalog = written(&root, &catalog_text(""));

        for (command, refusal) in [
            ("launch", "carries no model called 'nowhere'"),
            ("bench", "no entry called 'nowhere'"),
        ] {
            let output = with_stub(&[command, &catalog, "nowhere"], &root);
            assert!(
                !output.status.success(),
                "{command}: an unknown model fails"
            );
            assert!(
                text(&output.stderr).contains(refusal),
                "{command}: {}",
                text(&output.stderr)
            );
        }
    }

    #[test]
    fn serve_refuses_an_address_that_is_not_one() {
        let root = ModelsRoot::with(&[MODEL]);
        let catalog = written(&root, &catalog_text(""));

        let output = with_stub(&["serve", &catalog, "not-an-address"], &root);
        assert!(
            !output.status.success(),
            "a bad address fails before binding"
        );
        assert!(
            text(&output.stderr).contains("'not-an-address' is not an address to bind"),
            "{}",
            text(&output.stderr)
        );
    }

    /// Everything `serve` says on standard output, from the address on, once
    /// a signal has ended it.
    fn served(catalog: &str, root: &ModelsRoot) -> String {
        let search = SearchPath::with_stub();
        let mut router = RouterProcess::serve(Path::new(catalog), root, &search);
        let first = router.address();
        router.terminate();
        router
            .exited_within(Duration::from_secs(20))
            .unwrap_or_else(|| panic!("serving {first} outlived its signal:\n{}", router.stderr()));
        router.rest_of_stdout()
    }

    // Memory a resident holds is gone for good, so `serve` says how much at
    // the start, and says nothing about it when no entry is resident.
    #[test]
    fn serve_says_what_residents_reserve_only_when_there_are_some() {
        let root = ModelsRoot::with(&[MODEL]);

        let said = served(&written(&root, &catalog_text("")), &root);
        assert!(
            !said.contains("residents reserve"),
            "no entry is resident, so nothing is reserved:\n{said}"
        );

        let said = served(
            &written(&root, &catalog_text("residency = \"resident\"\n")),
            &root,
        );
        assert!(
            said.contains("residents reserve 512 MiB"),
            "the resident's estimate is reserved:\n{said}"
        );
    }

    // Without an address `serve` binds the default port, which no test can
    // count on being free. A setting only `serve` reads, set to nonsense,
    // ends it before the bind, and says it was `serve` that read it: `check`
    // and `bench` would have succeeded here, and the usage names no setting.
    #[test]
    fn serve_without_an_address_reads_its_settings_before_it_binds() {
        let root = ModelsRoot::with(&[MODEL]);
        let catalog = written(&root, &catalog_text(""));

        let output = with_stub_and(
            &["serve", &catalog],
            &root,
            &[("MAESTRO_IDLE_UNLOAD_SECONDS", "soon")],
        );
        assert!(!output.status.success(), "a nonsense setting fails");
        assert!(
            text(&output.stderr).contains("MAESTRO_IDLE_UNLOAD_SECONDS carries 'soon'"),
            "{}",
            text(&output.stderr)
        );
    }

    // What a child writes reaches the router's own standard error under the
    // entry it came from, so a service manager's journal keeps it. Only that
    // passing on writes the entry before the child's line: a failure quotes
    // the child's last words indented, under the failure.
    #[test]
    fn a_childs_lines_reach_standard_error_under_its_entry() {
        let root = ModelsRoot::with(&[MODEL]);
        let catalog = written(
            &root,
            &catalog_text(
                "\n[models.gemma3.flags]\nready-after = \"600000\"\nexit-after = \"250\"\n",
            ),
        );

        let output = with_stub(&["launch", &catalog, "gemma3"], &root);
        assert!(!output.status.success(), "a child that exits never answers");
        assert!(
            text(&output.stderr).contains("gemma3: stub-llama-server: exiting"),
            "{}",
            text(&output.stderr)
        );
    }

    #[test]
    fn launch_starts_an_entry_proves_it_answers_and_stops_it() {
        let root = ModelsRoot::with(&[MODEL]);
        let catalog = written(&root, &catalog_text(""));

        let output = with_stub(&["launch", &catalog, "gemma3"], &root);
        assert!(output.status.success(), "{}", text(&output.stderr));
        let said = text(&output.stdout);
        assert!(
            said.contains("gemma3 is ready at http://127.0.0.1:"),
            "{said}"
        );
        assert!(said.contains("gemma3 stopped"), "{said}");
    }

    #[test]
    fn bench_loads_an_entry_and_reports_what_it_declared() {
        let root = ModelsRoot::with(&[MODEL]);
        let catalog = written(&root, &catalog_text(""));

        let output = with_stub(&["bench", &catalog, "gemma3"], &root);
        assert!(output.status.success(), "{}", text(&output.stderr));
        let said = text(&output.stdout);
        assert!(
            said.contains("entry") && said.contains("declared") && said.contains("measured"),
            "the table has its heading:\n{said}"
        );
        let row = said
            .lines()
            .find(|line| line.starts_with("gemma3"))
            .unwrap_or_else(|| panic!("a row for the entry:\n{said}"));
        assert!(
            row.contains("512") && row.contains('s'),
            "the row carries the declared estimate and a load time: {row}"
        );
    }

    #[test]
    fn bench_without_a_model_measures_every_entry() {
        let root = ModelsRoot::with(&[MODEL]);
        let catalog = written(&root, &catalog_text(""));

        let output = with_stub(&["bench", &catalog], &root);
        assert!(output.status.success(), "{}", text(&output.stderr));
        let said = text(&output.stdout);
        assert!(
            said.lines().any(|line| line.starts_with("gemma3")),
            "a row for the catalog's one entry:\n{said}"
        );
    }
}
