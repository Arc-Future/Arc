//! RFC 025 M2：跨包 `internal` 可见性单元测试（成员 + 类型级）。

#[cfg(test)]
mod tests {
    use ast::{Ident, Span, Visibility};
    use indexmap::IndexMap;

    use crate::oop_types::{
        AccessContext, ExtensionScope, FieldInfo, NominalType, TypeKind, TypeRegistry,
    };

    fn nom(name: &str, file_id: u32) -> NominalType {
        nom_with_vis(name, file_id, Visibility::Public)
    }

    fn nom_with_vis(name: &str, file_id: u32, vis: Visibility) -> NominalType {
        let mut fields = IndexMap::new();
        fields.insert(
            "Secret".into(),
            FieldInfo {
                name: "Secret".into(),
                ty: "int".into(),
                vis: Visibility::Internal,
                is_const: false,
                is_readonly: false,
                is_init_only: false,
                get_vis: None,
                set_vis: None,
                is_static: false,
                init: None,
            },
        );
        fields.insert(
            "Open".into(),
            FieldInfo {
                name: "Open".into(),
                ty: "int".into(),
                vis: Visibility::Public,
                is_const: false,
                is_readonly: false,
                is_init_only: false,
                get_vis: None,
                set_vis: None,
                is_static: false,
                init: None,
            },
        );
        NominalType {
            name: name.into(),
            kind: TypeKind::Class,
            vis,
            is_abstract: false,
            is_record: false,
            is_readonly: false,
            fields,
            methods: IndexMap::new(),
            bases: vec![],
            base_types: vec![],
            span: Span {
                file_id,
                start: 0,
                end: 0,
            },
            variants: vec![],
            generic_params: vec![],
            namespace: vec![],
            const_values: IndexMap::new(),
            constructors: vec![],
            soa: false,
            required_props: Default::default(),
        }
    }

    #[test]
    fn internal_same_package_allowed() {
        let mut reg = TypeRegistry::default();
        reg.types.insert("Helper".into(), nom("Helper", 1));
        reg.file_packages.insert(1, "LibA".into());
        let ctx = AccessContext {
            current_type: None,
            extension_scope: ExtensionScope::default(),
            current_package: Some("LibA".into()),
            enclosing_namespace: vec![],
            skip_type_visibility: false,
        };
        assert!(reg.can_access(Visibility::Internal, &Ident::from("Helper"), &ctx));
        assert!(reg.can_access(Visibility::Public, &Ident::from("Helper"), &ctx));
    }

    #[test]
    fn internal_cross_package_rejected() {
        let mut reg = TypeRegistry::default();
        reg.types.insert("Helper".into(), nom("Helper", 1));
        reg.file_packages.insert(1, "LibA".into());
        let ctx = AccessContext {
            current_type: None,
            extension_scope: ExtensionScope::default(),
            current_package: Some("LibB".into()),
            enclosing_namespace: vec![],
            skip_type_visibility: false,
        };
        assert!(!reg.can_access(Visibility::Internal, &Ident::from("Helper"), &ctx));
        assert!(reg.can_access(Visibility::Public, &Ident::from("Helper"), &ctx));
    }

    #[test]
    fn internal_cross_package_visible_to_allowed() {
        // RFC 025 M2+ InternalsVisibleTo：声明方包 LibA 对 LibB 开放 internal。
        let mut reg = TypeRegistry::default();
        reg.types.insert(
            "Helper".into(),
            nom_with_vis("Helper", 1, Visibility::Internal),
        );
        reg.file_packages.insert(1, "LibA".into());
        reg.internals_visible_to
            .insert("LibA".into(), vec!["LibB".to_string()]);
        let ctx = AccessContext {
            current_type: None,
            extension_scope: ExtensionScope::default(),
            current_package: Some("LibB".into()),
            enclosing_namespace: vec![],
            skip_type_visibility: false,
        };
        assert!(reg.can_access(Visibility::Internal, &Ident::from("Helper"), &ctx));
        assert!(reg.can_access_type(&Ident::from("Helper"), &ctx));
    }

    #[test]
    fn internal_cross_package_visible_to_does_not_leak_to_others() {
        // LibA 只对 LibB 开放；LibC 仍被拒（类型级 internal class）。
        let mut reg = TypeRegistry::default();
        reg.types.insert(
            "Helper".into(),
            nom_with_vis("Helper", 1, Visibility::Internal),
        );
        reg.file_packages.insert(1, "LibA".into());
        reg.internals_visible_to
            .insert("LibA".into(), vec!["LibB".to_string()]);
        let ctx = AccessContext {
            current_type: None,
            extension_scope: ExtensionScope::default(),
            current_package: Some("LibC".into()),
            enclosing_namespace: vec![],
            skip_type_visibility: false,
        };
        assert!(!reg.can_access(Visibility::Internal, &Ident::from("Helper"), &ctx));
        assert!(!reg.can_access_type(&Ident::from("Helper"), &ctx));
    }

    #[test]
    fn internal_cross_package_visible_to_member_allowed() {
        // InternalsVisibleTo 亦放行成员级 internal（经 resolve_field 路径）。
        let mut reg = TypeRegistry::default();
        reg.types.insert("Helper".into(), nom("Helper", 1));
        reg.file_packages.insert(1, "LibA".into());
        reg.internals_visible_to
            .insert("LibA".into(), vec!["LibB".to_string()]);
        let ctx = AccessContext {
            current_type: None,
            extension_scope: ExtensionScope::default(),
            current_package: Some("LibB".into()),
            enclosing_namespace: vec![],
            skip_type_visibility: false,
        };
        assert!(reg
            .resolve_field(&Ident::from("Helper"), &Ident::from("Secret"), &ctx)
            .is_ok());
        // 未开放包（LibC）经 resolve_field 访问 internal 成员仍被拒。
        let ctx_c = AccessContext {
            current_type: None,
            extension_scope: ExtensionScope::default(),
            current_package: Some("LibC".into()),
            enclosing_namespace: vec![],
            skip_type_visibility: false,
        };
        assert!(reg
            .resolve_field(&Ident::from("Helper"), &Ident::from("Secret"), &ctx_c)
            .is_err());
    }

    #[test]
    fn internal_without_package_map_remains_module_wide() {
        let mut reg = TypeRegistry::default();
        reg.types.insert("Helper".into(), nom("Helper", 1));
        // file_packages 空 → 单模块 MVP
        let ctx = AccessContext {
            current_type: None,
            extension_scope: ExtensionScope::default(),
            current_package: Some("LibB".into()),
            enclosing_namespace: vec![],
            skip_type_visibility: false,
        };
        assert!(reg.can_access(Visibility::Internal, &Ident::from("Helper"), &ctx));
    }

    #[test]
    fn internal_class_same_package_type_accessible() {
        let mut reg = TypeRegistry::default();
        reg.types.insert(
            "Secret".into(),
            nom_with_vis("Secret", 1, Visibility::Internal),
        );
        reg.file_packages.insert(1, "LibA".into());
        let ctx = AccessContext {
            current_type: None,
            extension_scope: ExtensionScope::default(),
            current_package: Some("LibA".into()),
            enclosing_namespace: vec![],
            skip_type_visibility: false,
        };
        assert!(reg.can_access_type(&Ident::from("Secret"), &ctx));
    }

    #[test]
    fn internal_class_cross_package_type_rejected() {
        let mut reg = TypeRegistry::default();
        reg.types.insert(
            "Secret".into(),
            nom_with_vis("Secret", 1, Visibility::Internal),
        );
        reg.file_packages.insert(1, "LibA".into());
        let ctx = AccessContext {
            current_type: None,
            extension_scope: ExtensionScope::default(),
            current_package: Some("LibB".into()),
            enclosing_namespace: vec![],
            skip_type_visibility: false,
        };
        assert!(!reg.can_access_type(&Ident::from("Secret"), &ctx));
        // public 成员也无法经由不可见类型访问
        assert!(reg
            .resolve_field(&Ident::from("Secret"), &Ident::from("Open"), &ctx)
            .is_err());
    }

    #[test]
    fn explicit_private_class_type_remains_name_visible() {
        let mut reg = TypeRegistry::default();
        reg.types.insert(
            "Helper".into(),
            nom_with_vis("Helper", 1, Visibility::Private),
        );
        reg.file_packages.insert(1, "LibA".into());
        let ctx = AccessContext {
            current_type: None,
            extension_scope: ExtensionScope::default(),
            current_package: Some("LibB".into()),
            enclosing_namespace: vec![],
            skip_type_visibility: false,
        };
        // 显式 `private` 顶层：类型级暂不强制（嵌套私有语义未单列）
        assert!(reg.can_access_type(&Ident::from("Helper"), &ctx));
    }
}
