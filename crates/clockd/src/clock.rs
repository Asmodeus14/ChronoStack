//! The clock page writer (ADR-004, ADR-017).

use std::fs::{self, OpenOptions};
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::Mutex;
use std::sync::atomic::{Ordering, fence};

use chronostack_core::api::ClockState;
use chronostack_core::page::{ClockPage, FILE_SIZE, LAYOUT_VERSION, MAGIC};
use memmap2::MmapMut;

fn clock_ns(id: libc::clockid_t) -> i64 {
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: valid pointer to a local timespec; clock ids used here always exist on Linux.
    #[allow(unsafe_code)]
    unsafe {
        libc::clock_gettime(id, &mut ts);
    }
    ts.tv_sec * 1_000_000_000 + ts.tv_nsec
}

pub fn mono_ns() -> i64 {
    clock_ns(libc::CLOCK_MONOTONIC)
}

pub fn real_ns() -> i64 {
    clock_ns(libc::CLOCK_REALTIME)
}

fn random_u64() -> u64 {
    let mut b = [0u8; 8];
    if fs::File::open("/dev/urandom")
        .and_then(|mut f| io::Read::read_exact(&mut f, &mut b))
        .is_ok()
    {
        u64::from_le_bytes(b)
    } else {
        real_ns() as u64 ^ (std::process::id() as u64) << 32
    }
}

/// Owns the mapping. All mutations go through `&self` methods guarded by a
/// mutex, so there is exactly one writer even if the server becomes threaded.
pub struct Clock {
    map: MmapMut,
    write_lock: Mutex<()>,
}

impl Clock {
    /// Open an existing valid page (keeping its state, so time never jumps back
    /// across controller restarts) or initialize a fresh one at virtual == real.
    pub fn open_or_init(path: &Path) -> io::Result<(Clock, bool)> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)?;
        if file.metadata()?.len() < FILE_SIZE as u64 {
            file.set_len(FILE_SIZE as u64)?;
        }
        fs::set_permissions(path, fs::Permissions::from_mode(0o644))?;
        // SAFETY: the file is at least FILE_SIZE bytes; the mapping lives as long as `Clock`.
        #[allow(unsafe_code)]
        let map = unsafe { MmapMut::map_mut(&file)? };
        let clock = Clock {
            map,
            write_lock: Mutex::new(()),
        };
        let p = clock.page();
        let valid = p.magic.load(Ordering::Acquire) == MAGIC
            && p.layout_version.load(Ordering::Relaxed) == LAYOUT_VERSION
            && p.page_size.load(Ordering::Relaxed) as usize == size_of::<ClockPage>();
        if valid {
            // Repair a write interrupted by a crash (odd seq) so readers stop waiting.
            let seq = p.seq.load(Ordering::Relaxed);
            if seq & 1 == 1 {
                p.seq.store(seq + 1, Ordering::Release);
            }
            return Ok((clock, true));
        }
        clock.init();
        Ok((clock, false))
    }

    /// Anonymous page for tests.
    #[cfg(test)]
    pub fn anonymous() -> Clock {
        let clock = Clock {
            map: MmapMut::map_anon(FILE_SIZE).unwrap(),
            write_lock: Mutex::new(()),
        };
        clock.init();
        clock
    }

    fn init(&self) {
        let p = self.page();
        p.magic.store(0, Ordering::Relaxed);
        p.layout_version.store(LAYOUT_VERSION, Ordering::Relaxed);
        p.page_size
            .store(size_of::<ClockPage>() as u32, Ordering::Relaxed);
        p.seq.store(0, Ordering::Relaxed);
        p.offset_ns.store(real_ns() - mono_ns(), Ordering::Relaxed);
        p.frozen_virtual_ns.store(0, Ordering::Relaxed);
        p.frozen.store(0, Ordering::Relaxed);
        p.flags.store(0, Ordering::Relaxed);
        p.generation.store(0, Ordering::Relaxed);
        p.session_id.store(random_u64(), Ordering::Relaxed);
        // Publish last: readers validate magic before trusting anything else.
        p.magic.store(MAGIC, Ordering::Release);
    }

    pub fn page(&self) -> &ClockPage {
        // SAFETY: the mapping is page-aligned (>= 64), at least FILE_SIZE bytes,
        // and every field of ClockPage is an atomic, valid for any bit pattern.
        #[allow(unsafe_code)]
        unsafe {
            &*(self.map.as_ptr() as *const ClockPage)
        }
    }

    /// Run `f` inside the seqlock write window (ADR-017 writer protocol).
    fn write<R>(&self, f: impl FnOnce(&ClockPage) -> R) -> R {
        let _guard = self.write_lock.lock().unwrap_or_else(|e| e.into_inner());
        let p = self.page();
        let s = p.seq.load(Ordering::Relaxed);
        p.seq.store(s + 1, Ordering::Relaxed);
        // SeqCst, not Release: clock reads inside `f` must not precede the odd seq's visibility.
        fence(Ordering::SeqCst);
        let r = f(p);
        p.generation
            .store(p.generation.load(Ordering::Relaxed) + 1, Ordering::Relaxed);
        p.seq.store(s + 2, Ordering::Release);
        r
    }

    /// Consistent snapshot (seqlock read).
    pub fn state(&self) -> ClockState {
        let p = self.page();
        loop {
            let s1 = p.seq.load(Ordering::Acquire);
            if s1 & 1 == 1 {
                std::hint::spin_loop();
                continue;
            }
            let off = p.offset_ns.load(Ordering::Relaxed);
            let frozen = p.frozen.load(Ordering::Relaxed) != 0;
            let fv = p.frozen_virtual_ns.load(Ordering::Relaxed);
            let generation = p.generation.load(Ordering::Relaxed);
            let session = p.session_id.load(Ordering::Relaxed);
            let mono = mono_ns();
            fence(Ordering::Acquire);
            if p.seq.load(Ordering::Relaxed) != s1 {
                continue;
            }
            let real = real_ns();
            let virtual_ns = if frozen { fv } else { mono + off };
            return ClockState {
                virtual_ns,
                real_ns: real,
                offset_ns: virtual_ns - real,
                frozen,
                generation,
                session_id: format!("{session:016x}"),
                layout_version: LAYOUT_VERSION,
            };
        }
    }

    /// Move virtual time forward by `by_ns` (> 0).
    pub fn advance(&self, by_ns: i64) {
        self.write(|p| {
            if p.frozen.load(Ordering::Relaxed) != 0 {
                p.frozen_virtual_ns.fetch_add(by_ns, Ordering::Relaxed);
            } else {
                p.offset_ns.fetch_add(by_ns, Ordering::Relaxed);
            }
        });
    }

    pub fn freeze(&self) {
        self.write(|p| {
            if p.frozen.load(Ordering::Relaxed) == 0 {
                let v = mono_ns() + p.offset_ns.load(Ordering::Relaxed);
                p.frozen_virtual_ns.store(v, Ordering::Relaxed);
                p.frozen.store(1, Ordering::Relaxed);
            }
        });
    }

    pub fn unfreeze(&self) {
        self.write(|p| {
            if p.frozen.load(Ordering::Relaxed) != 0 {
                let off = p.frozen_virtual_ns.load(Ordering::Relaxed) - mono_ns();
                p.offset_ns.store(off, Ordering::Relaxed);
                p.frozen.store(0, Ordering::Relaxed);
            }
        });
    }
}

/// Copy `*.so` from `src` into `dst` via temp file + rename, so a library that
/// running processes have mapped is never modified in place.
pub fn install_libs(src: &Path, dst: &Path) -> io::Result<usize> {
    fs::create_dir_all(dst)?;
    fs::set_permissions(dst, fs::Permissions::from_mode(0o755))?;
    let mut n = 0;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let name = entry.file_name();
        let Some(name_str) = name.to_str() else {
            continue;
        };
        if !name_str.ends_with(".so") {
            continue;
        }
        let tmp = dst.join(format!(".{name_str}.tmp"));
        fs::copy(entry.path(), &tmp)?;
        fs::set_permissions(&tmp, fs::Permissions::from_mode(0o644))?;
        fs::rename(&tmp, dst.join(&name))?;
        n += 1;
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;

    const S: i64 = 1_000_000_000;

    #[test]
    fn fresh_page_starts_at_real_time() {
        let c = Clock::anonymous();
        let st = c.state();
        assert!(st.offset_ns.abs() < S / 10, "offset {}", st.offset_ns);
        assert!(!st.frozen);
    }

    #[test]
    fn advance_freeze_unfreeze() {
        let c = Clock::anonymous();
        c.advance(7 * 86_400 * S);
        let st = c.state();
        assert!((st.offset_ns - 7 * 86_400 * S).abs() < S / 10);
        c.freeze();
        let a = c.state().virtual_ns;
        std::thread::sleep(std::time::Duration::from_millis(30));
        assert_eq!(c.state().virtual_ns, a, "frozen time must not move");
        c.advance(3600 * S);
        assert_eq!(
            c.state().virtual_ns,
            a + 3600 * S,
            "advance while frozen moves the frozen value"
        );
        c.unfreeze();
        let b = c.state().virtual_ns;
        assert!(
            b >= a + 3600 * S && b - (a + 3600 * S) < S / 10,
            "unfreeze resumes without a jump"
        );
        assert_eq!(c.state().generation, 4);
    }

    #[test]
    fn readers_never_see_time_go_backwards() {
        let c = Arc::new(Clock::anonymous());
        let stop = Arc::new(AtomicBool::new(false));
        let readers: Vec<_> = (0..4)
            .map(|_| {
                let (c, stop) = (c.clone(), stop.clone());
                std::thread::spawn(move || {
                    let mut prev = c.state().virtual_ns;
                    let mut back = 0u64;
                    while !stop.load(Ordering::Relaxed) {
                        let v = c.state().virtual_ns;
                        if v < prev {
                            back += 1;
                        }
                        prev = v;
                    }
                    back
                })
            })
            .collect();
        let end = std::time::Instant::now() + std::time::Duration::from_millis(500);
        while std::time::Instant::now() < end {
            c.freeze();
            c.advance(1_000_000);
            c.unfreeze();
            c.advance(1_000_000);
        }
        stop.store(true, Ordering::Relaxed);
        let back: u64 = readers.into_iter().map(|h| h.join().unwrap()).sum();
        assert_eq!(back, 0);
    }

    #[test]
    fn reopen_keeps_state_and_repairs_odd_seq() {
        let dir = std::env::temp_dir().join(format!("clockd-test-{}", std::process::id()));
        let path = dir.join("clock");
        let (c, reattached) = Clock::open_or_init(&path).unwrap();
        assert!(!reattached);
        c.advance(86_400 * S);
        c.page().seq.fetch_add(1, Ordering::Relaxed); // simulate crash mid-write
        drop(c);
        let (c2, reattached) = Clock::open_or_init(&path).unwrap();
        assert!(reattached);
        assert_eq!(c2.page().seq.load(Ordering::Relaxed) & 1, 0);
        assert!((c2.state().offset_ns - 86_400 * S).abs() < S / 10);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn installs_libs_atomically() {
        let base = std::env::temp_dir().join(format!("clockd-libs-{}", std::process::id()));
        let (src, dst) = (base.join("src"), base.join("dst"));
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("libchronoclock-x86_64-gnu.so"), b"elf").unwrap();
        fs::write(src.join("README"), b"skip").unwrap();
        assert_eq!(install_libs(&src, &dst).unwrap(), 1);
        assert_eq!(
            fs::read(dst.join("libchronoclock-x86_64-gnu.so")).unwrap(),
            b"elf"
        );
        assert!(!dst.join("README").exists());
        fs::remove_dir_all(base).unwrap();
    }
}
