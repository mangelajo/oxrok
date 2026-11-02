//! Attaching a USB/IP socket to the Linux `vhci-hcd` virtual host
//! controller through its sysfs files, as the `usbip attach` tool
//! does: a free port is picked from `status`, and `port sockfd devid
//! speed` is written to `attach`; `detach` takes the port back. Needs
//! root, and the module (`modprobe vhci-hcd`).

use std::fs;
use std::os::fd::RawFd;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::model::Speed;
use crate::{Error, Result};

/// The controller's sysfs directory.
pub const SYSFS: &str = "/sys/devices/platform/vhci_hcd.0";

/// Whether the module is loaded.
pub fn available() -> bool {
    Path::new(SYSFS).join("attach").exists()
}

/// One line of `status`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PortStatus {
    /// `hs` or `ss`.
    pub hub: String,
    /// The port number, what `attach` and `detach` take.
    pub port: u8,
    /// `004` is free (`VDEV_ST_NULL`), `006` in use.
    pub status: u16,
    /// The bus id of the attached device, `0-0` when none.
    pub busid: String,
}

/// Parses the `status` file(s).
pub fn parse_status(text: &str) -> Vec<PortStatus> {
    text.lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split_whitespace().collect();
            if f.len() < 7 || f[0] == "hub" {
                return None;
            }
            Some(PortStatus {
                hub: f[0].to_owned(),
                port: f[1].parse().ok()?,
                status: f[2].parse().ok()?,
                busid: f[6].to_owned(),
            })
        })
        .collect()
}

/// All ports: `status` and, with several controllers, `status.N`.
pub fn ports() -> Result<Vec<PortStatus>> {
    let dir = Path::new(SYSFS);
    let mut text = fs::read_to_string(dir.join("status"))
        .map_err(|e| Error(format!("{SYSFS}/status: {e} (is vhci-hcd loaded?)")))?;
    for n in 1.. {
        match fs::read_to_string(dir.join(format!("status.{n}"))) {
            Ok(t) => text.push_str(&t),
            Err(_) => break,
        }
    }
    Ok(parse_status(&text))
}

/// A free port of the hub that takes `speed`.
pub fn free_port(speed: Speed) -> Result<u8> {
    let hub = match speed {
        Speed::Low | Speed::Full | Speed::High => "hs",
    };
    ports()?
        .iter()
        .find(|p| p.hub == hub && p.status == 4)
        .map(|p| p.port)
        .ok_or_else(|| Error(format!("no free {hub} port on {SYSFS}")))
}

/// Hands the connected socket to the controller.
pub fn attach(port: u8, sockfd: RawFd, devid: u32, speed: Speed) -> Result<()> {
    let line = format!("{port} {sockfd} {devid} {}", speed as u32);
    fs::write(Path::new(SYSFS).join("attach"), &line).map_err(|e| {
        Error(format!("attach '{line}': {e} (root needed; dmesg has the kernel's reason)"))
    })
}

/// Takes the port back; the device disappears from the bus.
pub fn detach(port: u8) -> Result<()> {
    fs::write(Path::new(SYSFS).join("detach"), format!("{port}"))
        .map_err(|e| Error(format!("detach port {port}: {e}")))
}

/// The bus id the kernel gives the device on `port`; it appears in
/// `status` once the device has enumerated, so this waits up to
/// `timeout`.
pub fn busid(port: u8, timeout: Duration) -> Result<String> {
    let start = Instant::now();
    loop {
        if let Some(p) = ports()?.iter().find(|p| p.port == port && p.busid != "0-0") {
            return Ok(p.busid.clone());
        }
        if start.elapsed() > timeout {
            return Err(Error(format!("port {port} has no enumerated device after {timeout:?}")));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Waits until the device at `busid` is configured; returns its sysfs
/// directory.
pub fn wait_configured(busid: &str, timeout: Duration) -> Result<PathBuf> {
    let dir = PathBuf::from("/sys/bus/usb/devices").join(busid);
    let start = Instant::now();
    loop {
        if let Ok(v) = fs::read_to_string(dir.join("bConfigurationValue"))
            && !v.trim().is_empty()
        {
            return Ok(dir);
        }
        if start.elapsed() > timeout {
            return Err(Error(format!("{} not configured after {timeout:?}", dir.display())));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// The device nodes the kernel created for the device: `/dev/hidrawN`,
/// `/dev/ttyACMN`, `/dev/usbtmcN`. Waits up to `timeout` for the first.
pub fn device_nodes(dir: &Path, timeout: Duration) -> Vec<PathBuf> {
    let start = Instant::now();
    loop {
        let mut nodes = Vec::new();
        collect_nodes(dir, 0, &mut nodes);
        nodes.sort();
        if !nodes.is_empty() || start.elapsed() > timeout {
            return nodes;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn collect_nodes(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() || entry.file_type().is_ok_and(|t| t.is_symlink()) {
            continue;
        }
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if matches!(name.as_ref(), "hidraw" | "tty" | "usbmisc") {
            if let Ok(children) = fs::read_dir(&path) {
                for c in children.flatten() {
                    let child = c.file_name();
                    // usbhid's hiddev nodes live under /dev/usb.
                    let dev = if child.to_string_lossy().starts_with("hiddev") {
                        PathBuf::from("/dev/usb").join(&child)
                    } else {
                        PathBuf::from("/dev").join(&child)
                    };
                    if dev.exists() {
                        out.push(dev);
                    }
                }
            }
        } else if depth < 4 {
            collect_nodes(&path, depth + 1, out);
        }
    }
}

/// Makes the nodes readable and writable by everyone, so the tools
/// under test run without root.
pub fn open_nodes(nodes: &[PathBuf]) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    for n in nodes {
        fs::set_permissions(n, fs::Permissions::from_mode(0o666))
            .map_err(|e| Error(format!("chmod {}: {e}", n.display())))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_status_table() {
        let text = "hub port sta spd dev      sockfd local_busid\n\
                    hs  0000 006 002 00010002 000004 1-1\n\
                    hs  0001 004 000 00000000 000000 0-0\n\
                    ss  0008 004 000 00000000 000000 0-0\n";
        let p = parse_status(text);
        assert_eq!(p.len(), 3);
        assert_eq!(p[0], PortStatus { hub: "hs".into(), port: 0, status: 6, busid: "1-1".into() });
        assert_eq!(p[1].status, 4);
        assert_eq!(p[2].hub, "ss");
    }
}
