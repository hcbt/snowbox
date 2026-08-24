fn main() {
    #[cfg(target_os = "linux")]
    if let Err(e) = run() {
        eprintln!("snowbox-agent: {e}");
        std::process::exit(1);
    }
    #[cfg(not(target_os = "linux"))]
    {
        eprintln!("snowbox-agent runs in the guest");
        std::process::exit(1);
    }
}

#[cfg(target_os = "linux")]
fn run() -> std::io::Result<()> {
    let listener = snowbox_guest::vsock::bind_retry(52)?;
    loop {
        let stream = listener.accept()?;
        std::thread::spawn(move || {
            if let Err(e) = snowbox_guest::agent::handle_socket(stream) {
                eprintln!("snowbox-agent: {e}");
            }
        });
    }
}
