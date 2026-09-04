fn main() {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("--self-test") => match native::run_self_test() {
            Ok(()) => {
                println!("native self-test: ok");
            }
            Err(err) => {
                eprintln!("native self-test: {err}");
                std::process::exit(1);
            }
        },
        _ => {
            eprintln!("usage: native --self-test");
            std::process::exit(2);
        }
    }
}
