//! Internal allocation-instrumented helper for the unified benchmark report.

#[allow(dead_code)]
#[path = "benchmark/phase.rs"]
mod phase;

use std::alloc::{GlobalAlloc, Layout, System};
use std::hint::black_box;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use njavac_compiler::{CompileObserver, CompilePhase};

use phase::{PhaseName, PhaseValues, SequenceValidator};

struct CountingAllocator;

static CALLS: AtomicU64 = AtomicU64::new(0);
static REQUESTED: AtomicU64 = AtomicU64::new(0);
static RELEASED: AtomicU64 = AtomicU64::new(0);
static LIVE: AtomicU64 = AtomicU64::new(0);
static PEAK_LIVE: AtomicU64 = AtomicU64::new(0);
static ACCOUNTING_ERROR: AtomicBool = AtomicBool::new(false);

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc(layout) };
        if !pointer.is_null() {
            record_allocation(layout.size() as u64);
        }
        pointer
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc_zeroed(layout) };
        if !pointer.is_null() {
            record_allocation(layout.size() as u64);
        }
        pointer
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        record_deallocation(layout.size() as u64);
        unsafe { System.dealloc(pointer, layout) };
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let new_pointer = unsafe { System.realloc(pointer, layout, new_size) };
        if !new_pointer.is_null() {
            // A successful realloc is one requested allocation of the new size
            // and one release of the old layout. A failed realloc changes no
            // counters because the original allocation remains valid.
            record_deallocation(layout.size() as u64);
            record_allocation(new_size as u64);
        }
        new_pointer
    }
}

fn record_allocation(bytes: u64) {
    CALLS.fetch_add(1, Ordering::Relaxed);
    REQUESTED.fetch_add(bytes, Ordering::Relaxed);
    let live = LIVE.fetch_add(bytes, Ordering::Relaxed).wrapping_add(bytes);
    let mut peak = PEAK_LIVE.load(Ordering::Relaxed);
    while live > peak {
        match PEAK_LIVE.compare_exchange_weak(peak, live, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => break,
            Err(current) => peak = current,
        }
    }
}

fn record_deallocation(bytes: u64) {
    RELEASED.fetch_add(bytes, Ordering::Relaxed);
    if checked_live_after_deallocation(LIVE.load(Ordering::Relaxed), bytes).is_none() {
        ACCOUNTING_ERROR.store(true, Ordering::Relaxed);
        return;
    }
    if LIVE
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |live| {
            live.checked_sub(bytes)
        })
        .is_err()
    {
        ACCOUNTING_ERROR.store(true, Ordering::Relaxed);
    }
}

fn checked_live_after_deallocation(live: u64, bytes: u64) -> Option<u64> {
    live.checked_sub(bytes)
}

#[derive(Clone, Copy, Default)]
struct Snapshot {
    calls: u64,
    requested_bytes: u64,
    released_bytes: u64,
    live_bytes: u64,
}

fn snapshot() -> Snapshot {
    Snapshot {
        calls: CALLS.load(Ordering::Relaxed),
        requested_bytes: REQUESTED.load(Ordering::Relaxed),
        released_bytes: RELEASED.load(Ordering::Relaxed),
        live_bytes: LIVE.load(Ordering::Relaxed),
    }
}

#[derive(Clone, Copy, Default)]
struct AllocationMetric {
    calls: u64,
    requested_bytes: u64,
    released_bytes: u64,
}

struct AllocationObserver {
    starts: PhaseValues<Snapshot>,
    values: PhaseValues<AllocationMetric>,
    sequence: SequenceValidator,
    error: Option<&'static str>,
}

impl AllocationObserver {
    fn new() -> Self {
        Self {
            starts: PhaseValues::default(),
            values: PhaseValues::default(),
            sequence: SequenceValidator::default(),
            error: None,
        }
    }

    fn complete_compile(&mut self) -> Result<(), String> {
        if let Some(error) = self.error.take() {
            return Err(error.to_string());
        }
        self.sequence
            .complete_success()
            .map_err(|error| error.to_string())
    }

    fn drop_result(&mut self, bytes: Vec<u8>) {
        black_box(&bytes);
        let start = snapshot();
        drop(bytes);
        self.add_delta(PhaseName::ResultBytesDrop, start, snapshot());
    }

    fn add_delta(&mut self, phase: PhaseName, start: Snapshot, end: Snapshot) {
        let Some(calls) = end.calls.checked_sub(start.calls) else {
            self.error = Some("allocation call counter moved backwards");
            return;
        };
        let Some(requested_bytes) = end.requested_bytes.checked_sub(start.requested_bytes) else {
            self.error = Some("requested byte counter moved backwards");
            return;
        };
        let Some(released_bytes) = end.released_bytes.checked_sub(start.released_bytes) else {
            self.error = Some("released byte counter moved backwards");
            return;
        };
        let value = self.values.get_mut(phase);
        value.calls = value.calls.saturating_add(calls);
        value.requested_bytes = value.requested_bytes.saturating_add(requested_bytes);
        value.released_bytes = value.released_bytes.saturating_add(released_bytes);
    }
}

impl CompileObserver for AllocationObserver {
    fn phase_started(&mut self, phase: CompilePhase) {
        self.sequence.started(phase);
        *self.starts.get_mut(PhaseName::from(phase)) = snapshot();
    }

    fn phase_finished(&mut self, phase: CompilePhase) {
        self.sequence.finished(phase);
        let phase = PhaseName::from(phase);
        self.add_delta(phase, *self.starts.get(phase), snapshot());
    }
}

struct Fixture {
    source: String,
    source_file: String,
    display_path: String,
}

fn main() {
    if std::env::args().nth(1).as_deref() == Some("--selftest-accounting") {
        if let Err(error) = accounting_selftest() {
            eprintln!("benchmark-alloc: {error}");
            std::process::exit(1);
        }
        return;
    }
    if let Err(error) = run() {
        eprintln!("benchmark-alloc: {error}");
        std::process::exit(1);
    }
}

fn accounting_selftest() -> Result<(), String> {
    let baseline = snapshot();
    PEAK_LIVE.store(baseline.live_bytes, Ordering::Relaxed);

    let mut allocation = Vec::with_capacity(32);
    allocation.resize(32, 0_u8);
    let allocated = snapshot();
    if allocated.live_bytes < baseline.live_bytes + 32 {
        return Err("tracked allocation did not increase live bytes".to_string());
    }

    let before_growth = snapshot();
    allocation.reserve_exact(256);
    let after_growth = snapshot();
    if after_growth.calls <= before_growth.calls
        || after_growth.requested_bytes <= before_growth.requested_bytes
        || after_growth.released_bytes <= before_growth.released_bytes
    {
        return Err(
            "successful realloc growth was not counted as release plus request".to_string(),
        );
    }

    let retained = snapshot();
    if retained.live_bytes == baseline.live_bytes {
        return Err("retained allocation was not visible above the baseline".to_string());
    }
    allocation.shrink_to_fit();
    drop(allocation);
    let final_snapshot = snapshot();
    if final_snapshot.live_bytes != baseline.live_bytes {
        return Err(format!(
            "selftest ended with {} live bytes, baseline was {}",
            final_snapshot.live_bytes, baseline.live_bytes,
        ));
    }
    if checked_live_after_deallocation(1, 2).is_some() {
        return Err("accounting underflow was not rejected".to_string());
    }
    if ACCOUNTING_ERROR.load(Ordering::Relaxed) {
        return Err("global allocator reported an accounting error".to_string());
    }
    println!("allocation accounting selftest passed");
    Ok(())
}

fn run() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let first = args.next().ok_or("missing allocation rounds")?;
    if first == "--verify-only" {
        let paths: Vec<PathBuf> = args.map(PathBuf::from).collect();
        if paths.is_empty() {
            return Err("at least one fixture path is required".to_string());
        }
        let fixtures = load_fixtures(&paths)?;
        allocation_preflight(&fixtures)?;
        println!(
            "allocation instrumentation verified for {} fixtures",
            fixtures.len()
        );
        return Ok(());
    }
    let rounds: usize = first
        .parse()
        .ok()
        .filter(|&value| value > 0)
        .ok_or("rounds must be a positive integer")?;
    let paths: Vec<PathBuf> = args.map(PathBuf::from).collect();
    if paths.is_empty() {
        return Err("at least one fixture path is required".to_string());
    }
    let fixtures = load_fixtures(&paths)?;

    let mut observer = AllocationObserver::new();
    let baseline = snapshot();
    PEAK_LIVE.store(baseline.live_bytes, Ordering::Relaxed);

    for _ in 0..rounds {
        for fixture in &fixtures {
            let bytes = njavac_compiler::compile_observed(
                &fixture.source,
                &fixture.source_file,
                &mut observer,
            )
            .map_err(|diagnostic| diagnostic.render(&fixture.display_path, &fixture.source))?;
            observer.complete_compile()?;
            observer.drop_result(bytes);
        }
    }

    let final_snapshot = snapshot();
    if ACCOUNTING_ERROR.load(Ordering::Relaxed) {
        return Err("allocator live-byte accounting underflowed".to_string());
    }
    if final_snapshot.live_bytes != baseline.live_bytes {
        return Err(format!(
            "tracked live bytes ended at {}, baseline was {}",
            final_snapshot.live_bytes, baseline.live_bytes,
        ));
    }
    let peak_live_growth = PEAK_LIVE
        .load(Ordering::Relaxed)
        .checked_sub(baseline.live_bytes)
        .ok_or("peak live bytes fell below baseline")?;
    let total_requested = final_snapshot
        .requested_bytes
        .checked_sub(baseline.requested_bytes)
        .ok_or("requested byte counter moved backwards")?;
    let total_released = final_snapshot
        .released_bytes
        .checked_sub(baseline.released_bytes)
        .ok_or("released byte counter moved backwards")?;

    for phase in PhaseName::ALL {
        let metric = observer.values.get(phase);
        println!(
            "phase\t{}\t{}\t{}\t{}",
            phase.as_str(),
            metric.calls,
            metric.requested_bytes,
            metric.released_bytes,
        );
    }
    println!("baseline_live\t{}", baseline.live_bytes);
    println!("peak_live_growth\t{peak_live_growth}");
    println!("final_live\t{}", final_snapshot.live_bytes);
    println!("total\t{total_requested}\t{total_released}");
    Ok(())
}

fn load_fixtures(paths: &[PathBuf]) -> Result<Vec<Fixture>, String> {
    paths
        .iter()
        .map(|path| {
            let source = std::fs::read_to_string(path)
                .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
            let source_file = path
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or_else(|| format!("{} has no UTF-8 filename", path.display()))?
                .to_string();
            Ok(Fixture {
                source,
                source_file,
                display_path: path.display().to_string(),
            })
        })
        .collect()
}

fn allocation_preflight(fixtures: &[Fixture]) -> Result<(), String> {
    for fixture in fixtures {
        let ordinary = njavac::compile(&fixture.source, &fixture.source_file)
            .map_err(|diagnostic| diagnostic.render(&fixture.display_path, &fixture.source))?;
        let mut observer = AllocationObserver::new();
        let observed =
            njavac_compiler::compile_observed(&fixture.source, &fixture.source_file, &mut observer)
                .map_err(|diagnostic| diagnostic.render(&fixture.display_path, &fixture.source))?;
        observer.complete_compile()?;
        if observed != ordinary {
            return Err(format!(
                "allocation-observed output differs from ordinary output for {}",
                fixture.display_path,
            ));
        }
        observer.drop_result(observed);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::checked_live_after_deallocation;

    #[test]
    fn live_accounting_covers_baselines_realloc_and_underflow() {
        let baseline = 100;
        let after_preexisting_release = checked_live_after_deallocation(baseline, 40).unwrap();
        let after_measured_allocation = after_preexisting_release + 40;
        assert_eq!(after_measured_allocation, baseline);

        let grown = checked_live_after_deallocation(130, 30).unwrap() + 60;
        assert_eq!(grown, 160);
        let shrunk = checked_live_after_deallocation(grown, 60).unwrap() + 10;
        assert_eq!(shrunk, 110);
        assert_eq!(checked_live_after_deallocation(5, 6), None);
        assert_ne!(baseline + 1, baseline);
    }
}
