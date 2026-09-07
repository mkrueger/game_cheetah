//! Read-only diagnostic for the production background scanner.
//! Arguments: PID HEX_TARGET [MAX_DEPTH]. Nothing is saved or written.
use game_cheetah::{
    SearchResult, SearchType,
    pointer_scan::{ProcessIdentity, ScanJob, ScanOptions},
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let pid = args.next().ok_or("PID required")?.parse()?;
    let target = game_cheetah::address::parse_hex(&args.next().ok_or("Hex target required")?)?;
    let depth = args.next().map(|text| text.parse()).transpose()?.unwrap_or(4);
    let identity = ProcessIdentity::capture(pid)?;
    let options = ScanOptions { depth, ..Default::default() };
    let job = ScanJob::start(identity, SearchResult::new(target, SearchType::Int), options, None)?;
    let progress = job.progress.clone();
    let report = job.wait()?;
    println!(
        "Bytes scanned: {}, indexed pointers: {}",
        progress.bytes.load(std::sync::atomic::Ordering::Relaxed),
        progress.pointers.load(std::sync::atomic::Ordering::Relaxed)
    );
    println!(
        "Candidates: {}, limited: {}, failed reads: {}",
        report.candidates.len(),
        report.limited,
        report.failed_reads
    );
    for spec in report.candidates {
        println!("{}", spec.label());
    }
    Ok(())
}
