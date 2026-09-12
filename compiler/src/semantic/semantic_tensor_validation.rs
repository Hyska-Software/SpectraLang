use super::*;

impl SemanticAnalyzer {
    pub(crate) fn types_compatible(&self, a: &Type, b: &Type) -> bool {
        self.types_match(a, b) && self.types_match(b, a)
    }

    pub(crate) fn tensor_dims_match(
        a: &Option<Vec<Option<usize>>>,
        b: &Option<Vec<Option<usize>>>,
    ) -> bool {
        let (Some(a), Some(b)) = (a, b) else {
            return true;
        };
        a.len() == b.len()
            && a.iter()
                .zip(b.iter())
                .all(|(left, right)| left.is_none() || right.is_none() || left == right)
    }

    pub(crate) fn tensor_mismatch_diagnostic(
        actual: &Type,
        expected: &Type,
    ) -> Option<(&'static str, String, &'static str)> {
        let (
            Type::Tensor {
                dtype: actual_dtype,
                rank: actual_rank,
                dims: actual_dims,
                layout: actual_layout,
                device: actual_device,
            },
            Type::Tensor {
                dtype: expected_dtype,
                rank: expected_rank,
                dims: expected_dims,
                layout: expected_layout,
                device: expected_device,
            },
        ) = (actual, expected)
        else {
            return None;
        };

        if actual_dtype != expected_dtype {
            return Some((
                "E1402",
                format!(
                    "Tensor dtype mismatch: expected {}, found {}",
                    type_name(expected),
                    type_name(actual)
                ),
                "Use a tensor constructor or literal with the declared dtype.",
            ));
        }

        if actual_rank.is_some() && expected_rank.is_some() && actual_rank != expected_rank {
            return Some((
                "E1401",
                format!(
                    "Tensor rank mismatch: expected {}, found {}",
                    type_name(expected),
                    type_name(actual)
                ),
                "Change the declared rank or produce a tensor with the expected rank.",
            ));
        }

        if !Self::tensor_dims_match(actual_dims, expected_dims) {
            return Some((
                "E1403",
                format!(
                    "Tensor shape mismatch: expected {}, found {}",
                    type_name(expected),
                    type_name(actual)
                ),
                "Change the tensor dimensions or use dynamic dimensions with `dynamic_dim`.",
            ));
        }

        if actual_layout.is_some() && expected_layout.is_some() && actual_layout != expected_layout
        {
            return Some((
                "E1404",
                format!(
                    "Tensor layout mismatch: expected {}, found {}",
                    type_name(expected),
                    type_name(actual)
                ),
                "Convert the tensor layout or change the declared layout annotation.",
            ));
        }

        if actual_device.is_some() && expected_device.is_some() && actual_device != expected_device
        {
            return Some((
                "E1405",
                format!(
                    "Tensor device mismatch: expected {}, found {}",
                    type_name(expected),
                    type_name(actual)
                ),
                "Move the tensor to the declared device or change the device annotation.",
            ));
        }

        None
    }

    pub(crate) fn tensor_literal_matches(&mut self, value: &Expression, expected: &Type) -> bool {
        let Type::Tensor {
            dtype, rank, dims, ..
        } = expected
        else {
            return false;
        };
        let ExpressionKind::ArrayLiteral { elements } = &value.kind else {
            return false;
        };

        match rank {
            Some(1) => {
                if let Some(Some(expected_len)) =
                    dims.as_ref().and_then(|dims| dims.first()).copied()
                {
                    if elements.len() != expected_len {
                        return false;
                    }
                }
                elements.iter().all(|element| {
                    let actual = self.infer_expression_type(element);
                    self.types_match(&actual, dtype)
                })
            }
            Some(2) => {
                let Some((first, rest)) = elements.split_first() else {
                    return true;
                };
                let ExpressionKind::ArrayLiteral {
                    elements: first_row,
                } = &first.kind
                else {
                    return false;
                };
                let width = first_row.len();
                if let Some(dims) = dims {
                    if let Some(Some(expected_rows)) = dims.first().copied() {
                        if elements.len() != expected_rows {
                            return false;
                        }
                    }
                    if let Some(Some(expected_cols)) = dims.get(1).copied() {
                        if width != expected_cols {
                            return false;
                        }
                    }
                }
                first_row.iter().all(|element| {
                    let actual = self.infer_expression_type(element);
                    self.types_match(&actual, dtype)
                }) && rest.iter().all(|row| {
                    let ExpressionKind::ArrayLiteral { elements: row } = &row.kind else {
                        return false;
                    };
                    row.len() == width
                        && row.iter().all(|element| {
                            let actual = self.infer_expression_type(element);
                            self.types_match(&actual, dtype)
                        })
                })
            }
            None => true,
            _ => false,
        }
    }

    pub(crate) fn validate_static_tensor_call(
        &mut self,
        callee: &Expression,
        arguments: &[Expression],
        span: Span,
    ) {
        let Some(name) = Self::tensor_call_name(callee) else {
            return;
        };
        self.validate_static_tensor_operation(&name, arguments, span);
    }

    pub(crate) fn validate_static_tensor_method_call(
        &mut self,
        object: &Expression,
        method_name: &str,
        arguments: &[Expression],
        span: Span,
    ) {
        let Some(path) = namespace_path(object) else {
            return;
        };
        if matches!(
            path.as_str(),
            "tensor" | "std.tensor" | "spectra.std.tensor"
        ) {
            self.validate_static_tensor_operation(method_name, arguments, span);
        }
    }

    pub(crate) fn validate_static_ml_method_call(
        &mut self,
        object: &Expression,
        method_name: &str,
        arguments: &[Expression],
        span: Span,
    ) {
        let Some(path) = namespace_path(object) else {
            return;
        };
        if !matches!(path.as_str(), "ml" | "std.ml" | "spectra.std.ml") {
            return;
        }
        if method_name != "linear" || arguments.len() != 3 {
            return;
        }

        let input = self.infer_expression_type(&arguments[0]);
        let weight = self.infer_expression_type(&arguments[1]);
        let bias = self.infer_expression_type(&arguments[2]);
        let (Some(input_dims), Some(weight_dims), Some(bias_dims)) = (
            Self::tensor_dims(&input),
            Self::tensor_dims(&weight),
            Self::tensor_dims(&bias),
        ) else {
            return;
        };
        if input_dims.len() != 2 || weight_dims.len() != 2 || bias_dims.len() != 1 {
            return;
        }

        let in_features = input_dims[1];
        let weight_in = weight_dims[0];
        let out_features = weight_dims[1];
        let bias_out = bias_dims[0];

        let inner_mismatch =
            in_features.is_some() && weight_in.is_some() && in_features != weight_in;
        let bias_mismatch =
            out_features.is_some() && bias_out.is_some() && out_features != bias_out;
        if inner_mismatch || bias_mismatch {
            self.error_coded_with_hint(
                "E1403",
                format!(
                    "Tensor shape mismatch in ml.linear: input is {}, weight is {}, bias is {}",
                    type_name(&input),
                    type_name(&weight),
                    type_name(&bias)
                ),
                span,
                "For ml.linear, input shape must be [batch, in], weight [in, out], and bias [out].",
            );
        }
    }

    fn validate_static_tensor_operation(
        &mut self,
        name: &str,
        arguments: &[Expression],
        span: Span,
    ) {
        match name {
            "add" | "sub" | "mul" | "div" => {
                if arguments.len() != 2 {
                    return;
                }
                let left = self.infer_expression_type(&arguments[0]);
                let right = self.infer_expression_type(&arguments[1]);
                let (Some(left_dims), Some(right_dims)) =
                    (Self::tensor_dims(&left), Self::tensor_dims(&right))
                else {
                    return;
                };
                if !Self::tensor_dim_slices_match(left_dims, right_dims) {
                    self.error_coded_with_hint(
                        "E1403",
                        format!(
                            "Tensor shape mismatch in tensor.{}: left is {}, right is {}",
                            name,
                            type_name(&left),
                            type_name(&right)
                        ),
                        span,
                        "Use tensors with matching static dimensions or mark unknown dimensions as `dynamic_dim`.",
                    );
                }
            }
            "matmul" => {
                if arguments.len() != 2 {
                    return;
                }
                let left = self.infer_expression_type(&arguments[0]);
                let right = self.infer_expression_type(&arguments[1]);
                let (Some(left_dims), Some(right_dims)) =
                    (Self::tensor_dims(&left), Self::tensor_dims(&right))
                else {
                    return;
                };
                if left_dims.len() != 2 || right_dims.len() != 2 {
                    return;
                }
                let inner_left = left_dims[1];
                let inner_right = right_dims[0];
                if inner_left.is_some() && inner_right.is_some() && inner_left != inner_right {
                    self.error_coded_with_hint(
                        "E1403",
                        format!(
                            "Tensor shape mismatch in tensor.matmul: left is {}, right is {}",
                            type_name(&left),
                            type_name(&right)
                        ),
                        span,
                        "For matmul, left columns must match right rows.",
                    );
                }
            }
            "reshape" => {
                if arguments.len() != 3 {
                    return;
                }
                let source = self.infer_expression_type(&arguments[0]);
                let Some(source_dims) = Self::tensor_dims(&source) else {
                    return;
                };
                let Some(source_len) = Self::known_element_count(source_dims) else {
                    return;
                };
                let Some(rows) = Self::const_int_expression(&arguments[1]) else {
                    return;
                };
                let Some(cols) = Self::const_int_expression(&arguments[2]) else {
                    return;
                };
                if rows <= 0 || cols <= 0 {
                    return;
                }
                let target_len = (rows as usize).saturating_mul(cols as usize);
                if source_len != target_len {
                    self.error_coded_with_hint(
                        "E1403",
                        format!(
                            "Tensor shape mismatch in tensor.reshape: source {} has {} element(s), target shape has {}",
                            type_name(&source),
                            source_len,
                            target_len
                        ),
                        span,
                        "Choose reshape dimensions whose product equals the source tensor element count.",
                    );
                }
            }
            _ => {}
        }
    }
}
