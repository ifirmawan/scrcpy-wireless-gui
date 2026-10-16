//! Tiny persistence for saved devices: one `name<TAB>addr` line per device,
//! stored at ~/.config/scrcpy-wireless/devices.txt. No deps — a few lines of fs.

use std::fs;
use std::path::PathBuf;

#[derive(Clone)]
pub struct Device {
    pub name: String,
    pub addr: String, // "ip:port"
}

fn config_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("scrcpy-wireless").join("devices.txt"))
}

pub fn load() -> Vec<Device> {
    let Some(p) = config_path() else { return Vec::new() };
    let Ok(text) = fs::read_to_string(&p) else { return Vec::new() };
    text.lines()
        .filter_map(|line| {
            let (name, addr) = line.split_once('\t')?;
            let (name, addr) = (name.trim(), addr.trim());
            if name.is_empty() || addr.is_empty() {
                return None;
            }
            Some(Device { name: name.to_string(), addr: addr.to_string() })
        })
        .collect()
}

pub fn save(list: &[Device]) -> std::io::Result<()> {
    let Some(p) = config_path() else {
        return Err(std::io::Error::new(std::io::ErrorKind::NotFound, "no config dir"));
    };
    if let Some(dir) = p.parent() {
        fs::create_dir_all(dir)?;
    }
    let body: String = list
        .iter()
        .map(|d| format!("{}\t{}\n", d.name.replace('\t', " "), d.addr.trim()))
        .collect();
    fs::write(&p, body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_through_a_temp_config() {
        let dir = std::env::temp_dir().join(format!("scw-test-{}", std::process::id()));
        std::env::set_var("XDG_CONFIG_HOME", &dir);
        let list = vec![
            Device { name: "Phone A".into(), addr: "192.168.1.136:5555".into() },
            Device { name: "Tablet".into(), addr: "192.168.1.50:5555".into() },
        ];
        save(&list).unwrap();
        let back = load();
        assert_eq!(back.len(), 2);
        assert_eq!(back[0].name, "Phone A");
        assert_eq!(back[1].addr, "192.168.1.50:5555");
        let _ = fs::remove_dir_all(&dir);
    }
}
