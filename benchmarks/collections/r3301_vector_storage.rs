use std::collections::VecDeque;
use std::hint::black_box;
use std::time::Instant;

const FIXED_CAPACITY: usize = 4096;

fn emit(name: &str, size: usize, started: Instant, checksum: i64, before: usize, after: usize) {
    let elapsed_ns = started.elapsed().as_nanos();
    let operations = size * 2;
    let bytes = after * std::mem::size_of::<i64>();
    println!(
        "R3301VECTOR|{name}|{size}|{elapsed_ns}|{operations}|{checksum}|{before}|{after}|{bytes}"
    );
}

fn benchmark_vec_deque(size: usize) {
    let mut values = VecDeque::with_capacity(size);
    let capacity = values.capacity();
    let started = Instant::now();
    for value in 0..size {
        values.push_back(black_box(value as i64));
    }
    let mut checksum = 0_i64;
    for index in 0..size {
        checksum += black_box(values[index]);
    }
    black_box(checksum);
    emit("vec_deque", size, started, checksum, capacity, values.capacity());
}

fn benchmark_vec(size: usize) {
    let mut values = Vec::with_capacity(size);
    let capacity = values.capacity();
    let started = Instant::now();
    for value in 0..size {
        values.push(black_box(value as i64));
    }
    let mut checksum = 0_i64;
    for index in 0..size {
        checksum += black_box(values[index]);
    }
    black_box(checksum);
    emit("vec_prototype", size, started, checksum, capacity, values.capacity());
}

fn benchmark_fixed_array(size: usize) {
    let mut values = [0_i64; FIXED_CAPACITY];
    let started = Instant::now();
    for (index, value) in values.iter_mut().take(size).enumerate() {
        *value = black_box(index as i64);
    }
    let mut checksum = 0_i64;
    for value in values.iter().take(size) {
        checksum += black_box(*value);
    }
    black_box(checksum);
    emit(
        "fixed_array_storage_probe",
        size,
        started,
        checksum,
        FIXED_CAPACITY,
        FIXED_CAPACITY,
    );
}

fn main() {
    for size in [64, 512, FIXED_CAPACITY] {
        benchmark_vec_deque(size);
        benchmark_vec(size);
        benchmark_fixed_array(size);
    }
}
