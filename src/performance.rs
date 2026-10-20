//! Resource budgeting for useful throughput. No memory is allocated to manufacture load.
const MIB: u64 = 1024 * 1024;

#[derive(Clone, Copy, Debug)]
pub(crate) struct Resources {
    pub cores: usize,
    pub available_memory: u64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Schedule {
    pub workers: usize,
    pub threads_per_worker: usize,
}

pub(crate) fn resources() -> Resources {
    let cores = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(2);
    Resources {
        cores,
        available_memory: available_memory().unwrap_or(2 * 1024 * MIB),
    }
}

pub(crate) fn schedule(
    resources: Resources,
    queued: usize,
    requested_workers: usize,
    video: bool,
    hardware: bool,
    maximum_pixels: u64,
) -> Schedule {
    if queued == 0 {
        return Schedule {
            workers: 0,
            threads_per_worker: 1,
        };
    }
    let cores = resources.cores.max(1);
    // Frame buffers, lookahead and codec state grow with picture size. Reserve
    // another 512 MiB for the UI/OS and use at most three quarters of reclaimable RAM.
    // Budget for software buffers even when selecting hardware, because an
    // unsupported input can fall back to software after workers have started.
    let per_worker = if video {
        256 * MIB + maximum_pixels.saturating_mul(48)
    } else {
        96 * MIB
    };
    let usable = resources.available_memory.saturating_sub(512 * MIB) * 3 / 4;
    let memory_workers = (usable / per_worker).max(1).min(usize::MAX as u64) as usize;
    let automatic = if video {
        cores.div_ceil(if hardware { 2 } else { 4 })
    } else {
        cores
    };
    let desired = if requested_workers == 0 {
        automatic
    } else {
        requested_workers
    };
    let workers = desired.max(1).min(queued).min(memory_workers);
    Schedule {
        workers,
        threads_per_worker: (cores / workers).max(1),
    }
}

#[cfg(target_os = "macos")]
fn available_memory() -> Option<u64> {
    let output = std::process::Command::new("/usr/bin/vm_stat")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let statistics = String::from_utf8(output.stdout).ok()?;
    let page_size = statistics
        .lines()
        .next()?
        .split("page size of ")
        .nth(1)?
        .split_whitespace()
        .next()?
        .parse::<u64>()
        .ok()?;
    let pages = statistics
        .lines()
        .filter_map(|line| {
            let (name, value) = line.split_once(':')?;
            // Inactive pages can be reclaimed. Purgeable is a subset and must not
            // be counted again. Compressed memory and swap are deliberately excluded.
            ["Pages free", "Pages inactive", "Pages speculative"]
                .contains(&name)
                .then(|| value.trim().trim_end_matches('.').parse::<u64>().ok())
                .flatten()
        })
        .sum::<u64>();
    Some(pages.saturating_mul(page_size))
}

#[cfg(target_os = "linux")]
fn available_memory() -> Option<u64> {
    let text = std::fs::read_to_string("/proc/meminfo").ok()?;
    text.lines()
        .find_map(|line| line.strip_prefix("MemAvailable:"))?
        .split_whitespace()
        .next()?
        .parse::<u64>()
        .ok()?
        .checked_mul(1024)
}

#[cfg(target_os = "windows")]
fn available_memory() -> Option<u64> {
    #[repr(C)]
    #[derive(Default)]
    struct MemoryStatusEx {
        length: u32,
        memory_load: u32,
        total_physical: u64,
        available_physical: u64,
        total_page_file: u64,
        available_page_file: u64,
        total_virtual: u64,
        available_virtual: u64,
        available_extended_virtual: u64,
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        #[link_name = "GlobalMemoryStatusEx"]
        fn global_memory_status_ex(status: *mut MemoryStatusEx) -> i32;
    }
    let mut status = MemoryStatusEx {
        length: std::mem::size_of::<MemoryStatusEx>() as u32,
        ..Default::default()
    };
    // SAFETY: The structure has the native MEMORYSTATUSEX layout, correct size,
    // and is exclusively borrowed for this synchronous Windows API call.
    (unsafe { global_memory_status_ex(&mut status) } != 0).then_some(status.available_physical)
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
fn available_memory() -> Option<u64> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn single_file_can_use_every_available_cpu_thread() {
        let result = schedule(
            Resources {
                cores: 16,
                available_memory: 16 * 1024 * MIB,
            },
            1,
            0,
            true,
            false,
            1920 * 1080,
        );
        assert_eq!(
            result,
            Schedule {
                workers: 1,
                threads_per_worker: 16
            }
        );
    }
    #[test]
    fn automatic_work_shares_cpu_without_spawning_empty_workers() {
        let resources = Resources {
            cores: 10,
            available_memory: 16 * 1024 * MIB,
        };
        let cpu = schedule(resources, 100, 0, true, false, 1920 * 1080);
        let hardware = schedule(resources, 100, 0, true, true, 1920 * 1080);
        assert_eq!(cpu.workers, 3);
        assert_eq!(hardware.workers, 5);
        assert!(cpu.workers * cpu.threads_per_worker <= resources.cores);
        assert_eq!(
            schedule(resources, 2, 10, true, false, 1920 * 1080).workers,
            2
        );
        assert_eq!(schedule(resources, 0, 0, true, false, 0).workers, 0);
    }
    #[test]
    fn scarce_memory_reduces_parallelism_but_never_divides_by_zero() {
        let low = Resources {
            cores: 0,
            available_memory: 0,
        };
        assert_eq!(
            schedule(low, 4, 0, true, false, 7680 * 4320),
            Schedule {
                workers: 1,
                threads_per_worker: 1
            }
        );
        let limited = Resources {
            cores: 32,
            available_memory: 2 * 1024 * MIB,
        };
        assert!(schedule(limited, 10, 10, true, false, 3840 * 2160).workers < 10);
        assert_eq!(
            schedule(limited, 10, 10, true, true, 3840 * 2160),
            schedule(limited, 10, 10, true, false, 3840 * 2160),
            "Hardware jobs must reserve enough memory for per-file CPU fallback"
        );
    }
}
