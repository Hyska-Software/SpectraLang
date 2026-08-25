impl CodeGenerator {
    /// Create a new code generator
    pub fn new() -> Self {
        // R-3129: opt into Cranelift's speed optimizer for JIT code.
        // The default `JITBuilder::new` uses `opt_level = "none"`, which
        // skips almost all mid-end optimization passes and produces
        // measurably slower native code. See `cranelift-codegen` settings
        // for the full list of options.
        let mut builder = JITBuilder::with_flags(
            &[
                ("opt_level", "speed"),
                // Cranelift's x64 `return_call` implementation restores the
                // caller's frame pointer, so it requires frame pointers to be
                // preserved (see emit_return_call_common_sequence).
                ("preserve_frame_pointers", "true"),
            ],
            cranelift_module::default_libcall_names(),
        )
        .expect("Failed to create JIT builder");

        register_jit_runtime_symbols(&mut builder);

        let mut module = JITModule::new(builder);
        let ctx = module.make_context();

        let runtime_bindings =
            declare_runtime_bindings(&mut module).expect("Failed to declare runtime ABI imports");

        Self {
            module,
            ctx,
            builder_context: FunctionBuilderContext::new(),
            function_map: HashMap::new(),
            global_data: HashMap::new(),
            finalized_function_ptrs: HashMap::new(),
            runtime_bindings,
            string_literal_data: HashMap::new(),
            hostcall_batch_stats: HostCallBatchStats::default(),
            string_literal_storage: Vec::new(),
            host_call_sites: HashMap::new(),
            host_name_storage: Vec::new(),
            host_call_cache_storage: Vec::new(),
            jit_debug_functions: Vec::new(),
            #[cfg(test)]
            last_finalized_func: None,
        }
    }


    /// Drain the JIT debug sidecar entries collected during code generation:
    /// one `(function name, variables)` pair per function that had at least
    /// one compiler-proven variable range. The CLI run path serializes this
    /// via [`crate::debug::write_jit_debug_sidecar`].
    pub fn take_jit_debug_functions(
        &mut self,
    ) -> Vec<(String, Vec<crate::debug::JitDebugVariable>)> {
        std::mem::take(&mut self.jit_debug_functions)
    }
    /// Returns the hostcall batching plan emitted by the most recent module.
    pub fn hostcall_batch_stats(&self) -> HostCallBatchStats {
        self.hostcall_batch_stats
    }

    /// Generate code for an entire module
    pub fn generate_module(&mut self, ir_module: &IRModule) -> BackendResult<()> {
        self.hostcall_batch_stats = HostCallBatchStats::default();
        let _tensor_ir = validate_tensor_ir(ir_module)?;
        self.pre_intern_host_names(ir_module);
        self.define_globals(ir_module)?;
        let function_params: HashMap<String, Vec<IRType>> = ir_module
            .functions
            .iter()
            .map(|func| {
                (
                    func.name.clone(),
                    func.params.iter().map(|param| param.ty.clone()).collect(),
                )
            })
            .collect();
        // First pass: declare all functions
        for func in &ir_module.functions {
            self.declare_function(func)?;
        }

        // Second pass: define all functions
        for func in &ir_module.functions {
            self.define_function(func, &function_params)?;
        }

        // Finalize all functions
        self.module.finalize_definitions().map_err(|e| {
            BackendCodegenError::cranelift(format!("Failed to finalize definitions: {}", e))
        })?;

        for func in &ir_module.functions {
            let Some(&func_id) = self.function_map.get(&func.name) else {
                continue;
            };
            let ptr = self.module.get_finalized_function(func_id) as usize as i64;
            self.finalized_function_ptrs.insert(func.name.clone(), ptr);
        }

        Ok(())
    }

    pub(crate) fn global_symbol(module_name: &str, global_name: &str) -> String {
        let sanitize = |value: &str| {
            value
                .chars()
                .map(|ch| {
                    if ch.is_ascii_alphanumeric() || ch == '_' {
                        ch
                    } else {
                        '_'
                    }
                })
                .collect::<String>()
        };
        let qualified_name = if global_name.contains("::") {
            global_name.to_string()
        } else {
            format!("{}::{}", module_name, global_name)
        };
        format!(".__spectra_global_{}", sanitize(&qualified_name))
    }

    pub(crate) fn global_initializer_bytes(global: &Global) -> BackendResult<Vec<u8>> {
        let size = Self::type_size_bytes(&global.ty);
        let mut bytes = vec![0u8; size];
        let initializer = global.initializer.as_ref().ok_or_else(|| {
            BackendCodegenError::invalid_ir(format!(
                "global '{}' has no initializer",
                global.name
            ))
        })?;

        match (&global.ty, initializer) {
            (IRType::Int, Constant::Int(value))
            | (IRType::ExactInt { .. }, Constant::Int(value)) => {
                let raw = value.to_le_bytes();
                bytes.copy_from_slice(&raw[..size.min(raw.len())]);
            }
            (IRType::Bool, Constant::Bool(value)) => {
                bytes[0] = u8::from(*value);
            }
            (IRType::Char, Constant::Char(value)) => {
                let raw = (*value as u32).to_le_bytes();
                bytes.copy_from_slice(&raw[..size.min(raw.len())]);
            }
            (IRType::Float, Constant::Float(value))
            | (IRType::ExactFloat { .. }, Constant::Float(value)) => {
                if matches!(global.ty, IRType::ExactFloat { width: spectra_midend::ir::FloatWidth::F32 }) {
                    let raw = (*value as f32).to_le_bytes();
                    bytes.copy_from_slice(&raw[..size.min(raw.len())]);
                } else {
                    let raw = value.to_le_bytes();
                    bytes.copy_from_slice(&raw[..size.min(raw.len())]);
                }
            }
            (_, Constant::String(_)) => {
                return Err(BackendCodegenError::invalid_ir(format!(
                    "global '{}' has a string initializer but string global storage is not part of the scalar static contract",
                    global.name
                )));
            }
            (ty, value) => {
                return Err(BackendCodegenError::invalid_ir(format!(
                    "global '{}' initializer {:?} does not match type {:?}",
                    global.name, value, ty
                )));
            }
        }

        Ok(bytes)
    }

    fn define_globals(&mut self, ir_module: &IRModule) -> BackendResult<()> {
        for global in &ir_module.globals {
            if self.global_data.contains_key(&global.name) {
                continue;
            }
            let symbol = Self::global_symbol(&ir_module.name, &global.name);
            let Some(_initializer) = global.initializer.as_ref() else {
                let data_id = self
                    .module
                    .declare_data(&symbol, Linkage::Import, true, false)
                    .map_err(|error| {
                        BackendCodegenError::cranelift(format!(
                            "failed to declare imported global '{}': {}",
                            global.name, error
                        ))
                    })?;
                self.global_data.insert(global.name.clone(), data_id);
                continue;
            };
            let data_id = self
                .module
                .declare_data(&symbol, Linkage::Local, true, false)
                .map_err(|error| {
                    BackendCodegenError::cranelift(format!(
                        "failed to declare global '{}': {}",
                        global.name, error
                    ))
            })?;
            let mut data = DataDescription::new();
            let bytes = Self::global_initializer_bytes(global)?;
            data.set_align(Self::type_size_bytes(&global.ty).clamp(1, 8) as u64);
            data.define(bytes.into_boxed_slice());
            self.module.define_data(data_id, &data).map_err(|error| {
                BackendCodegenError::cranelift(format!(
                    "failed to define global '{}': {}",
                    global.name, error
                ))
            })?;
            self.global_data.insert(global.name.clone(), data_id);
        }
        Ok(())
    }

    /// Pre-intern every host-call name once per module before lowering starts.
    /// Sorting keeps allocation order deterministic across equivalent IR
    /// modules and prevents the normal lowering path from allocating names.
    fn pre_intern_host_names(&mut self, ir_module: &IRModule) {
        let mut names = ir_module
            .functions
            .iter()
            .flat_map(|func| func.blocks.iter())
            .flat_map(|block| block.instructions.iter())
            .filter_map(|instr| match &instr.kind {
                InstructionKind::HostCall { host, .. } => Some(host.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>();
        names.sort_unstable();
        names.dedup();
        for name in names {
            intern_jit_host_call_site(
                &mut self.host_call_sites,
                &mut self.host_name_storage,
                &mut self.host_call_cache_storage,
                name,
            );
        }
    }

    #[cfg(test)]
    fn pre_intern_host_names_for_test(&mut self, ir_func: &IRFunction) {
        let mut module = IRModule::new("test_host_names");
        module.functions.push(ir_func.clone());
        self.pre_intern_host_names(&module);
    }

    /// Whether `ir_func` must be declared with `CallConv::Tail`.
    ///
    /// Functions containing a marked self-tail-call (`InstructionKind::Call`
    /// with `is_tail`) need it: Cranelift's verifier accepts a native
    /// `return_call` only when caller and callee share a calling convention
    /// that supports tail calls, and only `CallConv::Tail` does. Entry points
    /// and functions that need the manual allocation frame keep the platform
    /// default — a tail jump would skip the `frame_exit` cleanup those
    /// functions rely on, and external callers expect the native ABI.
    pub(crate) fn uses_tail_call_convention(ir_func: &IRFunction) -> bool {
        ir_func.name != "main"
            && ir_func.blocks.iter().any(|block| {
                block.instructions.iter().any(|instruction| {
                    matches!(
                        &instruction.kind,
                        InstructionKind::Call {
                            function: callee,
                            is_tail: true,
                            ..
                        } if *callee == ir_func.name
                    )
                })
            })
            && !Self::function_needs_manual_frame(
                ir_func,
                &Self::collect_stack_allocas(ir_func),
            )
    }

    /// Declare a function signature
    fn declare_function(&mut self, ir_func: &IRFunction) -> BackendResult<FuncId> {
        let mut sig = self.module.make_signature();
        if Self::uses_tail_call_convention(ir_func) {
            sig.call_conv = isa::CallConv::Tail;
        }

        // Add parameters
        for param in &ir_func.params {
            let cl_type = Self::ir_type_to_cranelift(&param.ty)?;
            sig.params.push(AbiParam::new(cl_type));
        }

        // Add return type
        let return_type = Self::ir_type_to_cranelift(&ir_func.return_type)?;
        if return_type != types::I8 || ir_func.return_type != IRType::Void {
            sig.returns.push(AbiParam::new(return_type));
        }

        // Declare function in module
        let func_id = self
            .module
            .declare_function(&ir_func.name, Linkage::Export, &sig)
            .map_err(|e| {
                BackendCodegenError::cranelift(format!(
                    "Failed to declare function '{}': {}",
                    ir_func.name, e
                ))
            })?;

        self.function_map.insert(ir_func.name.clone(), func_id);

        Ok(func_id)
    }

    /// Define a function body
    fn define_function(
        &mut self,
        ir_func: &IRFunction,
        function_params: &HashMap<String, Vec<IRType>>,
    ) -> BackendResult<()> {
        let func_id = *self
            .function_map
            .get(&ir_func.name)
            .ok_or_else(|| BackendCodegenError::missing_function(&ir_func.name))?;

        // Clear context
        self.ctx.func.clear();

        // Set function signature
        self.ctx.func.signature = self
            .module
            .declarations()
            .get_function_decl(func_id)
            .signature
            .clone();

        // Create function builder
        let mut builder = FunctionBuilder::new(&mut self.ctx.func, &mut self.builder_context);
        // Enable value-label tracking so Cranelift's post-allocation pass can
        // resolve compiler-proven live ranges for IR values. This mirrors the
        // AOT path (`aot.rs`) and feeds the JIT debug sidecar; it never alters
        // generated machine code.
        builder.func.collect_debug_info();

        // Create entry block
        let entry_block = builder.create_block();
        builder.append_block_params_for_function_params(entry_block);
        builder.switch_to_block(entry_block);
        builder.seal_block(entry_block);

        // Create value and block mappings
        let mut value_map = DenseValueMap::with_capacity(ir_func.next_value_id);
        let mut block_map: HashMap<usize, Block> = HashMap::new();
        let mut allocation_vars: Vec<Variable> = Vec::new();
        let mut stack_array_lengths: HashMap<usize, i64> = HashMap::new();
        let mut string_literal_lengths: HashMap<usize, i64> = HashMap::new();
        let stack_allocas = Self::collect_stack_allocas(ir_func);
        let scalar_alloca_types =
            Self::collect_promotable_scalar_allocas_with_stack_allocas(ir_func, &stack_allocas);
        let mut scalar_alloca_vars = HashMap::with_capacity(scalar_alloca_types.len());
        for (alloca_id, ty) in &scalar_alloca_types {
            let variable = builder.declare_var(Self::ir_type_to_cranelift(ty)?);
            scalar_alloca_vars.insert(*alloca_id, variable);
        }
        let manual_frame_active = Self::function_needs_manual_frame(ir_func, &stack_allocas);
        let frame_token = if manual_frame_active {
            let frame_enter_ref = self.module.declare_func_in_func(
                self.runtime_bindings.get(RuntimeImport::ManualFrameEnter),
                builder.func,
            );
            let frame_call = builder.ins().call(frame_enter_ref, &[]);
            builder.inst_results(frame_call)[0]
        } else {
            builder.ins().iconst(types::I64, 0)
        };
        // In cranelift 0.130+, declare_var(Type) -> Variable (no manual index tracking needed)
        let frame_var = builder.declare_var(types::I64);
        builder.def_var(frame_var, frame_token);
        // Map function parameters to Cranelift values
        let params = builder.block_params(entry_block).to_vec();
        for (param, &cl_value) in ir_func.params.iter().zip(params.iter()) {
            value_map.insert(param.id, cl_value);
        }

        // Create all basic blocks
        for ir_block in &ir_func.blocks {
            if ir_block.id == 0 {
                block_map.insert(0, entry_block);
            } else {
                let block = builder.create_block();
                block_map.insert(ir_block.id, block);
            }
        }

        // Collect PHI descriptors and add block parameters to Cranelift blocks.
        // Cranelift uses block parameters (not PHI nodes) for SSA values that
        // are defined by predecessor terminators.
        let mut phi_map: HashMap<usize, Vec<PhiDescriptor>> = HashMap::new();
        for ir_block in &ir_func.blocks {
            let mut phis = Vec::new();
            for instr in &ir_block.instructions {
                if let InstructionKind::Phi { result, incoming } = &instr.kind {
                    let mut incoming_map = HashMap::new();
                    for (val, pred_bb) in incoming {
                        incoming_map.insert(*pred_bb, val.id);
                    }
                    phis.push(PhiDescriptor {
                        result_id: result.id,
                        incoming: incoming_map,
                    });
                }
            }
            if !phis.is_empty() {
                phi_map.insert(ir_block.id, phis);
            }
        }

        // Add block parameters for PHI nodes to Cranelift blocks.
        for ir_block in &ir_func.blocks {
            if let Some(phis) = phi_map.get(&ir_block.id) {
                let block = *block_map
                    .get(&ir_block.id)
                    .ok_or_else(|| BackendCodegenError::missing_block(ir_block.id))?;
                for _ in phis {
                    builder.append_block_param(block, types::I64);
                }
            }
        }

        // Generate code for each block
        let blocks = ir_func.blocks.clone();
        let mut emitted_tail_call = false;
        let mut hostcall = HostCallLoweringContext {
            bindings: &self.runtime_bindings,
            host_call_sites: &self.host_call_sites,
            string_literal_data: &mut self.string_literal_data,
            string_literal_storage: &mut self.string_literal_storage,
            batch_stats: &mut self.hostcall_batch_stats,
            finalized_function_ptrs: Some(&self.finalized_function_ptrs),
        };
        for ir_block in &blocks {
            Self::generate_block(
                &mut self.module,
                &self.function_map,
                function_params,
                &mut hostcall,
                &mut builder,
                ir_block,
                &mut value_map,
                &block_map,
                &mut allocation_vars,
                &mut stack_array_lengths,
                &mut string_literal_lengths,
                &stack_allocas,
                &scalar_alloca_vars,
                &self.global_data,
                frame_var,
                manual_frame_active,
                ir_block.id,
                &phi_map,
                &mut emitted_tail_call,
            )?;
        }

        // Seal all blocks after generating code
        for ir_block in &ir_func.blocks {
            if ir_block.id != 0 {
                // Entry block already sealed
                if let Some(&block) = block_map.get(&ir_block.id) {
                    builder.seal_block(block);
                }
            }
        }

        // Attach Cranelift value labels before finalization so the register
        // allocator resolves them to real register or CFA-relative locations
        // in the compiled machine code — the same collection the AOT path
        // performs.
        for local in &ir_func.locals {
            if let Some(value_id) = local.value_id {
                if let Some(value) = value_map.get(value_id) {
                    builder.set_val_label(
                        value,
                        cranelift_codegen::ir::ValueLabel::from_u32(value_id as u32),
                    );
                }
            }
        }
        for ir_block in &ir_func.blocks {
            for instr in &ir_block.instructions {
                let Some(result) = crate::aot::instruction_result_value(&instr.kind) else {
                    continue;
                };
                if let Some(value) = value_map.get(result.id) {
                    builder.set_val_label(
                        value,
                        cranelift_codegen::ir::ValueLabel::from_u32(result.id as u32),
                    );
                }
            }
        }

        // Finalize function
        builder.finalize();

        // Define function in module
        self.module
            .define_function(func_id, &mut self.ctx)
            .map_err(|e| {
                BackendCodegenError::cranelift(format!(
                    "Failed to define function '{}': {e:?}",
                    ir_func.name
                ))
            })?;


        // Collect the sidecar data while this function's compiled code is
        // still available: name/type/line come from the IR local's debug
        // info and declaration span; ranges from the shared value-label
        // extraction (`aot::value_label_ranges`). Offsets are relative to the
        // start of the compiled function body.
        if let Some(compiled) = self.ctx.compiled_code() {
            let mut variables = Vec::new();
            for local in &ir_func.locals {
                let Some(value_id) = local.value_id else {
                    continue;
                };
                let ranges =
                    crate::aot::value_label_ranges(&compiled.value_labels_ranges, value_id);
                if ranges.is_empty() {
                    continue;
                }
                variables.push(crate::debug::JitDebugVariable {
                    name: local.name.clone(),
                    type_name: crate::debug::ir_type_debug_name(&local.ty),
                    line: local.declaration.as_ref().map(|span| span.start_line),
                    ranges,
                });
            }
            if !variables.is_empty() {
                self.jit_debug_functions.push((ir_func.name.clone(), variables));
            }
        }
        // Test-only snapshot: keep the finalized Cranelift IR inspectable
        // before the shared context is released.
        #[cfg(test)]
        {
            self.last_finalized_func = Some(self.ctx.func.clone());
        }
        // Clear context
        self.module.clear_context(&mut self.ctx);

        Ok(())
    }

}
