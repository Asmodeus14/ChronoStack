//! `clockd` — the ChronoStack controller.
//!
//! ```text
//! clockd serve [--page PATH] [--lib-src DIR] [--lib-dst DIR] [--listen ADDR]
//! clockd healthcheck [--addr HOST:PORT]
//! ```
//!
//! `serve` requires `CHRONOSTACK_TOKEN` in the environment.

#[cfg(target_os = "linux")]
mod clock;
#[cfg(target_os = "linux")]
mod server;

fn main() {
    #[cfg(target_os = "linux")]
    std::process::exit(linux::main());
    #[cfg(not(target_os = "linux"))]
    {
        eprintln!("clockd runs only on Linux (inside its container)");
        std::process::exit(2);
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use std::io::{Read, Write};
    use std::net::TcpStream;
    use std::time::Duration;

    use chronostack_core::paths;

    use crate::{clock, server};

    fn arg(args: &[String], name: &str, default: &str) -> String {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .cloned()
            .unwrap_or_else(|| default.to_string())
    }

    pub fn main() -> i32 {
        let args: Vec<String> = std::env::args().skip(1).collect();
        match args.first().map(String::as_str) {
            Some("serve") => serve(&args),
            Some("healthcheck") => healthcheck(&arg(
                &args,
                "--addr",
                &format!("127.0.0.1:{}", paths::CLOCKD_PORT),
            )),
            _ => {
                eprintln!(
                    "usage: clockd serve [--page PATH] [--lib-src DIR] [--lib-dst DIR] [--listen ADDR] | clockd healthcheck"
                );
                2
            }
        }
    }

    fn serve(args: &[String]) -> i32 {
        let token = std::env::var("CHRONOSTACK_TOKEN").unwrap_or_default();
        if token.len() < 32 {
            eprintln!(
                "clockd: CHRONOSTACK_TOKEN must be set (>= 32 chars); refusing to start an unauthenticated API"
            );
            return 2;
        }
        let page = arg(args, "--page", paths::CLOCK);
        let lib_src = arg(args, "--lib-src", "/opt/chronostack/lib");
        let lib_dst = arg(args, "--lib-dst", paths::LIB_DIR);
        let listen = arg(args, "--listen", &format!("0.0.0.0:{}", paths::CLOCKD_PORT));

        match clock::install_libs(lib_src.as_ref(), lib_dst.as_ref()) {
            Ok(n) => server::log(&format!(
                r#"{{"event":"libs_installed","count":{n},"dir":"{lib_dst}"}}"#
            )),
            Err(e) => {
                eprintln!("clockd: installing preload libraries failed: {e}");
                return 1;
            }
        }
        let (clock, reattached) = match clock::Clock::open_or_init(page.as_ref()) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("clockd: opening clock page {page} failed: {e}");
                return 1;
            }
        };
        let st = clock.state();
        server::log(&format!(
            r#"{{"event":"{}","page":"{page}","virtual_ns":{},"offset_ns":{},"session_id":"{}"}}"#,
            if reattached {
                "page_reattached"
            } else {
                "page_initialized"
            },
            st.virtual_ns,
            st.offset_ns,
            st.session_id
        ));
        server::serve(&clock, &listen, &token)
    }

    fn healthcheck(addr: &str) -> i32 {
        let check = || -> std::io::Result<bool> {
            let mut s = TcpStream::connect(addr)?;
            s.set_read_timeout(Some(Duration::from_secs(2)))?;
            s.write_all(b"GET /v1/health HTTP/1.0\r\nHost: localhost\r\n\r\n")?;
            let mut buf = String::new();
            s.read_to_string(&mut buf)?;
            Ok(buf.starts_with("HTTP/1.1 200") || buf.starts_with("HTTP/1.0 200"))
        };
        match check() {
            Ok(true) => 0,
            _ => 1,
        }
    }
}
