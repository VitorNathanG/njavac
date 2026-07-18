//! Internal allocation-instrumented helper for the unified benchmark report.

use std::alloc::{GlobalAlloc, Layout, System};
use std::hint::black_box;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use njavac::{CompileObserver, CompilePhase};

struct CountingAllocator;

static ACTIVE: AtomicBool = AtomicBool::new(false);
static CALLS: AtomicU64 = AtomicU64::new(0);
static ALLOCATED: AtomicU64 = AtomicU64::new(0);
static DEALLOCATED: AtomicU64 = AtomicU64::new(0);
static LIVE: AtomicU64 = AtomicU64::new(0);
static PEAK_LIVE: AtomicU64 = AtomicU64::new(0);

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() && ACTIVE.load(Ordering::Relaxed) {
            record_allocation(layout.size() as u64);
        }
        ptr
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc_zeroed(layout) };
        if !ptr.is_null() && ACTIVE.load(Ordering::Relaxed) {
            record_allocation(layout.size() as u64);
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if ACTIVE.load(Ordering::Relaxed) {
            DEALLOCATED.fetch_add(layout.size() as u64, Ordering::Relaxed);
            subtract_live(layout.size() as u64);
        }
        unsafe { System.dealloc(ptr, layout) };
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let new_ptr = unsafe { System.realloc(ptr, layout, new_size) };
        if !new_ptr.is_null() && ACTIVE.load(Ordering::Relaxed) {
            DEALLOCATED.fetch_add(layout.size() as u64, Ordering::Relaxed);
            subtract_live(layout.size() as u64);
            record_allocation(new_size as u64);
        }
        new_ptr
    }
}

fn record_allocation(bytes: u64) {
    CALLS.fetch_add(1, Ordering::Relaxed);
    ALLOCATED.fetch_add(bytes, Ordering::Relaxed);
    let live = LIVE.fetch_add(bytes, Ordering::Relaxed).saturating_add(bytes);
    let mut peak = PEAK_LIVE.load(Ordering::Relaxed);
    while live > peak {
        match PEAK_LIVE.compare_exchange_weak(peak, live, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => break,
            Err(current) => peak = current,
        }
    }
}

fn subtract_live(bytes: u64) {
    let _ = LIVE.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |live| {
        Some(live.saturating_sub(bytes))
    });
}

#[derive(Clone, Copy, Default)]
struct Snapshot {
    calls: u64,
    bytes: u64,
    deallocated_bytes: u64,
}

fn snapshot() -> Snapshot {
    Snapshot {
        calls: CALLS.load(Ordering::Relaxed),
        bytes: ALLOCATED.load(Ordering::Relaxed),
        deallocated_bytes: DEALLOCATED.load(Ordering::Relaxed),
    }
}

struct AllocationObserver {
    starts: [Snapshot; 7],
    calls: [u64; 7],
    bytes: [u64; 7],
    deallocated_bytes: [u64; 7],
}

impl AllocationObserver {
    fn new() -> Self {
        Self {
            starts: [Snapshot::default(); 7],
            calls: [0; 7],
            bytes: [0; 7],
            deallocated_bytes: [0; 7],
        }
    }
}

impl CompileObserver for AllocationObserver {
    fn phase_started(&mut self, phase: CompilePhase) {
        self.starts[phase_index(phase)] = snapshot();
    }

    fn phase_finished(&mut self, phase: CompilePhase) {
        let index = phase_index(phase);
        let end = snapshot();
        self.calls[index] += end.calls.saturating_sub(self.starts[index].calls);
        self.bytes[index] += end.bytes.saturating_sub(self.starts[index].bytes);
        self.deallocated_bytes[index] += end
            .deallocated_bytes
            .saturating_sub(self.starts[index].deallocated_bytes);
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let rounds: usize = args
        .next()
        .and_then(|value| value.parse().ok())
        .filter(|&value| value > 0)
        .unwrap_or_else(|| usage("rounds must be a positive integer"));
    let paths: Vec<PathBuf> = args.map(PathBuf::from).collect();
    if paths.is_empty() {
        usage("at least one fixture path is required");
    }

    let fixtures: Vec<(String, String, String)> = paths
        .iter()
        .map(|path| {
            let source = std::fs::read_to_string(path)
                .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));
            let source_file = path.file_name().unwrap().to_string_lossy().into_owned();
            (source, source_file, path.display().to_string())
        })
        .collect();

    CALLS.store(0, Ordering::Relaxed);
    ALLOCATED.store(0, Ordering::Relaxed);
    DEALLOCATED.store(0, Ordering::Relaxed);
    LIVE.store(0, Ordering::Relaxed);
    PEAK_LIVE.store(0, Ordering::Relaxed);
    ACTIVE.store(true, Ordering::Relaxed);

    let mut observer = AllocationObserver::new();
    for _ in 0..rounds {
        for (source, source_file, path) in &fixtures {
            let bytes = njavac::compile_observed(source, source_file, &mut observer)
                .unwrap_or_else(|diagnostic| panic!("{}", diagnostic.render(path, source)));
            observer.phase_started(CompilePhase::ResultDrop);
            black_box(bytes);
            observer.phase_finished(CompilePhase::ResultDrop);
        }
    }

    ACTIVE.store(false, Ordering::Relaxed);
    for index in 0..7 {
        println!(
            "phase\t{index}\t{}\t{}\t{}",
            observer.calls[index], observer.bytes[index], observer.deallocated_bytes[index],
        );
    }
    println!("peak\t{}", PEAK_LIVE.load(Ordering::Relaxed));
    println!("live\t{}", LIVE.load(Ordering::Relaxed));
}

fn phase_index(phase: CompilePhase) -> usize {
    match phase {
        CompilePhase::Lex => 0,
        CompilePhase::Parse => 1,
        CompilePhase::Sema => 2,
        CompilePhase::CodegenPlan => 3,
        CompilePhase::ClassfileEmit => 4,
        CompilePhase::Cleanup => 5,
        CompilePhase::ResultDrop => 6,
    }
}

fn usage(message: &str) -> ! {
    eprintln!("benchmark-alloc: {message}");
    eprintln!("usage: benchmark-alloc <rounds> <fixture.java>...");
    std::process::exit(2);
}
