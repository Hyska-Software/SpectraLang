// IR Builder - constructs IR from AST

use crate::ir::{Function, InstructionKind, SourceSpan, Terminator, Type, Value};

/// Builder for constructing IR
pub struct IRBuilder {
    current_function: Option<usize>,
    current_block: Option<usize>,
    current_span: Option<SourceSpan>,
}

impl IRBuilder {
    pub fn new() -> Self {
        Self {
            current_function: None,
            current_block: None,
            current_span: None,
        }
    }

    pub fn set_current_function(&mut self, func_idx: usize) {
        self.current_function = Some(func_idx);
    }

    pub fn set_current_block(&mut self, block_id: usize) {
        self.current_block = Some(block_id);
    }

    pub fn set_source_span(&mut self, span: Option<SourceSpan>) {
        self.current_span = span;
    }

    pub fn get_current_block(&self) -> Option<usize> {
        self.current_block
    }

    // -----------------------------------------------------------------------
    // Private helper: allocate a value ID and emit an instruction into the
    // current block only when both the block handle and the block itself exist.
    // This prevents orphaned value IDs when callers build instructions without
    // an active block.
    // -----------------------------------------------------------------------
    fn try_emit<F>(&self, func: &mut Function, make_kind: F) -> Value
    where
        F: FnOnce(Value) -> InstructionKind,
    {
        let Some(block_id) = self.current_block else {
            // No active block: return a sentinel instead of wasting a value ID.
            return Value { id: usize::MAX };
        };
        let Some(pos) = func.blocks.iter().position(|b| b.id == block_id) else {
            return Value { id: usize::MAX };
        };
        let result = func.next_value();
        let kind = make_kind(result);
        func.blocks[pos].add_instruction(kind);
        if let Some(instruction) = func.blocks[pos].instructions.last_mut() {
            instruction.source_span = self.current_span.clone();
        }
        result
    }

    pub fn build_add(&self, func: &mut Function, lhs: Value, rhs: Value) -> Value {
        self.try_emit(func, |result| InstructionKind::Add { result, lhs, rhs })
    }

    pub fn build_sub(&self, func: &mut Function, lhs: Value, rhs: Value) -> Value {
        self.try_emit(func, |result| InstructionKind::Sub { result, lhs, rhs })
    }

    pub fn build_mul(&self, func: &mut Function, lhs: Value, rhs: Value) -> Value {
        self.try_emit(func, |result| InstructionKind::Mul { result, lhs, rhs })
    }

    pub fn build_div(&self, func: &mut Function, lhs: Value, rhs: Value) -> Value {
        self.try_emit(func, |result| InstructionKind::Div { result, lhs, rhs })
    }

    pub fn build_rem(&self, func: &mut Function, lhs: Value, rhs: Value) -> Value {
        self.try_emit(func, |result| InstructionKind::Rem { result, lhs, rhs })
    }

    pub fn build_eq(&self, func: &mut Function, lhs: Value, rhs: Value) -> Value {
        self.try_emit(func, |result| InstructionKind::Eq { result, lhs, rhs })
    }

    pub fn build_ne(&self, func: &mut Function, lhs: Value, rhs: Value) -> Value {
        self.try_emit(func, |result| InstructionKind::Ne { result, lhs, rhs })
    }

    pub fn build_lt(&self, func: &mut Function, lhs: Value, rhs: Value) -> Value {
        self.try_emit(func, |result| InstructionKind::Lt { result, lhs, rhs })
    }

    pub fn build_le(&self, func: &mut Function, lhs: Value, rhs: Value) -> Value {
        self.try_emit(func, |result| InstructionKind::Le { result, lhs, rhs })
    }

    pub fn build_gt(&self, func: &mut Function, lhs: Value, rhs: Value) -> Value {
        self.try_emit(func, |result| InstructionKind::Gt { result, lhs, rhs })
    }

    pub fn build_ge(&self, func: &mut Function, lhs: Value, rhs: Value) -> Value {
        self.try_emit(func, |result| InstructionKind::Ge { result, lhs, rhs })
    }

    pub fn build_and(&self, func: &mut Function, lhs: Value, rhs: Value) -> Value {
        self.try_emit(func, |result| InstructionKind::And { result, lhs, rhs })
    }

    pub fn build_or(&self, func: &mut Function, lhs: Value, rhs: Value) -> Value {
        self.try_emit(func, |result| InstructionKind::Or { result, lhs, rhs })
    }

    pub fn build_not(&self, func: &mut Function, operand: Value) -> Value {
        self.try_emit(func, |result| InstructionKind::Not { result, operand })
    }

    pub fn build_alloca(&self, func: &mut Function, ty: crate::ir::Type) -> Value {
        self.try_emit(func, |result| InstructionKind::Alloca { result, ty })
    }

    pub fn build_global_addr(
        &self,
        func: &mut Function,
        name: String,
        ty: crate::ir::Type,
    ) -> Value {
        self.try_emit(func, |result| InstructionKind::GlobalAddr {
            result,
            name,
            ty,
        })
    }

    pub fn build_load(&self, func: &mut Function, ptr: Value) -> Value {
        self.try_emit(func, |result| InstructionKind::Load {
            result,
            ptr,
            ty: crate::ir::Type::Int,
        })
    }

    pub fn build_load_typed(&self, func: &mut Function, ptr: Value, ty: crate::ir::Type) -> Value {
        self.try_emit(func, |result| InstructionKind::Load { result, ptr, ty })
    }

    pub fn build_store(&self, func: &mut Function, ptr: Value, value: Value) {
        if let Some(block_id) = self.current_block {
            if let Some(block) = func.get_block_mut(block_id) {
                block.add_instruction(InstructionKind::Store { ptr, value });
                if let Some(instruction) = block.instructions.last_mut() {
                    instruction.source_span = self.current_span.clone();
                }
            }
        }
    }

    pub fn build_getelementptr(
        &self,
        func: &mut Function,
        ptr: Value,
        index: Value,
        element_type: crate::ir::Type,
    ) -> Value {
        self.try_emit(func, |result| InstructionKind::GetElementPtr {
            result,
            ptr,
            index,
            element_type,
        })
    }

    pub fn build_field_ptr(&self, func: &mut Function, ptr: Value, offset: i64) -> Value {
        self.try_emit(func, |result| InstructionKind::FieldPtr {
            result,
            ptr,
            offset,
        })
    }

    pub fn build_manual_alloc(&self, func: &mut Function, size: i64) -> Value {
        self.try_emit(func, |result| InstructionKind::ManualAlloc { result, size })
    }

    pub fn build_escape_manual_alloc(&self, func: &mut Function, ptr: Value) {
        if let Some(block_id) = self.current_block {
            if let Some(block) = func.get_block_mut(block_id) {
                block.add_instruction(InstructionKind::EscapeManualAlloc { ptr });
            }
        }
    }

    pub fn build_copy(&self, func: &mut Function, source: Value) -> Value {
        self.try_emit(func, |result| InstructionKind::Copy { result, source })
    }

    pub fn build_phi(&self, func: &mut Function, incoming: Vec<(Value, usize)>) -> Value {
        self.try_emit(func, |result| InstructionKind::Phi { result, incoming })
    }

    pub fn build_const_int(&self, func: &mut Function, value: i64) -> Value {
        self.try_emit(func, |result| InstructionKind::ConstInt { result, value })
    }

    pub fn build_const_int_typed(
        &self,
        func: &mut Function,
        value: i64,
        ty: crate::ir::Type,
    ) -> Value {
        self.try_emit(func, |result| InstructionKind::ConstIntTyped {
            result,
            value,
            ty,
        })
    }

    pub fn build_const_float(&self, func: &mut Function, value: f64) -> Value {
        self.try_emit(func, |result| InstructionKind::ConstFloat { result, value })
    }

    pub fn build_const_float_typed(
        &self,
        func: &mut Function,
        value: f64,
        ty: crate::ir::Type,
    ) -> Value {
        self.try_emit(func, |result| InstructionKind::ConstFloatTyped {
            result,
            value,
            ty,
        })
    }

    pub fn build_const_bool(&self, func: &mut Function, value: bool) -> Value {
        self.try_emit(func, |result| InstructionKind::ConstBool { result, value })
    }

    pub fn build_const_string(&self, func: &mut Function, value: String) -> Value {
        self.try_emit(func, |result| InstructionKind::ConstString {
            result,
            value,
        })
    }

    pub fn build_return(&self, func: &mut Function, value: Option<Value>) {
        if let Some(block_id) = self.current_block {
            if let Some(block) = func.get_block_mut(block_id) {
                block.set_terminator(Terminator::Return { value });
            }
        }
    }

    pub fn build_branch(&self, func: &mut Function, target: usize) {
        if let Some(block_id) = self.current_block {
            if let Some(block) = func.get_block_mut(block_id) {
                block.set_terminator(Terminator::Branch { target });
            }
        }
    }

    pub fn build_cond_branch(
        &self,
        func: &mut Function,
        condition: Value,
        true_block: usize,
        false_block: usize,
    ) {
        if let Some(block_id) = self.current_block {
            if let Some(block) = func.get_block_mut(block_id) {
                block.set_terminator(Terminator::CondBranch {
                    condition,
                    true_block,
                    false_block,
                });
            }
        }
    }

    pub fn build_unreachable(&self, func: &mut Function) {
        if let Some(block_id) = self.current_block {
            if let Some(block) = func.get_block_mut(block_id) {
                block.set_terminator(Terminator::Unreachable);
            }
        }
    }

    pub fn build_call(
        &self,
        func: &mut Function,
        function_name: String,
        args: Vec<Value>,
        has_return: bool,
    ) -> Option<Value> {
        let block_id = self.current_block?;
        let pos = func.blocks.iter().position(|b| b.id == block_id)?;
        let result = if has_return {
            Some(func.next_value())
        } else {
            None
        };
        func.blocks[pos].add_instruction(InstructionKind::Call {
            result,
            function: function_name,
            args,
            is_tail: false,
        });
        result
    }

    pub fn build_host_call(
        &self,
        func: &mut Function,
        host_name: String,
        args: Vec<Value>,
        has_return: bool,
    ) -> Option<Value> {
        self.build_host_call_with_type(func, host_name, args, has_return, None)
    }

    pub fn build_typed_host_call(
        &self,
        func: &mut Function,
        host_name: String,
        args: Vec<Value>,
        return_type: Type,
        has_return: bool,
    ) -> Option<Value> {
        self.build_host_call_with_type(func, host_name, args, has_return, Some(return_type))
    }

    fn build_host_call_with_type(
        &self,
        func: &mut Function,
        host_name: String,
        args: Vec<Value>,
        has_return: bool,
        result_type: Option<Type>,
    ) -> Option<Value> {
        let block_id = self.current_block?;
        let pos = func.blocks.iter().position(|b| b.id == block_id)?;
        let result = if has_return {
            Some(func.next_value())
        } else {
            None
        };
        func.blocks[pos].add_instruction(InstructionKind::HostCall {
            result,
            host: host_name,
            args,
            result_type,
        });
        result
    }

    /// Emit a `FuncAddr` instruction: returns the address of `function` as an i64.
    pub fn build_func_addr(&self, func: &mut Function, function: String) -> Value {
        self.try_emit(func, |result| InstructionKind::FuncAddr {
            result,
            function,
        })
    }

    /// Emit a `CallIndirect` instruction: call through a function pointer.
    /// Returns the result value when `sig_return` is not `Type::Void`.
    pub fn build_call_indirect(
        &self,
        func: &mut Function,
        fn_ptr: Value,
        args: Vec<Value>,
        sig_params: Vec<crate::ir::Type>,
        sig_return: crate::ir::Type,
    ) -> Option<Value> {
        let block_id = self.current_block?;
        let pos = func.blocks.iter().position(|b| b.id == block_id)?;
        let has_return = sig_return != crate::ir::Type::Void;
        let result = if has_return {
            Some(func.next_value())
        } else {
            None
        };
        func.blocks[pos].add_instruction(InstructionKind::CallIndirect {
            result,
            fn_ptr,
            args,
            signature_params: sig_params,
            signature_return: Box::new(sig_return),
        });
        result
    }

    pub fn build_async_suspend(&self, func: &mut Function, task: Value, state: usize) {
        if let Some(block_id) = self.current_block {
            if let Some(block) = func.get_block_mut(block_id) {
                block.add_instruction(InstructionKind::AsyncSuspend { task, state });
            }
        }
    }

    pub fn build_async_resume(&self, func: &mut Function, task: Value, state: usize) {
        if let Some(block_id) = self.current_block {
            if let Some(block) = func.get_block_mut(block_id) {
                block.add_instruction(InstructionKind::AsyncResume { task, state });
            }
        }
    }

    pub fn build_async_ready(
        &self,
        func: &mut Function,
        value: Option<Value>,
        output_type: Type,
    ) -> Value {
        self.try_emit(func, |result| InstructionKind::AsyncReady {
            result,
            value,
            output_type,
        })
    }

    /// Emit a `Cast` instruction: convert between numeric types or int↔char.
    pub fn build_cast(
        &self,
        func: &mut Function,
        operand: Value,
        from_ty: Type,
        to_ty: Type,
    ) -> Value {
        self.try_emit(func, |result| InstructionKind::Cast {
            result,
            operand,
            from_ty,
            to_ty,
        })
    }

    /// Build a fat pointer for `dyn Trait` from (data_ptr, vtable_ptr).
    pub fn build_make_dyn_fat_ptr(
        &self,
        func: &mut Function,
        data_ptr: Value,
        vtable_ptr: Value,
    ) -> Value {
        self.try_emit(func, |result| InstructionKind::MakeDynFatPtr {
            result,
            data_ptr,
            vtable_ptr,
        })
    }

    /// Load the data pointer out of a fat pointer.
    pub fn build_load_dyn_data_ptr(&self, func: &mut Function, fat_ptr: Value) -> Value {
        self.try_emit(func, |result| InstructionKind::LoadDynDataPtr {
            result,
            fat_ptr,
        })
    }

    /// Load the vtable pointer out of a fat pointer.
    pub fn build_load_dyn_vtable_ptr(&self, func: &mut Function, fat_ptr: Value) -> Value {
        self.try_emit(func, |result| InstructionKind::LoadDynVtablePtr {
            result,
            fat_ptr,
        })
    }

    /// Load a function pointer from a vtable at the given slot index.
    pub fn build_load_vtable_slot(
        &self,
        func: &mut Function,
        vtable_ptr: Value,
        slot_index: usize,
    ) -> Value {
        self.try_emit(func, |result| InstructionKind::LoadVtableSlot {
            result,
            vtable_ptr,
            slot_index,
        })
    }
}

impl Default for IRBuilder {
    fn default() -> Self {
        Self::new()
    }
}
