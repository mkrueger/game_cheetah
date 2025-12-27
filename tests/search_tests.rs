use game_cheetah::{SearchType, search_memory};

#[test]
fn test_search_byte_values() {
    let memory = vec![0x00, 0x42, 0x00, 0x42, 0x00, 0x42, 0xFF];
    let search_value = vec![0x42];

    let results = search_memory(&memory, &search_value, SearchType::Byte, 0x1000);

    assert_eq!(results.len(), 3);
    assert_eq!(results[0].addr, 0x1001);
    assert_eq!(results[1].addr, 0x1003);
    assert_eq!(results[2].addr, 0x1005);
}

#[test]
fn test_search_short_aligned() {
    let memory = vec![
        0x12, 0x34, // 0x3412 at offset 0
        0x00, 0x00, // padding
        0x12, 0x34, // 0x3412 at offset 4
        0x00, 0x00,
    ];
    let search_value = 0x3412u16.to_le_bytes().to_vec();

    let results = search_memory(&memory, &search_value, SearchType::Short, 0x2000);

    assert!(results.iter().any(|r| r.addr == 0x2000));
    assert!(results.iter().any(|r| r.addr == 0x2004));
}

#[test]
fn test_search_short_unaligned() {
    let memory = vec![
        0x00, // padding
        0x12, 0x34, // 0x3412 at offset 1 (unaligned)
        0x00, // padding
        0x12, 0x34, // 0x3412 at offset 4 (unaligned)
    ];
    let search_value = 0x3412u16.to_le_bytes().to_vec();

    let results = search_memory(&memory, &search_value, SearchType::Short, 0x2000);

    assert!(results.iter().any(|r| r.addr == 0x2001));
    assert!(results.iter().any(|r| r.addr == 0x2004));
}

#[test]
fn test_search_int_values_aligned() {
    let memory = vec![
        0x78, 0x56, 0x34, 0x12, // 0x12345678 at offset 0
        0x00, 0x00, 0x00, 0x00, // padding
        0x78, 0x56, 0x34, 0x12, // 0x12345678 at offset 8
    ];
    let search_value = 0x12345678u32.to_le_bytes().to_vec();

    let results = search_memory(&memory, &search_value, SearchType::Int, 0x3000);

    assert_eq!(results.len(), 2);
    assert!(results.iter().any(|r| r.addr == 0x3000));
    assert!(results.iter().any(|r| r.addr == 0x3008));
}

#[test]
fn test_search_int64_values() {
    let memory = vec![
        0xEF, 0xCD, 0xAB, 0x89, 0x67, 0x45, 0x23, 0x01, // First int64
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // Zero padding
        0xEF, 0xCD, 0xAB, 0x89, 0x67, 0x45, 0x23, 0x01, // Second int64
    ];
    let search_value = 0x0123456789ABCDEFu64.to_le_bytes().to_vec();

    let results = search_memory(&memory, &search_value, SearchType::Int64, 0x4000);

    assert_eq!(results.len(), 2);
    assert_eq!(results[0].addr, 0x4000);
    assert_eq!(results[1].addr, 0x4010);
}

#[test]
fn test_search_float_values() {
    let pi: f32 = std::f32::consts::PI;
    let pi5: f32 = std::f32::consts::PI * 5.0;

    let mut memory = vec![0x00; 20];
    memory[0..4].copy_from_slice(&pi.to_le_bytes());
    memory[8..12].copy_from_slice(&pi.to_le_bytes());
    memory[16..20].copy_from_slice(&pi5.to_le_bytes());

    let search_value = pi.to_le_bytes().to_vec();

    let results = search_memory(&memory, &search_value, SearchType::Float, 0x5000);

    assert_eq!(results.len(), 2);
    assert_eq!(results[0].addr, 0x5000);
    assert_eq!(results[1].addr, 0x5008);
}

#[test]
fn test_search_double_values() {
    let pi: f64 = std::f64::consts::PI;
    let pi5: f64 = std::f64::consts::PI * 5.0;

    let mut memory = vec![0x00; 32];
    memory[0..8].copy_from_slice(&pi.to_le_bytes());
    memory[16..24].copy_from_slice(&pi.to_le_bytes());
    memory[24..32].copy_from_slice(&pi5.to_le_bytes());

    let search_value = pi.to_le_bytes().to_vec();

    let results = search_memory(&memory, &search_value, SearchType::Double, 0x6000);

    assert_eq!(results.len(), 2);
    assert_eq!(results[0].addr, 0x6000);
    assert_eq!(results[1].addr, 0x6010);
}

#[test]
fn test_search_empty_memory() {
    let memory = vec![];
    let search_value = vec![0x42];

    let results = search_memory(&memory, &search_value, SearchType::Byte, 0x9000);

    assert_eq!(results.len(), 0);
}

#[test]
fn test_search_no_matches() {
    let memory = vec![0x00, 0x01, 0x02, 0x03, 0x04, 0x05];
    let search_value = vec![0xFF];

    let results = search_memory(&memory, &search_value, SearchType::Byte, 0xA000);

    assert_eq!(results.len(), 0);
}

#[test]
fn test_search_at_memory_boundaries() {
    let memory = vec![0x00, 0x00, 0x00, 0x42]; // Value at the end
    let search_value = vec![0x42];

    let results = search_memory(&memory, &search_value, SearchType::Byte, 0xC000);

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].addr, 0xC003);
}

#[test]
fn test_search_large_addresses() {
    let memory = vec![0x00, 0x42, 0x00, 0x42];
    let search_value = vec![0x42];
    let base_address = 0x7FFFFFFF0000; // Large address

    let results = search_memory(&memory, &search_value, SearchType::Byte, base_address);

    assert_eq!(results.len(), 2);
    assert_eq!(results[0].addr, base_address + 1);
    assert_eq!(results[1].addr, base_address + 3);
}

#[cfg(target_arch = "x86_64")]
#[test]
fn test_simd_search_performance() {
    use std::time::Instant;

    // Create a large memory block
    let mut memory = vec![0x00; 1024 * 1024]; // 1MB

    // Place values at various positions
    for i in (0..memory.len()).step_by(1024) {
        memory[i..i + 4].copy_from_slice(&0x12345678u32.to_le_bytes());
    }

    let search_value = 0x12345678u32.to_le_bytes().to_vec();

    let start = Instant::now();
    let results = search_memory(&memory, &search_value, SearchType::Int, 0x10000000);
    let duration = start.elapsed();

    println!("SIMD Int32 search took: {:?} for 1MB", duration);
    assert_eq!(results.len(), 1024);
    assert!(duration.as_millis() < 100); // Should be fast
}

#[cfg(target_arch = "x86_64")]
#[test]
fn test_simd_float_search_performance() {
    use std::time::Instant;

    let mut memory = vec![0x00; 1024 * 1024]; // 1MB
    let target: f32 = 42.125;

    for i in (0..memory.len()).step_by(1024) {
        memory[i..i + 4].copy_from_slice(&target.to_le_bytes());
    }

    let search_value = target.to_le_bytes().to_vec();

    let start = Instant::now();
    let results = search_memory(&memory, &search_value, SearchType::Float, 0x10000000);
    let duration = start.elapsed();

    println!("SIMD Float32 search took: {:?} for 1MB", duration);
    assert_eq!(results.len(), 1024);
    assert!(duration.as_millis() < 100);
}

#[cfg(target_arch = "x86_64")]
#[test]
fn test_simd_short_search_performance() {
    use std::time::Instant;

    let mut memory = vec![0x00; 1024 * 1024]; // 1MB
    let target: u16 = 0x1234;

    for i in (0..memory.len()).step_by(1024) {
        memory[i..i + 2].copy_from_slice(&target.to_le_bytes());
    }

    let search_value = target.to_le_bytes().to_vec();

    let start = Instant::now();
    let results = search_memory(&memory, &search_value, SearchType::Short, 0x10000000);
    let duration = start.elapsed();

    println!("SIMD Int16 search took: {:?} for 1MB", duration);
    assert_eq!(results.len(), 1024);
    assert!(duration.as_millis() < 100);
}

#[cfg(target_arch = "x86_64")]
#[test]
fn test_simd_double_search_performance() {
    use std::time::Instant;

    let mut memory = vec![0x00; 1024 * 1024]; // 1MB
    let target: f64 = 42.12345678;

    for i in (0..memory.len()).step_by(1024) {
        memory[i..i + 8].copy_from_slice(&target.to_le_bytes());
    }

    let search_value = target.to_le_bytes().to_vec();

    let start = Instant::now();
    let results = search_memory(&memory, &search_value, SearchType::Double, 0x10000000);
    let duration = start.elapsed();

    println!("SIMD Float64 search took: {:?} for 1MB", duration);
    assert_eq!(results.len(), 1024);
    assert!(duration.as_millis() < 100);
}

#[test]
fn test_search_special_float_values() {
    let mut memory = vec![0x00; 16];

    // Test NaN
    let nan = f32::NAN;
    memory[0..4].copy_from_slice(&nan.to_le_bytes());

    // Test Infinity
    let inf = f32::INFINITY;
    memory[4..8].copy_from_slice(&inf.to_le_bytes());

    // Test negative zero
    let neg_zero = -0.0f32;
    memory[8..12].copy_from_slice(&neg_zero.to_le_bytes());

    // Search for infinity
    let search_value = inf.to_le_bytes().to_vec();
    let results = search_memory(&memory, &search_value, SearchType::Float, 0xD000);

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].addr, 0xD004);
}

#[test]
fn test_result_ordering() {
    let memory = vec![0x42, 0x00, 0x42, 0x00, 0x42];
    let search_value = vec![0x42];

    let results = search_memory(&memory, &search_value, SearchType::Byte, 0xE000);

    // Results should be ordered by address
    assert_eq!(results.len(), 3);
    assert!(results[0].addr < results[1].addr);
    assert!(results[1].addr < results[2].addr);
}
