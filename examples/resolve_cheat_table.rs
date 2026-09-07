//! Read-only check of saved definitions using the same resolver as the UI.
use game_cheetah::{CheatTable, ModuleCatalog, load_cheat_table_with_process};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let pid: process_memory::Pid = args.next().ok_or("PID required")?.parse()?;
    let path = args.next().ok_or("Table path required")?;
    let modules = ModuleCatalog::for_process(pid)?;
    let table: CheatTable = toml::from_str(&std::fs::read_to_string(&path)?)?;
    let loaded = load_cheat_table_with_process(std::path::Path::new(&path), "", &modules, pid)?;
    for search in loaded {
        for result in search.collect_results().iter() {
            println!("LOADED {:?} 0x{:X}", result.search_type, result.addr);
        }
        for pending in &search.unresolved_addresses {
            println!("UNRESOLVED {}: {}", pending.address.label(), pending.reason);
        }
    }
    for search in table.searches {
        for entry in search.entries {
            let resolved = modules.trace_for_process(&entry.address, entry.search_type.fixed_byte_length().unwrap_or(1), pid)?;
            println!("{} => 0x{:X}", entry.address.label(), resolved.address);
            for step in resolved.steps {
                println!(
                    "  [0x{:X}] = 0x{:X}; offset {} => 0x{:X}",
                    step.location, step.pointer, step.offset, step.target
                );
            }
        }
    }
    Ok(())
}
