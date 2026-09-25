//! A libtest-compatible runner that owns the main thread.

use std::any::Any;
use std::cell::RefCell;
use std::io::Write;
use std::panic::{self, AssertUnwindSafe};
use std::time::Instant;

use crate::__private::TestCase;
use crate::app::{TestApp, TestContext};
use crate::exec::block_on;

struct Options {
    filters: Vec<String>,
    skip: Vec<String>,
    exact: bool,
    list: bool,
    native: bool,
    ignored: bool,
}

fn parse(args: impl Iterator<Item = String>) -> Options {
    let mut options =
        Options { filters: Vec::new(), skip: Vec::new(), exact: false, list: false, native: false, ignored: false };
    let mut args = args.peekable();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--exact" => options.exact = true,
            "--list" => options.list = true,
            "--native" => options.native = true,
            "--ignored" => options.ignored = true,
            "--skip" => options.skip.extend(args.next()),
            // Accepted for libtest/IDE compatibility; tests always run
            // serially on the main thread and output is not captured.
            "--nocapture" | "--show-output" | "--include-ignored" | "-q" | "--quiet" => {}
            "--test-threads" | "--format" | "--color" | "-Z" | "--logfile" => {
                args.next();
            }
            a if a.starts_with("--test-threads=") || a.starts_with("--format=") || a.starts_with("--color=") => {}
            a if a.starts_with('-') => eprintln!("mitsuami-test: ignoring unknown flag {a}"),
            _ => options.filters.push(arg),
        }
    }
    options
}

fn display_name(case: &TestCase) -> &'static str {
    case.name.split_once("::").map_or(case.name, |(_, rest)| rest)
}

fn selected(options: &Options, name: &str) -> bool {
    let matches = |f: &String| if options.exact { name == f } else { name.contains(f.as_str()) };
    (options.filters.is_empty() || options.filters.iter().any(matches)) && !options.skip.iter().any(matches)
}

thread_local! {
    static PANIC_MESSAGE: RefCell<Option<String>> = const { RefCell::new(None) };
}

fn payload_text(payload: &(dyn Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|s| s.to_string())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "panic with a non-string payload".into())
}

pub fn run_main() {
    let options = parse(std::env::args().skip(1));
    let mut cases: Vec<&TestCase> = inventory::iter::<TestCase>.into_iter().collect();
    cases.sort_by_key(|c| c.name);
    let total = cases.len();
    let cases: Vec<&TestCase> = cases.into_iter().filter(|c| selected(&options, display_name(c))).collect();

    if options.list {
        for case in &cases {
            println!("{}: test", display_name(case));
        }
        println!("\n{} tests, 0 benchmarks", cases.len());
        return;
    }
    if options.ignored {
        println!("\nrunning 0 tests\n\ntest result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out\n");
        return;
    }
    if options.native {
        eprintln!(
            "mitsuami-test: --native needs a native backend for this platform; none is available yet \
             (they arrive in milestones M1–M3). Run without --native for headless tests."
        );
        std::process::exit(2);
    }

    panic::set_hook(Box::new(|info| {
        let location = info.location().map(|l| format!(" at {}:{}", l.file(), l.line())).unwrap_or_default();
        let message = format!("{}{location}", payload_text(info.payload()));
        PANIC_MESSAGE.with(|m| *m.borrow_mut() = Some(message));
    }));

    println!("\nrunning {} tests", cases.len());
    let started = Instant::now();
    let mut failures = Vec::new();
    for case in &cases {
        let name = display_name(case);
        print!("test {name} ... ");
        let _ = std::io::stdout().flush();
        let context = TestContext {
            name: name.to_owned(),
            file_prefix: case.name.replace("::", "__"),
            manifest_dir: case.manifest_dir,
        };
        let run = case.run;
        let result = panic::catch_unwind(AssertUnwindSafe(|| {
            let app = TestApp::new(context);
            block_on(run(app));
        }));
        match result {
            Ok(()) => println!("ok"),
            Err(payload) => {
                println!("FAILED");
                let message = PANIC_MESSAGE.with(|m| m.borrow_mut().take()).unwrap_or_else(|| payload_text(&*payload));
                failures.push((name, message));
            }
        }
    }
    let _ = panic::take_hook();

    if !failures.is_empty() {
        println!("\nfailures:\n");
        for (name, message) in &failures {
            println!("---- {name} ----\n{message}\n");
        }
        println!("failures:");
        for (name, _) in &failures {
            println!("    {name}");
        }
    }
    let status = if failures.is_empty() { "ok" } else { "FAILED" };
    println!(
        "\ntest result: {status}. {} passed; {} failed; 0 ignored; 0 measured; {} filtered out; finished in {:.2}s\n",
        cases.len() - failures.len(),
        failures.len(),
        total - cases.len(),
        started.elapsed().as_secs_f64()
    );
    if !failures.is_empty() {
        std::process::exit(101);
    }
}
