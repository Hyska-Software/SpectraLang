include!("gpu_core_kernels.rs");

include!("gpu_device_dispatch.rs");

#[cfg(test)]
mod gpu_reduce_tests {
    include!("gpu_reduce_tests.rs");
}

include!("gpu_runtime.rs");
