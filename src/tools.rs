//! Backend: drives the bundled `adb` and `scrcpy` binaries.
//! All long-running work runs on a worker thread so the UI never blocks.

use std::net::{SocketAddr, TcpStream};
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use eframe::egui;

/// Port range Android's wireless-debugging service picks from.
const SCAN_LO: u32 = 30000;
const SCAN_HI: u32 = 49999;
const SCAN_WORKERS: u32 = 300;

pub struct Shared {
    pub log: Mutex<String>,
    pub busy: AtomicBool,
    pub paired: AtomicBool,
    pub connected: AtomicBool,
    pub mirror_target: Mutex<Option<String>>,
}

impl Shared {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            log: Mutex::new(String::new()),
            busy: AtomicBool::new(false),
            paired: AtomicBool::new(false),
            connected: AtomicBool::new(false),
            mirror_target: Mutex::new(None),
        })
    }
}

#[derive(Clone)]
pub struct Backend {
    pub shared: Arc<Shared>,
    pub dir: String,
    pub ctx: egui::Context,
}

impl Backend {
    fn log(&self, msg: impl AsRef<str>) {
        if let Ok(mut l) = self.shared.log.lock() {
            l.push_str(msg.as_ref());
            l.push('\n');
        }
        self.ctx.request_repaint();
    }

    fn tool(&self, name: &str) -> PathBuf {
        PathBuf::from(&self.dir).join(name)
    }

    /// Run `adb <args>`, capture combined stdout+stderr, log it, return stdout.
    fn adb(&self, args: &[&str]) -> std::io::Result<String> {
        let out = Command::new(self.tool("adb")).args(args).output()?;
        let so = String::from_utf8_lossy(&out.stdout).trim().to_string();
        let se = String::from_utf8_lossy(&out.stderr).trim().to_string();
        if !so.is_empty() {
            self.log(&so);
        }
        if !se.is_empty() {
            self.log(&se);
        }
        Ok(so)
    }

    fn with_busy(&self, work: impl FnOnce(Backend) + Send + 'static) {
        if self.shared.busy.swap(true, Ordering::SeqCst) {
            self.log("busy — wait for the current step to finish");
            return;
        }
        let me = self.clone();
        thread::spawn(move || {
            work(me.clone());
            me.shared.busy.store(false, Ordering::SeqCst);
            me.ctx.request_repaint();
        });
    }

    /// Step 1: pair with the IP:port + code from the pairing dialog.
    pub fn pair(&self, addr: String, code: String) {
        self.with_busy(move |b| {
            b.log(format!("pairing with {addr} ..."));
            match b.adb(&["pair", &addr, &code]) {
                Ok(o) if o.contains("Successfully paired") => {
                    b.shared.paired.store(true, Ordering::SeqCst);
                    b.log("paired ✓");
                }
                Ok(_) => b.log("pair failed — check the code hasn't expired, then retry"),
                Err(e) => b.log(format!("pair error: {e}")),
            }
        });
    }

    /// Step 2: scan the device for the open connect port, then adb connect to it.
    pub fn find_and_connect(&self, ip: String) {
        self.with_busy(move |b| {
            b.scan_and_connect(&ip);
        });
    }

    /// Saved-device path: connect straight to a known `ip:port` and mirror,
    /// skipping the scan. If the pinned port is dead (e.g. after a reboot),
    /// fall back to scanning the same IP, then mirror whatever connected.
    pub fn quick_connect(&self, addr: String) {
        self.with_busy(move |b| {
            b.log(format!("quick connect {addr} ..."));
            // ponytail: a wrong-but-open port can make `adb connect` hang on TLS;
            // acceptable since saved addrs point at a real, pinned adb port.
            let connected = b.connect_target(&addr) || {
                b.log("pinned port not responding — falling back to scan");
                b.scan_and_connect(host_of(&addr))
            };
            if connected {
                b.shared.paired.store(true, Ordering::SeqCst);
                let target = b.shared.mirror_target.lock().unwrap().clone();
                b.spawn_scrcpy(target.as_deref());
            } else {
                b.log("quick connect failed — open Wireless debugging on the phone and retry");
            }
        });
    }

    /// Scan `ip` for the open connect port and adb-connect to the first that
    /// accepts. Runs on the caller's thread; returns true on success.
    fn scan_and_connect(&self, ip: &str) -> bool {
        self.log(format!("scanning {ip}:{SCAN_LO}-{SCAN_HI} for the connect port ..."));
        let ports = scan_ports(ip);
        if ports.is_empty() {
            self.log("no open ports found — is Wireless debugging still on?");
            return false;
        }
        self.log(format!("open: {ports:?} — trying adb connect"));
        for p in ports {
            if self.connect_target(&format!("{ip}:{p}")) {
                return true;
            }
        }
        self.log("found open ports but none accepted an adb connection");
        false
    }

    /// Step 3: launch scrcpy against the connected device (detached).
    pub fn start_mirror(&self) {
        let target = self.shared.mirror_target.lock().unwrap().clone();
        self.with_busy(move |b| b.spawn_scrcpy(target.as_deref()));
    }

    /// `adb connect <target>`, then confirm it shows as an authorized device.
    /// On success records it as the mirror target. Runs on the caller's thread.
    fn connect_target(&self, target: &str) -> bool {
        let _ = self.adb(&["connect", target]);
        let devs = self.adb(&["devices"]).unwrap_or_default();
        let ok = devs
            .lines()
            .any(|l| l.starts_with(target) && l.trim_end().ends_with("device"));
        if ok {
            *self.shared.mirror_target.lock().unwrap() = Some(target.to_string());
            self.shared.connected.store(true, Ordering::SeqCst);
            self.log(format!("connected ✓  ({target})"));
        }
        ok
    }

    fn spawn_scrcpy(&self, target: Option<&str>) {
        let mut cmd = Command::new(self.tool("scrcpy"));
        if let Some(t) = target {
            cmd.args(["-s", t]);
        }
        match cmd.spawn() {
            Ok(_) => self.log("scrcpy launched — mirror window should open ▶"),
            Err(e) => self.log(format!("could not start scrcpy: {e}")),
        }
    }
}

/// "192.168.1.136:5555" -> "192.168.1.136"
fn host_of(addr: &str) -> &str {
    addr.rsplit_once(':').map(|(h, _)| h).unwrap_or(addr)
}

/// Parallel TCP scan; returns open ports, sorted.
fn scan_ports(ip: &str) -> Vec<u16> {
    let next = Arc::new(AtomicU32::new(SCAN_LO));
    let found = Arc::new(Mutex::new(Vec::<u16>::new()));
    let mut handles = Vec::new();
    for _ in 0..SCAN_WORKERS {
        let next = next.clone();
        let found = found.clone();
        let ip = ip.to_string();
        handles.push(thread::spawn(move || loop {
            let port = next.fetch_add(1, Ordering::Relaxed);
            if port > SCAN_HI {
                break;
            }
            let Ok(addr) = format!("{ip}:{port}").parse::<SocketAddr>() else {
                break;
            };
            if TcpStream::connect_timeout(&addr, Duration::from_millis(800)).is_ok() {
                found.lock().unwrap().push(port as u16);
            }
        }));
    }
    for h in handles {
        let _ = h.join();
    }
    let mut v = Arc::try_unwrap(found).unwrap().into_inner().unwrap();
    v.sort_unstable();
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_finds_a_listening_port() {
        // A local listener must be discoverable within the scan range.
        use std::net::TcpListener;
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        // Only ports in [SCAN_LO, SCAN_HI] are scanned; skip if OS picked outside.
        if (port as u32) < SCAN_LO || (port as u32) > SCAN_HI {
            return;
        }
        let open = scan_ports("127.0.0.1");
        assert!(open.contains(&port), "expected {port} in {open:?}");
    }
}
