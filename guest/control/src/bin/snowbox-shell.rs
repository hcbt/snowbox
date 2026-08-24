fn main() {
    #[cfg(target_os = "linux")]
    if let Err(e) = run() {
        eprintln!("snowbox-shell: {e}");
        std::process::exit(1);
    }
    #[cfg(not(target_os = "linux"))]
    {
        eprintln!("snowbox-shell runs in the guest");
        std::process::exit(1);
    }
}

#[cfg(target_os = "linux")]
fn run() -> std::io::Result<()> {
    let listener = snowbox_guest::vsock::bind_retry(53)?;
    loop {
        let stream = listener.accept()?;
        std::thread::spawn(move || {
            if let Err(e) = snowbox_guest::shell::handle_socket(stream) {
                eprintln!("snowbox-shell: {e}");
            }
        });
    }
}
