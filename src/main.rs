// SPDX-License-Identifier: GPL-3.0-or-later
use config::Config;
use libsystemd::daemon::{self, NotifyState};
use std::convert::TryInto;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::process::Command;
use std::thread::sleep;
use std::time::Duration;
use std::{fs, io};
use sysinfo::{RefreshKind, System, SystemExt};

// Hard coded paths:
const RUN_SYSD: &str = "/run/systemd";
const ETC_SYSD: &str = "/etc/systemd";
const VEN_SYSD: &str = "/usr/lib/systemd";
const DEF_CONFIG: &str = "/usr/share/systemd-swap/swap-default.conf";
const ETC_CONFIG: &str = "/etc/systemd/swap.conf";
const WORK_DIR: &str = "/run/systemd/swap";
const ZSWAP_M: &str = "/sys/module/zswap";
const ZSWAP_M_P: &str = "/sys/module/zswap/parameters";
const SWAPFC_PATH: &str = "/var/lib/systemd-swap/swapfc";
// Tmp config untill these are controlled by options
const SWAPFC_BUF_SIZE: usize = 8388608;
const SWAPFC_FREE_PERC: u8 = 15;
const SWAPFC_REMOVE_FREE_PERC: u8 = 55;
const SWAPFC_CHUNK_SIZE: usize = 268435456;
const SWAPFC_MAX_COUNT: u8 = 32;
const SWAPFC_MIN_COUNT: u8 = 0;
const ZSWAP_ENABLED: &str = "true";
const ZRAM_ENABLED: &str = "false";
const SWAPD_AUTO_SWAPON_ENABLED: &str = "true";
const SWAPFC_ENABLED: &str = "true";

fn load_config() {
    println!("Load config...");
}
fn config_is_true(config: &str) -> bool {
    if config == "true" {
        true
    } else if config == "yes" {
        true
    } else if config == "1" {
        true
    } else if config == "y" {
        true
    } else {
        false
    }
}
fn main() {
    load_config();
    daemon::notify(false, &[NotifyState::Ready]).expect("notify failed");
    if config_is_true(ZSWAP_ENABLED) {
        zswap();
    }
    if config_is_true(ZRAM_ENABLED) {
        zram();
    }
    if config_is_true(SWAPD_AUTO_SWAPON_ENABLED) {
        swapd_auto_swapon();
    }
    if config_is_true(SWAPFC_ENABLED) {
        swapfc();
    }
}
fn zswap() {
    println!("Zswap...");
}
fn zram() {
    println!("Zram...");
}
fn swapd_auto_swapon() {
    println!("swapd_auto_swapon...");
}
fn swapfc() {
    fs::create_dir_all(SWAPFC_PATH).expect("Unable to create swapfc_path");
    let mut allocated: u8 = 0;
    for _ in 0..SWAPFC_MIN_COUNT {
        create_swapfile(&mut allocated);
    }
    loop {
        sleep(Duration::from_secs(1));
        if allocated == 0 {
            let curr_free_ram_perc = get_free_ram_perc();
            if curr_free_ram_perc < SWAPFC_FREE_PERC {
                create_swapfile(&mut allocated);
            }
            continue;
        }
        let curr_free_swap_perc = get_free_swap_perc();
        if curr_free_swap_perc < SWAPFC_FREE_PERC && allocated < SWAPFC_MAX_COUNT {
            create_swapfile(&mut allocated);
        }
        if allocated <= 2 || allocated <= SWAPFC_MIN_COUNT {
            continue;
        }
        if curr_free_swap_perc < SWAPFC_REMOVE_FREE_PERC {
            destroy_swapfile(&mut allocated).expect("Unable to remove swap file");
        }
    }
}

fn get_free_ram_perc() -> u8 {
    let s = System::new_with_specifics(RefreshKind::new().with_memory());
    let total = s.get_total_memory();
    let free = s.get_free_memory();
    ((free * 100) / total).try_into().unwrap()
}

fn get_free_swap_perc() -> u8 {
    let s = System::new_with_specifics(RefreshKind::new().with_memory());
    let total = s.get_total_swap();
    let free = s.get_free_swap();
    ((free * 100) / total).try_into().unwrap()
}

fn create_swapfile(allocated: &mut u8) {
    daemon::notify(
        true,
        &[NotifyState::Status("Allocating swap file...".to_string())],
    )
    .expect("notify failed");
    *allocated += 1;
    allocate_chunk(*allocated).expect("Unable to allocate swap file");
    Command::new("/usr/bin/mkswap")
        .arg(Path::new(SWAPFC_PATH).join(allocated.to_string()))
        .output()
        .expect("Unable to mkswap");
    Command::new("/usr/bin/swapon")
        .arg(Path::new(SWAPFC_PATH).join(allocated.to_string()))
        .output()
        .expect("Unable to swapon");
    daemon::notify(
        true,
        &[NotifyState::Status(
            "Monitoring memory status...".to_string(),
        )],
    )
    .expect("notify failed");
}

fn allocate_chunk(file: u8) -> io::Result<()> {
    let file = Path::new(SWAPFC_PATH).join(file.to_string());
    // create swap file
    let mut dst = fs::OpenOptions::new()
        .create_new(true)
        .append(true)
        .mode(0o600)
        .open(&file)
        .unwrap();
    // create a 8MiB buffer of zeroes
    let buffer = vec![0; SWAPFC_BUF_SIZE];
    // write <SWAPFC_CHUNK_SIZE> to swap file
    for _ in (0..SWAPFC_CHUNK_SIZE).step_by(SWAPFC_BUF_SIZE) {
        // write 8MiB at a time
        dst.write_all(&buffer).expect("Unable to write to file");
    }
    let dst_len: usize = dst.metadata().unwrap().len().try_into().unwrap();
    if dst_len < SWAPFC_CHUNK_SIZE {
        dst.write_all(&buffer[..SWAPFC_CHUNK_SIZE - dst_len])
            .expect("Unable to write to file");
    }
    Ok(())
}

fn destroy_swapfile(allocated: &mut u8) -> io::Result<()> {
    daemon::notify(
        true,
        &[NotifyState::Status("Removing swap file...".to_string())],
    )
    .expect("notify failed");
    Command::new("/usr/bin/swapoff")
        .arg(Path::new(SWAPFC_PATH).join(allocated.to_string()))
        .output()
        .expect("Unable to swapoff");
    fs::remove_file(allocated.to_string()).expect("Unable to remove file");
    *allocated -= 1;
    daemon::notify(
        true,
        &[NotifyState::Status(
            "Monitoring memory status...".to_string(),
        )],
    )
    .expect("notify failed");
    Ok(())
}

/*
fn check_ENOSPC(x) {
}
*/
