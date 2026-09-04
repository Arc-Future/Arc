//! Type-checking for struct types — module-isolated from class checking.
//!
//! Mirrors `check_class.rs` but simplified: no virtual dispatch, no inheritance,
//! no abstract members. Enforces `readonly struct` constraints on method bodies.

use super::*;
use crate::field_keyword::rewrite_field_block;
use crate::generics::type_id_to_field_name;
use crate::typed::FnLinkage;
use crate::{method_link_name, method_link_name_static_abi, OopMethodSig, ParamSig};
use ast::{MethodModifier, TypeId};

impl TypeChecker {
    /// Type-check all struct members (methods, constructors, properties).
    /// Called from `check_module_items` after `collect_struct_attributes`.
    pub(crate) fn check_struct(&mut self, def: &ast::StructDef) -> Result<(), TypeError> {
        self.validate_where_clause(&def.generics, &def.where_clause)?;
        for p in &def.generics {
            if p.variance != Variance::Invariant {
                return Err(TypeError::VarianceNotOnInterface);
            }
        }
        // RFC 006：record struct 等显式/合成的泛型接口（IEquatable/IHashable）
        self.check_generic_interface_impls_named(&def.name, &def.bases)?;
        self.current_class = Some(def.name.clone());
        self.current_readonly_context = def.is_readonly;

        // Check constructors
        for ctor in &def.constructors {
            self.check_struct_constructor(ctor, def)?;
        }

        // Check methods
        for method in &def.methods {
            if matches!(
                method.node.sig.modifier,
                MethodModifier::Virtual | MethodModifier::Override | MethodModifier::Abstract
            ) {
                return Err(TypeError::Oop(format!(
                    "struct `{}` method `{}` cannot be virtual/override/abstract",
                    def.name, method.node.sig.name
                )));
            }
            self.check_struct_method(method.node.clone(), def)?;
        }

        // Check properties
        for prop in &def.properties {
            if prop.is_static_abstract {
                continue;
            }
            self.check_struct_property(prop, def)?;
        }

        self.current_readonly_context = false;
        self.current_class = None;
        Ok(())
    }

    fn check_struct_constructor(
        &mut self,
        ctor: &ast::Spanned<ast::ConstructorDef>,
        def: &ast::StructDef,
    ) -> Result<(), TypeError> {
        self.validate_params_m2b(&ctor.node.params)?;
        self.scopes.push(IndexMap::new());
        self.scopes.last_mut().unwrap().insert(
            "this".into(),
            TypeId::Ref {
                inner: Box::new(TypeId::Named(def.name.clone())),
                mutable: true,
                kind: ast::RefKind::Value,
            },
        );

        let mut ctor_params: Vec<(Ident, TypeId)> = vec![(
            "this".into(),
            TypeId::Ref {
                inner: Box::new(TypeId::Named(def.name.clone())),
                mutable: true,
                kind: ast::RefKind::Value,
            },
        )];
        for param in &ctor.node.params {
            let param_ty = self.lower_type(&param.ty.node)?;
            self.scopes
                .last_mut()
                .unwrap()
                .insert(param.name.clone(), param_ty.clone());
            ctor_params.push((param.name.clone(), param_ty));
        }

        // Constructor body is NOT readonly constrained (ctor can write to fields)
        let was_readonly = self.current_readonly_context;
        self.current_readonly_context = false;

        let was_ctor = self.in_ctor;
        self.in_ctor = true;

        let prev_fn_static = self.current_fn_is_static;
        self.current_fn_is_static = false;

        self.return_slot.push(TypeId::Void);
        let typed_body = self.check_block(&ctor.node.body, &TypeId::Void)?;
        self.return_slot.pop();

        self.current_fn_is_static = prev_fn_static;
        self.in_ctor = was_ctor;
        self.current_readonly_context = was_readonly;

        // Push typed constructor to emit list
        // ctor 重载 mangle：与 check_class 一致，无参用 `__ctor::Struct`；
        // 有参用 `__ctor::Struct_<arity>` 避免重复符号。
        let ctor_arity = ctor_params.len().saturating_sub(1); // 减去 this
        let ctor_name: Ident = if ctor_arity == 0 {
            format!("__ctor::{}", def.name).into()
        } else {
            format!("__ctor::{}_{}", def.name, ctor_arity).into()
        };
        self.push_typed_fn(
            ctor_name,
            Some(def.name.clone()),
            true,
            ctor_params,
            TypeId::Void,
            Some(ctor.node.body.clone()),
            Some(typed_body),
            false,
            FnLinkage::User,
            false,
            // RFC 009 M3：构造函数不支持 `[Parallelize]` 属性，恒为 false。
            false,
        );

        self.scopes.pop();
        Ok(())
    }

    fn check_struct_method(
        &mut self,
        method: ast::MethodDef,
        def: &ast::StructDef,
    ) -> Result<(), TypeError> {
        self.scopes.push(IndexMap::new());

        let is_static = matches!(method.sig.modifier, MethodModifier::Static);
        if method.sig.is_async
            && method
                .sig
                .params
                .iter()
                .any(|p| p.is_ref || p.is_out || p.is_in)
        {
            self.scopes.pop();
            return Err(TypeError::Oop(
                "ref/out/in parameters are not allowed in async methods".into(),
            ));
        }
        self.validate_params_m2b(&method.sig.params)?;

        let mut method_params: Vec<(Ident, TypeId)> = Vec::new();
        if !is_static {
            let this_ty = TypeId::Ref {
                inner: Box::new(TypeId::Named(def.name.clone())),
                mutable: !def.is_readonly,
                kind: ast::RefKind::Value,
            };
            self.scopes
                .last_mut()
                .unwrap()
                .insert("this".into(), this_ty.clone());
            method_params.push(("this".into(), this_ty));
        }

        // RFC 009 P1-F：与 class / 顶层函数同源——`ref`/`out`/`in` 升为
        // `TypeId::Ref`，使 MIR/codegen 走指针 ABI（调用点 AddrOf ↔ 被调方
        // 间接 store）。此前 struct 方法只 lower 裸类型，导致手写
        // `Deconstruct(out T)` 与合成 Deconstruct 静默写错调用方槽。
        for param in &method.sig.params {
            let param_ty = self.lower_type(&param.ty.node)?;
            let final_ty = if param.is_in {
                TypeId::Ref {
                    inner: Box::new(param_ty),
                    mutable: false,
                    kind: ast::RefKind::Var,
                }
            } else if param.is_ref || param.is_out {
                TypeId::Ref {
                    inner: Box::new(param_ty),
                    mutable: true,
                    kind: ast::RefKind::Var,
                }
            } else {
                param_ty
            };
            self.scopes
                .last_mut()
                .unwrap()
                .insert(param.name.clone(), final_ty.clone());
            method_params.push((param.name.clone(), final_ty));
        }

        let (typed_body, body_ret) = if let Some(body) = &method.body {
            let ret = method
                .sig
                .ret
                .as_ref()
                .map(|r| self.lower_type(&r.node))
                .transpose()?
                .unwrap_or(TypeId::Void);
            self.return_slot.push(ret.clone());
            let out_params: IndexSet<Ident> = method
                .sig
                .params
                .iter()
                .filter(|p| p.is_out)
                .map(|p| p.name.clone())
                .collect();
            let prev_flow = self.out_flow.take();
            self.out_flow = if out_params.is_empty() {
                None
            } else {
                Some(OutParamState::new(out_params))
            };
            // RFC 006 M2：进入方法体前设置 current_fn_is_static，
            // check_expr_inner 据此拦截静态方法内访问实例字段。
            let prev_fn_static = self.current_fn_is_static;
            self.current_fn_is_static = is_static;
            let tb_result = self.check_block(body, &ret);
            self.current_fn_is_static = prev_fn_static;
            if let Some(flow) = &self.out_flow {
                let missing = flow.unassigned();
                if !missing.is_empty() {
                    self.out_flow = prev_flow;
                    self.return_slot.pop();
                    self.scopes.pop();
                    return Err(TypeError::Oop(format!(
                        "out parameter `{}` must be assigned before control leaves the current method",
                        missing[0]
                    )));
                }
            }
            self.out_flow = prev_flow;
            let tb = tb_result?;
            self.return_slot.pop();
            (Some(tb), ret)
        } else {
            let ret = method
                .sig
                .ret
                .as_ref()
                .map(|r| self.lower_type(&r.node))
                .transpose()?
                .unwrap_or(TypeId::Void);
            (None, ret)
        };

        // Push typed method to emit list.
        // RFC 006：static/instance 同名（Equals/GetHashCode）时保留 static 的
        // Dictionary ABI（`Struct::Equals`），instance 按 arity 后缀消歧。
        let mut oop_params: Vec<ParamSig> = Vec::new();
        for param in &method.sig.params {
            let param_ty = self.lower_type(&param.ty.node)?;
            oop_params.push(ParamSig {
                name: param.name.clone(),
                ty: type_id_to_field_name(&param_ty),
                is_ref: param.is_ref,
                is_out: param.is_out,
                is_in: param.is_in,
                is_params: param.is_params,
                default: param
                    .default
                    .as_ref()
                    .and_then(|e| self.fold_param_default_expr(&e.node)),
            });
        }
        let oop_sig = OopMethodSig {
            name: method.sig.name.clone(),
            vis: method.sig.vis,
            params: oop_params,
            ret: type_id_to_field_name(&body_ret),
            modifier: method.sig.modifier,
            is_async: method.sig.is_async,
            generics: method.sig.generics.iter().map(|g| g.name.clone()).collect(),
            is_static_abstract: method.sig.is_static_abstract,
        };
        let static_count = def
            .methods
            .iter()
            .filter(|other| {
                other.node.sig.name == method.sig.name
                    && matches!(other.node.sig.modifier, MethodModifier::Static)
            })
            .count()
            .max(
                self.registry
                    .method_overload_count_kind(&def.name, &method.sig.name, true),
            );
        let instance_count = def
            .methods
            .iter()
            .filter(|other| {
                other.node.sig.name == method.sig.name
                    && !matches!(other.node.sig.modifier, MethodModifier::Static)
            })
            .count()
            .max(
                self.registry
                    .method_overload_count_kind(&def.name, &method.sig.name, false),
            );
        let method_name: Ident = if static_count > 0 && instance_count > 0 {
            method_link_name_static_abi(def.name.as_str(), &oop_sig, static_count, instance_count)
                .into()
        } else {
            let overload_count = def
                .methods
                .iter()
                .filter(|other| other.node.sig.name == method.sig.name)
                .count()
                .max(
                    self.registry
                        .method_overload_count(&def.name, &method.sig.name),
                );
            method_link_name(def.name.as_str(), &oop_sig, overload_count).into()
        };
        self.push_typed_fn(
            method_name,
            Some(def.name.clone()),
            false,
            method_params,
            body_ret,
            method.body.clone(),
            typed_body,
            false,
            FnLinkage::User,
            is_static,
            // RFC 009 M3：检测 `[Parallelize]` 属性，标记向量化候选。
            Self::has_parallelize_attr(&method.sig.attributes),
        );

        self.scopes.pop();
        Ok(())
    }

    fn check_struct_property(
        &mut self,
        prop: &ast::PropertyDef,
        def: &ast::StructDef,
    ) -> Result<(), TypeError> {
        let prop_ty = self.lower_type(&prop.ty.node)?;

        // Auto-property: no body to check, no codegen to emit
        if prop.get_body.is_none() && prop.set_body.is_none() {
            return Ok(());
        }

        // RFC 006 A2：`field` 关键字——检查前把访问器体内的 `Ident("field")`
        // 重写为 `this.<backing>`（backing field 字段访问，见 field_keyword.rs）。
        // 重写后的体同时用于 check_block 与 push_typed_fn，保证 typed_body 与
        // 存储的 AST 一致（codegen 复用既有字段访问路径，无需改动）。
        let backing = crate::field_keyword::backing_field_name(&prop.name);
        let get_body = prop
            .get_body
            .as_ref()
            .map(|b| rewrite_field_block(b, &backing));
        let set_body = prop
            .set_body
            .as_ref()
            .map(|b| rewrite_field_block(b, &backing));

        // Getter
        if let Some(get_body) = &get_body {
            let this_ty = TypeId::Ref {
                inner: Box::new(TypeId::Named(def.name.clone())),
                mutable: !def.is_readonly,
                kind: ast::RefKind::Value,
            };
            self.scopes.push(IndexMap::new());
            self.scopes
                .last_mut()
                .unwrap()
                .insert("this".into(), this_ty.clone());

            let get_params: Vec<(Ident, TypeId)> = vec![("this".into(), this_ty)];

            self.return_slot.push(prop_ty.clone());
            let prev_fn_static = self.current_fn_is_static;
            self.current_fn_is_static = false;
            let typed_body = self.check_block(get_body, &prop_ty)?;
            self.current_fn_is_static = prev_fn_static;
            self.return_slot.pop();

            let getter_name: Ident = format!("{}::get_{}", def.name, prop.name).into();
            self.push_typed_fn(
                getter_name,
                Some(def.name.clone()),
                false,
                get_params,
                prop_ty.clone(),
                Some(get_body.clone()),
                Some(typed_body),
                false,
                FnLinkage::User,
                false,
                // RFC 009 M3：property getter 不支持 `[Parallelize]` 属性。
                false,
            );

            self.scopes.pop();
        }

        // Setter
        if let Some(set_body) = &set_body {
            let this_ty = TypeId::Ref {
                inner: Box::new(TypeId::Named(def.name.clone())),
                mutable: !def.is_readonly,
                kind: ast::RefKind::Value,
            };
            self.scopes.push(IndexMap::new());
            self.scopes
                .last_mut()
                .unwrap()
                .insert("this".into(), this_ty.clone());
            self.scopes
                .last_mut()
                .unwrap()
                .insert("value".into(), prop_ty.clone());

            let mut set_params: Vec<(Ident, TypeId)> = vec![("this".into(), this_ty)];
            set_params.push(("value".into(), prop_ty.clone()));

            self.return_slot.push(TypeId::Void);
            let prev_fn_static = self.current_fn_is_static;
            self.current_fn_is_static = false;
            let typed_body = self.check_block(set_body, &TypeId::Void)?;
            self.current_fn_is_static = prev_fn_static;
            self.return_slot.pop();

            let setter_name: Ident = format!("{}::set_{}", def.name, prop.name).into();
            self.push_typed_fn(
                setter_name,
                Some(def.name.clone()),
                false,
                set_params,
                TypeId::Void,
                Some(set_body.clone()),
                Some(typed_body),
                false,
                FnLinkage::User,
                false,
                // RFC 009 M3：property setter 不支持 `[Parallelize]` 属性。
                false,
            );

            self.scopes.pop();
        }

        Ok(())
    }
}
