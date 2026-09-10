use super::*;
// ── std.tensor runtime ──────────────────────────────────────────────────────

pub const NUMERICAL_TOLERANCE_ABS: f64 = 1.0e-9;
pub const NUMERICAL_TOLERANCE_REL: f64 = 1.0e-9;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TensorDType {
    Int,
    Float,
}

impl TensorDType {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Int => "int",
            Self::Float => "float",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TensorLayout {
    Contiguous,
    View,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TensorDevice {
    Cpu,
    Cuda,
    Rocm,
    Metal,
    DirectMl,
    Vulkan,
    Wgpu,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TensorPrecision {
    F64,
    F32,
    F16,
    Bf16,
}

impl TensorPrecision {
    pub(crate) fn from_code(code: SpectraHostValue) -> Option<Self> {
        match code {
            0 => Some(Self::F64),
            1 => Some(Self::F32),
            2 => Some(Self::F16),
            3 => Some(Self::Bf16),
            _ => None,
        }
    }

    pub(crate) fn code(self) -> SpectraHostValue {
        match self {
            Self::F64 => 0,
            Self::F32 => 1,
            Self::F16 => 2,
            Self::Bf16 => 3,
        }
    }

    pub(crate) fn quantize(self, value: f64) -> f64 {
        match self {
            Self::F64 => value,
            Self::F32 => value as f32 as f64,
            Self::F16 => half::f16::from_f64(value).to_f64(),
            Self::Bf16 => half::bf16::from_f64(value).to_f64(),
        }
    }
}

impl TensorDevice {
    pub(crate) fn from_code(code: SpectraHostValue) -> Option<Self> {
        match code {
            0 => Some(Self::Cpu),
            1 => Some(Self::Cuda),
            2 => Some(Self::Rocm),
            3 => Some(Self::Metal),
            4 => Some(Self::DirectMl),
            5 => Some(Self::Vulkan),
            6 => Some(Self::Wgpu),
            _ => None,
        }
    }

    pub(crate) fn code(self) -> SpectraHostValue {
        match self {
            Self::Cpu => 0,
            Self::Cuda => 1,
            Self::Rocm => 2,
            Self::Metal => 3,
            Self::DirectMl => 4,
            Self::Vulkan => 5,
            Self::Wgpu => 6,
        }
    }

    pub(crate) fn is_available(self) -> bool {
        match self {
            Self::Cpu => true,
            Self::Wgpu => {
                #[cfg(feature = "gpu")]
                {
                    crate::gpu::is_available()
                }
                #[cfg(not(feature = "gpu"))]
                {
                    false
                }
            }
            _ => false,
        }
    }

    /// Returns true if this build has any implementation for this device code.
    /// CUDA, ROCm, Metal, DirectML, and Vulkan are reserved device codes
    /// with no implementation in this build; they must surface as
    /// `HOST_STATUS_INVALID_ARGUMENT` to the caller, not as a fake
    /// "reserved but not implemented" status that misleads users into
    /// expecting a future backend.
    pub(crate) fn is_implemented(self) -> bool {
        matches!(self, Self::Cpu | Self::Wgpu)
    }

    pub(crate) fn is_accelerator(self) -> bool {
        !matches!(self, Self::Cpu)
    }

    pub(crate) fn status_code(self) -> SpectraHostValue {
        if self.is_available() {
            return 0;
        }
        match self {
            Self::Cpu => 0,
            Self::Wgpu => 1,
            Self::Cuda | Self::Rocm | Self::Metal | Self::DirectMl | Self::Vulkan => 2,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AutogradOp {
    Add,
    Sub,
    Mul,
    Div,
    Neg,
    Relu,
    Exp,
    Log,
    Sqrt,
    Sigmoid,
    Tanh,
    SumTensor,
    MeanTensor,
    Matmul,
    BatchedMatmul,
    Transpose,
    DotTensor,
    View,
    Concat,
    Stack,
    Slice,
    Permute,
    MlLinear,
    MlConv2d,
    MaxPool2d,
    Dropout,
    MlMse,
    MlBce,
    MlCrossEntropy,
    MlNll,
}

#[derive(Debug, Clone)]
pub(crate) struct AutogradNode {
    pub(crate) op: AutogradOp,
    pub(crate) parents: Vec<usize>,
    pub(crate) input_shape: Vec<usize>,
    pub(crate) left_shape: Vec<usize>,
    pub(crate) right_shape: Vec<usize>,
    pub(crate) input: Vec<f64>,
    pub(crate) output: Vec<f64>,
    pub(crate) left: Vec<f64>,
    pub(crate) right: Vec<f64>,
    pub(crate) aux: Vec<usize>,
    #[cfg(feature = "gpu")]
    pub(crate) device_aux: Option<crate::gpu::DeviceBuffer>,
}

impl AutogradNode {
    pub(crate) fn unary(
        op: AutogradOp,
        parent: usize,
        input_shape: Vec<usize>,
        input: Vec<f64>,
        output: Vec<f64>,
    ) -> Self {
        Self {
            op,
            parents: vec![parent],
            input_shape: input_shape.clone(),
            left_shape: Vec::new(),
            right_shape: Vec::new(),
            input,
            output,
            left: Vec::new(),
            right: Vec::new(),
            aux: Vec::new(),
            #[cfg(feature = "gpu")]
            device_aux: None,
        }
    }

    pub(crate) fn binary(
        op: AutogradOp,
        left_parent: usize,
        right_parent: usize,
        shape: Vec<usize>,
        left: Vec<f64>,
        right: Vec<f64>,
    ) -> Self {
        Self {
            op,
            parents: vec![left_parent, right_parent],
            input_shape: shape.clone(),
            left_shape: Vec::new(),
            right_shape: Vec::new(),
            input: Vec::new(),
            output: Vec::new(),
            left,
            right,
            aux: Vec::new(),
            #[cfg(feature = "gpu")]
            device_aux: None,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct StdTensor {
    pub(crate) dtype: TensorDType,
    pub(crate) shape: Vec<usize>,
    pub(crate) strides: Vec<usize>,
    pub(crate) storage: Arc<Vec<SpectraHostValue>>,
    pub(crate) offset: usize,
    pub(crate) layout: TensorLayout,
    pub(crate) device: TensorDevice,
    pub(crate) precision: TensorPrecision,
    pub(crate) requires_grad: bool,
    pub(crate) grad: Option<Vec<f64>>,
    pub(crate) creator: Option<AutogradNode>,
    /// R-3052 (minimal): optional device-resident buffers, keyed by the
    /// pool's device tag. Populated by `to_device` after R-3021 lands.
    /// Not yet read by the GPU op sites — that is R-3052 full.
    #[cfg(feature = "gpu")]
    pub(crate) device_storage:
        std::collections::HashMap<crate::gpu::PoolDevice, crate::gpu::DeviceBuffer>,
    /// R-3052 full: optional device-resident gradient buffer, keyed by
    /// the pool's device tag. Populated by the GPU backward path when
    /// the parent is device-resident; consumed by the residency-aware
    /// `sgd_step`. Mirrors `device_storage` for the grad slot.
    #[cfg(feature = "gpu")]
    pub(crate) device_grad:
        std::collections::HashMap<crate::gpu::PoolDevice, crate::gpu::DeviceBuffer>,
}

impl StdTensor {
    pub(crate) fn new(
        dtype: TensorDType,
        shape: Vec<usize>,
        data: Vec<SpectraHostValue>,
    ) -> Option<Self> {
        let expected_len = shape
            .iter()
            .try_fold(1usize, |acc, dim| acc.checked_mul(*dim))?;
        if expected_len != data.len() {
            return None;
        }
        Self::from_storage(
            dtype,
            shape.clone(),
            tensor_strides(&shape),
            Arc::new(data),
            0,
            TensorLayout::Contiguous,
        )
    }

    pub(crate) fn from_storage(
        dtype: TensorDType,
        shape: Vec<usize>,
        strides: Vec<usize>,
        storage: Arc<Vec<SpectraHostValue>>,
        offset: usize,
        layout: TensorLayout,
    ) -> Option<Self> {
        if shape.is_empty() || shape.contains(&0) {
            return None;
        }
        let expected_len = shape
            .iter()
            .try_fold(1usize, |acc, dim| acc.checked_mul(*dim))?;
        if strides.len() != shape.len() {
            return None;
        }
        if expected_len == 0 {
            return None;
        }
        let max_offset = max_tensor_offset(&shape, &strides, offset)?;
        if max_offset >= storage.len() {
            return None;
        }
        Some(Self {
            dtype,
            shape,
            strides,
            storage,
            offset,
            layout,
            device: TensorDevice::Cpu,
            precision: TensorPrecision::F64,
            requires_grad: false,
            grad: None,
            creator: None,
            #[cfg(feature = "gpu")]
            device_storage: std::collections::HashMap::new(),
            #[cfg(feature = "gpu")]
            device_grad: std::collections::HashMap::new(),
        })
    }

    pub(crate) fn len(&self) -> usize {
        self.shape
            .iter()
            .fold(1usize, |acc, dim| acc.saturating_mul(*dim))
    }

    pub(crate) fn offset(&self, indices: &[usize]) -> Option<usize> {
        if indices.len() != self.shape.len() {
            return None;
        }
        let mut offset = self.offset;
        for ((idx, dim), stride) in indices
            .iter()
            .zip(self.shape.iter())
            .zip(self.strides.iter())
        {
            if *idx >= *dim {
                return None;
            }
            offset = offset.checked_add(idx.checked_mul(*stride)?)?;
        }
        Some(offset)
    }

    pub(crate) fn linear_offset(&self, index: usize) -> Option<usize> {
        if index >= self.len() {
            return None;
        }
        if self.layout == TensorLayout::Contiguous {
            return self.offset.checked_add(index);
        }

        let mut remaining = index;
        let mut offset = self.offset;
        for axis in (0..self.shape.len()).rev() {
            let dim = self.shape[axis];
            let axis_index = remaining % dim;
            remaining /= dim;
            offset = offset.checked_add(axis_index.checked_mul(self.strides[axis])?)?;
        }
        Some(offset)
    }

    pub(crate) fn value_at_linear(&self, index: usize) -> Option<SpectraHostValue> {
        let offset = self.linear_offset(index)?;
        self.storage.get(offset).copied()
    }

    pub(crate) fn materialize(&self) -> Vec<SpectraHostValue> {
        if self.layout == TensorLayout::Contiguous
            && self.offset == 0
            && self.storage.len() == self.len()
        {
            return self.storage.as_ref().clone();
        }
        (0..self.len())
            .filter_map(|index| self.value_at_linear(index))
            .collect()
    }

    pub(crate) fn set_linear(&mut self, index: usize, value: SpectraHostValue) -> bool {
        let Some(offset) = self.linear_offset(index) else {
            return false;
        };
        let storage = Arc::make_mut(&mut self.storage);
        if offset >= storage.len() {
            return false;
        }
        storage[offset] = value;
        true
    }

    pub(crate) fn storage_bytes(&self) -> usize {
        self.len()
            .saturating_mul(std::mem::size_of::<SpectraHostValue>())
    }

    pub(crate) fn is_contiguous(&self) -> bool {
        self.layout == TensorLayout::Contiguous && self.strides == tensor_strides(&self.shape)
    }
}

pub(crate) struct TensorRegistry {
    pub(crate) tensors: HandleTable<ManualBox<StdTensor>>,
    pub(crate) pool: Vec<Vec<SpectraHostValue>>,
    pub(crate) metrics: TensorMetrics,
    pub(crate) memory_step: usize,
    pub(crate) lifetimes: Vec<TensorLifetimeRecord>,
    pub(crate) active_lifetimes: HashMap<usize, usize>,
    /// R-3051: device buffer pool. Source of truth for the
    /// `stats_device_pool_*` host calls. Held inside the registry
    /// mutex, no extra lock surface.
    #[cfg(feature = "gpu")]
    pub(crate) device_arena: crate::gpu::DeviceArena,
}

#[derive(Debug, Clone)]
pub(crate) struct TensorLifetimeRecord {
    pub(crate) handle: usize,
    pub(crate) dtype: TensorDType,
    pub(crate) shape: Vec<usize>,
    pub(crate) bytes: usize,
    pub(crate) allocation_step: usize,
    pub(crate) release_step: Option<usize>,
    pub(crate) allocation_site: String,
}

impl TensorRegistry {
    pub(crate) fn new() -> Self {
        Self {
            tensors: HandleTable::new(HandleKind::Tensor),
            pool: Vec::new(),
            metrics: TensorMetrics::default(),
            memory_step: 0,
            lifetimes: Vec::new(),
            active_lifetimes: HashMap::new(),
            #[cfg(feature = "gpu")]
            device_arena: crate::gpu::DeviceArena::new(),
        }
    }

    pub(crate) fn insert(
        &mut self,
        tensor: ManualBox<StdTensor>,
        allocation_site: impl Into<String>,
    ) -> usize {
        let bytes = tensor
            .len()
            .saturating_mul(std::mem::size_of::<SpectraHostValue>());
        self.metrics.allocations = self.metrics.allocations.saturating_add(1);
        self.metrics.active_tensors = self.metrics.active_tensors.saturating_add(1);
        self.metrics.active_bytes = self.metrics.active_bytes.saturating_add(bytes);
        self.metrics.peak_bytes = self.metrics.peak_bytes.max(self.metrics.active_bytes);

        let dtype = tensor.dtype;
        let shape = tensor.shape.clone();
        let handle = self.tensors.insert(tensor).raw() as usize;
        self.memory_step = self.memory_step.saturating_add(1);
        let record = TensorLifetimeRecord {
            handle,
            dtype,
            shape,
            bytes,
            allocation_step: self.memory_step,
            release_step: None,
            allocation_site: allocation_site.into(),
        };
        let record_index = self.lifetimes.len();
        self.lifetimes.push(record);
        self.active_lifetimes.insert(handle, record_index);
        handle
    }

    pub(crate) fn remove(&mut self, handle: usize) -> Result<(), i32> {
        let id = HandleId::from_raw(handle as i64).map_err(|_| HOST_STATUS_NOT_FOUND)?;
        if let Ok(tensor) = self.tensors.remove(id) {
            self.mark_released(handle);
            self.recycle_tensor(tensor);
            Ok(())
        } else {
            Err(HOST_STATUS_NOT_FOUND)
        }
    }

    pub(crate) fn clear_all(&mut self) -> usize {
        let tensors = self.tensors.drain();
        let count = tensors.len();
        let handles = tensors
            .iter()
            .map(|(handle, _)| handle.raw() as usize)
            .collect::<Vec<_>>();
        for handle in handles {
            self.mark_released(handle);
        }
        for (_, tensor) in tensors {
            self.recycle_tensor(tensor);
        }
        count
    }

    pub(crate) fn get(&self, handle: usize) -> Option<&StdTensor> {
        let id = HandleId::from_raw(handle as i64).ok()?;
        self.tensors.get(id).ok().map(|boxed| boxed.as_ref())
    }

    pub(crate) fn get_mut(&mut self, handle: usize) -> Option<&mut StdTensor> {
        let id = HandleId::from_raw(handle as i64).ok()?;
        self.tensors.get_mut(id).ok().map(|boxed| boxed.as_mut())
    }

    pub(crate) fn take(&mut self, handle: usize) -> Option<ManualBox<StdTensor>> {
        let id = HandleId::from_raw(handle as i64).ok()?;
        self.tensors.take(id).ok()
    }

    pub(crate) fn put(&mut self, handle: usize, tensor: ManualBox<StdTensor>) -> bool {
        let Some(id) = HandleId::from_raw(handle as i64).ok() else {
            return false;
        };
        self.tensors.put(id, tensor).is_ok()
    }

    pub(crate) fn mark_released(&mut self, handle: usize) {
        self.memory_step = self.memory_step.saturating_add(1);
        if let Some(index) = self.active_lifetimes.remove(&handle) {
            if let Some(record) = self.lifetimes.get_mut(index) {
                record.release_step = Some(self.memory_step);
            }
        }
    }

    pub(crate) fn recycle_tensor(&mut self, tensor: ManualBox<StdTensor>) {
        #[allow(unused_mut)]
        let mut tensor = tensor.into_inner();
        let bytes = tensor.storage_bytes();
        self.metrics.active_tensors = self.metrics.active_tensors.saturating_sub(1);
        self.metrics.active_bytes = self.metrics.active_bytes.saturating_sub(bytes);
        #[cfg(feature = "gpu")]
        {
            for (_, buf) in tensor.device_storage.drain() {
                self.device_arena.release(buf);
            }
            for (_, buf) in tensor.device_grad.drain() {
                self.device_arena.release(buf);
            }
        }
        #[cfg(not(feature = "gpu"))]
        {
            let _ = tensor;
        }
        if tensor.offset == 0 && tensor.is_contiguous() && Arc::strong_count(&tensor.storage) == 1 {
            if let Ok(data) = Arc::try_unwrap(tensor.storage) {
                let capacity = data.capacity();
                if self.pool.len() < 32 {
                    self.pool.push(data);
                } else if let Some((replace_index, _)) = self
                    .pool
                    .iter()
                    .enumerate()
                    .filter(|(_, buffer)| buffer.capacity() < capacity)
                    .min_by_key(|(_, buffer)| buffer.capacity())
                {
                    self.pool[replace_index] = data;
                }
            }
        }
    }

    pub(crate) fn take_buffer(&mut self, len: usize) -> Vec<SpectraHostValue> {
        if let Some(buffer) = self.take_buffer_unfilled(len) {
            self.metrics.reused_buffers = self.metrics.reused_buffers.saturating_add(1);
            self.metrics.pool_hits = self.metrics.pool_hits.saturating_add(1);
            buffer
        } else {
            self.metrics.pool_misses = self.metrics.pool_misses.saturating_add(1);
            vec![0; len]
        }
    }

    pub(crate) fn take_buffer_unfilled(&mut self, len: usize) -> Option<Vec<SpectraHostValue>> {
        let index = self
            .pool
            .iter()
            .position(|buffer| buffer.capacity() >= len)?;
        let mut buffer = self.pool.swap_remove(index);
        if buffer.capacity() >= len {
            unsafe {
                buffer.set_len(len);
            }
        } else {
            buffer.resize(len, 0);
        }
        Some(buffer)
    }

    #[allow(dead_code)]
    pub(crate) fn reset_pool(&mut self) {
        self.pool.clear();
    }

    pub(crate) fn note_kernel(&mut self, elements: usize) {
        self.metrics.kernel_ops = self.metrics.kernel_ops.saturating_add(1);
        self.metrics.kernel_elements = self.metrics.kernel_elements.saturating_add(elements);
    }

    pub(crate) fn note_scratch_reuse(&mut self) {
        self.metrics.scratch_reuses = self.metrics.scratch_reuses.saturating_add(1);
    }

    pub(crate) fn note_device_transfer(&mut self) {
        self.metrics.device_transfers = self.metrics.device_transfers.saturating_add(1);
    }

    #[allow(dead_code)]
    pub(crate) fn note_gpu_kernel(&mut self) {
        self.metrics.gpu_kernel_ops = self.metrics.gpu_kernel_ops.saturating_add(1);
    }

    #[allow(dead_code)]
    pub(crate) fn note_cpu_fallback(&mut self) {
        self.metrics.cpu_fallbacks = self.metrics.cpu_fallbacks.saturating_add(1);
    }

    /// R-3052: count tensors that live on a device (any device). The
    /// `to_device` path increments this when it uploads a tensor to
    /// Wgpu. The host-side `device` field on `StdTensor` is the source
    /// of truth; this counter is the rolled-up view exposed through
    /// `stats_device_resident_tensors`.
    #[allow(dead_code)]
    pub(crate) fn note_device_resident(&mut self) {
        self.metrics.device_resident_tensors =
            self.metrics.device_resident_tensors.saturating_add(1);
    }

    /// R-3023: record a typed GPU error so callers can see per-kind
    /// counters via `std_tensor_stats_gpu_errors(kind)`.
    #[cfg(feature = "gpu")]
    pub(crate) fn note_gpu_error(&mut self, kind: crate::gpu::GpuErrorKind) {
        let code = kind.code();
        if (0..self.metrics.gpu_errors.len() as i32).contains(&code) {
            self.metrics.gpu_errors[code as usize] =
                self.metrics.gpu_errors[code as usize].saturating_add(1);
        }
    }

    /// R-3023: non-gpu build stub. The metric is still tracked by code so
    /// user code that compiles without --features gpu sees the same counter
    /// layout. The kind arg is ignored; only the slot for `Other` (6) is
    /// ever incremented in this build (which is unreachable in practice).
    #[cfg(not(feature = "gpu"))]
    #[allow(dead_code)]
    pub(crate) fn note_gpu_error(&mut self, _kind: u8) {
        self.metrics.gpu_errors[6] = self.metrics.gpu_errors[6].saturating_add(1);
    }

    pub(crate) fn reset_metrics(&mut self) {
        let active_tensors = self.tensors.len();
        let active_bytes = self
            .tensors
            .values()
            .map(|tensor| tensor.storage_bytes())
            .sum();
        self.metrics = TensorMetrics {
            active_tensors,
            active_bytes,
            peak_bytes: active_bytes,
            ..Default::default()
        };
        self.memory_step = 0;
        self.lifetimes.clear();
        self.active_lifetimes.clear();
        #[cfg(feature = "gpu")]
        {
            self.device_arena.reset();
        }
        let snapshots = self
            .tensors
            .iter()
            .map(|(handle, tensor)| {
                (
                    handle.raw() as usize,
                    tensor.dtype,
                    tensor.shape.clone(),
                    tensor.storage_bytes(),
                )
            })
            .collect::<Vec<_>>();
        for (handle, dtype, shape, bytes) in snapshots {
            self.memory_step = self.memory_step.saturating_add(1);
            let index = self.lifetimes.len();
            self.lifetimes.push(TensorLifetimeRecord {
                handle,
                dtype,
                shape,
                bytes,
                allocation_step: self.memory_step,
                release_step: None,
                allocation_site: "reset_stats.active_snapshot".to_string(),
            });
            self.active_lifetimes.insert(handle, index);
        }
    }

    pub(crate) fn allocation_site_count(&self) -> usize {
        self.lifetimes
            .iter()
            .map(|record| record.allocation_site.as_str())
            .collect::<HashSet<_>>()
            .len()
    }

    pub(crate) fn released_lifetime_count(&self) -> usize {
        self.lifetimes
            .iter()
            .filter(|record| record.release_step.is_some())
            .count()
    }

    pub(crate) fn reuse_rate_per_mille(&self) -> usize {
        let total = self
            .metrics
            .pool_hits
            .saturating_add(self.metrics.pool_misses);
        if total == 0 {
            return 0;
        }
        self.metrics.pool_hits.saturating_mul(1000) / total
    }

    pub(crate) fn memory_report_json(&self) -> String {
        let mut out = String::new();
        out.push_str("{\"schema\":\"spectra.tensor.memory_report.v1\"");
        out.push_str(&format!(
            ",\"allocations\":{},\"active_tensors\":{},\"active_bytes\":{},\"peak_bytes\":{},\"reused_buffers\":{},\"pool_hits\":{},\"pool_misses\":{},\"reuse_rate_per_mille\":{},\"scratch_reuses\":{},\"allocation_sites\":{},\"lifetime_records\":{},\"released_lifetimes\":{}",
            self.metrics.allocations,
            self.metrics.active_tensors,
            self.metrics.active_bytes,
            self.metrics.peak_bytes,
            self.metrics.reused_buffers,
            self.metrics.pool_hits,
            self.metrics.pool_misses,
            self.reuse_rate_per_mille(),
            self.metrics.scratch_reuses,
            self.allocation_site_count(),
            self.lifetimes.len(),
            self.released_lifetime_count()
        ));
        out.push_str(&format!(
            ",\"kernel_ops\":{},\"kernel_elements\":{},\"device_transfers\":{},\"gpu_kernel_ops\":{},\"cpu_fallbacks\":{}",
            self.metrics.kernel_ops,
            self.metrics.kernel_elements,
            self.metrics.device_transfers,
            self.metrics.gpu_kernel_ops,
            self.metrics.cpu_fallbacks
        ));
        out.push_str(&format!(
            ",\"gpu_errors\":[{},{},{},{},{},{},{}]",
            self.metrics.gpu_errors[0],
            self.metrics.gpu_errors[1],
            self.metrics.gpu_errors[2],
            self.metrics.gpu_errors[3],
            self.metrics.gpu_errors[4],
            self.metrics.gpu_errors[5],
            self.metrics.gpu_errors[6]
        ));
        out.push_str(",\"tensors\":[");
        for (index, record) in self.lifetimes.iter().enumerate() {
            if index > 0 {
                out.push(',');
            }
            out.push_str(&format!(
                "{{\"handle\":{},\"dtype\":\"{}\",\"shape\":[{}],\"bytes\":{},\"allocation_step\":{},\"release_step\":{},\"active\":{},\"allocation_site\":\"{}\"}}",
                record.handle,
                record.dtype.name(),
                record
                    .shape
                    .iter()
                    .map(|dim| dim.to_string())
                    .collect::<Vec<_>>()
                    .join(","),
                record.bytes,
                record.allocation_step,
                record
                    .release_step
                    .map(|step| step.to_string())
                    .unwrap_or_else(|| "null".to_string()),
                record.release_step.is_none(),
                json_escape(&record.allocation_site)
            ));
        }
        out.push_str("]}");
        out
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct TensorMetrics {
    pub(crate) allocations: usize,
    pub(crate) active_tensors: usize,
    pub(crate) active_bytes: usize,
    pub(crate) peak_bytes: usize,
    pub(crate) reused_buffers: usize,
    pub(crate) pool_hits: usize,
    pub(crate) pool_misses: usize,
    pub(crate) scratch_reuses: usize,
    pub(crate) kernel_ops: usize,
    pub(crate) kernel_elements: usize,
    pub(crate) device_transfers: usize,
    pub(crate) gpu_kernel_ops: usize,
    pub(crate) cpu_fallbacks: usize,
    /// R-3052: number of tensors that currently live on a device
    /// (incremented on `to_device`, decremented on free/reset). Surface
    /// of residency through `std_tensor_stats_device_resident_tensors`.
    pub(crate) device_resident_tensors: usize,
    /// Per-kind GPU error counter (R-3023). Indexed by `GpuErrorKind::code()`.
    /// 0 = ShapeMismatch, 1 = ShaderCompile, 2 = BufferAlloc, 3 = Dispatch,
    /// 4 = Readback, 5 = FeatureUnsupported, 6 = Other.
    pub(crate) gpu_errors: [usize; 7],
}

pub(crate) fn tensor_registry() -> &'static Mutex<TensorRegistry> {
    static REGISTRY: OnceLock<Mutex<TensorRegistry>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(TensorRegistry::new()))
}

pub(crate) fn tensor_grad_enabled() -> &'static Mutex<bool> {
    static ENABLED: OnceLock<Mutex<bool>> = OnceLock::new();
    ENABLED.get_or_init(|| Mutex::new(true))
}

pub(crate) fn tensor_deterministic_mode() -> &'static Mutex<bool> {
    static ENABLED: OnceLock<Mutex<bool>> = OnceLock::new();
    ENABLED.get_or_init(|| Mutex::new(false))
}

/// R-3080: global counter for GPU backward kernels that ran end-to-end
/// without falling back to CPU. Lives outside `TensorRegistry` because
/// the increment happens inside the autograd hot path, where the
/// registry mutex is already held. Cleared on `reset_stats`.
pub(crate) fn gpu_backward_ops_counter() -> &'static std::sync::atomic::AtomicUsize {
    static COUNTER: OnceLock<std::sync::atomic::AtomicUsize> = OnceLock::new();
    COUNTER.get_or_init(|| std::sync::atomic::AtomicUsize::new(0))
}

pub(crate) fn note_gpu_backward_op() {
    gpu_backward_ops_counter().fetch_add(1, std::sync::atomic::Ordering::Relaxed);
}

#[allow(dead_code)]
pub(crate) fn _ensure_note_gpu_backward_op_linked() {
    note_gpu_backward_op();
}
