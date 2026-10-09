//! Rust-specific flattening for `LibraryClass` inheritance.
//!
//! Rust structs do not inherit associated methods from a superclass.
//! Constructors are especially important: an inherited Ruby
//! `initialize` is the implementation of `<Subclass>::new`, even when
//! the subclass declares no initializer of its own.

use crate::dialect::{LibraryClass, MethodReceiver};
use crate::expr::{Expr, ExprNode};
use crate::ident::ClassId;

/// Copy an inherited instance `initialize` onto each target class that
/// lacks one. `available` must include target classes and any source or
/// runtime superclass classes they depend on. This is a Rust-only
/// lowering step; class-inheritance targets should keep native dispatch.
///
/// Initializers containing `super` are deliberately not flattened: the
/// Rust class representation has no super-method dispatch to preserve.
pub fn flatten_inherited_initializers(targets: &mut [LibraryClass], available: &[LibraryClass]) {
    let classes: std::collections::HashMap<ClassId, LibraryClass> = available
        .iter()
        .chain(targets.iter())
        .map(|class| (class.name.clone(), class.clone()))
        .collect();

    for target in targets {
        if target.methods.iter().any(|method| {
            method.name.as_str() == "initialize" && method.receiver == MethodReceiver::Instance
        }) {
            continue;
        }

        let mut parent = target.parent.as_ref();
        let mut seen = std::collections::HashSet::new();
        while let Some(parent_id) = parent {
            if !seen.insert(parent_id.clone()) {
                break;
            }
            let Some(parent_class) = classes.get(parent_id) else {
                break;
            };
            if let Some(initializer) = parent_class.methods.iter().find(|method| {
                method.name.as_str() == "initialize" && method.receiver == MethodReceiver::Instance
            }) {
                if !contains_super(&initializer.body) {
                    let mut inherited = initializer.clone();
                    inherited.enclosing_class = Some(target.name.0.clone());
                    target.methods.push(inherited);
                }
                break;
            }
            parent = parent_class.parent.as_ref();
        }
    }
}

/// Flatten only inherited methods reachable from the target's own
/// receiverless calls, then qualify those calls with `self`. The Rust
/// emitter has no superclass dispatch, and receiverless Sends otherwise
/// become free-function calls. Dependencies are followed transitively;
/// unrelated ancestor methods are not copied. Child definitions win;
/// the nearest ancestor supplies any missing method. Methods whose
/// bodies call `super` are left unflattened rather than emitting a false
/// implementation.
pub fn flatten_inherited_instance_methods(
    targets: &mut [LibraryClass],
    available: &[LibraryClass],
) {
    flatten_inherited_instance_methods_for_consumers(targets, available, &[]);
}

/// Variant of [`flatten_inherited_instance_methods`] that also seeds
/// inheritance from typed explicit method calls in consumer classes,
/// such as a view invoking a method on a controller-helper value. Only
/// calls whose receiver type is exactly the target class are considered;
/// unrelated ancestor methods remain unflattened.
pub fn flatten_inherited_instance_methods_for_consumers(
    targets: &mut [LibraryClass],
    available: &[LibraryClass],
    consumers: &[LibraryClass],
) {
    let classes: std::collections::HashMap<ClassId, LibraryClass> = available
        .iter()
        .chain(targets.iter())
        .map(|class| (class.name.clone(), class.clone()))
        .collect();

    for target in targets {
        let mut ancestors = Vec::new();
        let mut parent = target.parent.as_ref();
        let mut seen_classes = std::collections::HashSet::new();
        while let Some(parent_id) = parent {
            if !seen_classes.insert(parent_id.clone()) {
                break;
            }
            let Some(parent_class) = classes.get(parent_id) else {
                break;
            };
            ancestors.push(parent_class.clone());
            parent = parent_class.parent.as_ref();
        }

        let mut pending = Vec::new();
        for method in &target.methods {
            if method.receiver == MethodReceiver::Instance {
                collect_implicit_calls(&method.body, &mut pending);
            }
        }
        for consumer in consumers {
            for method in &consumer.methods {
                collect_explicit_calls_for_class(&method.body, &target.name, &mut pending);
            }
        }
        let mut attempted = std::collections::HashSet::new();
        while let Some(name) = pending.pop() {
            if target
                .methods
                .iter()
                .any(|method| method.receiver == MethodReceiver::Instance && method.name == name)
                || !attempted.insert(name.clone())
            {
                continue;
            }
            let Some(inherited) = ancestors.iter().find_map(|ancestor| {
                ancestor.methods.iter().find(|method| {
                    method.receiver == MethodReceiver::Instance && method.name == name
                })
            }) else {
                continue;
            };
            if contains_super(&inherited.body) {
                continue;
            }
            let mut inherited = inherited.clone();
            inherited.enclosing_class = Some(target.name.0.clone());
            collect_implicit_calls(&inherited.body, &mut pending);
            target.methods.push(inherited);
        }

        let instance_methods: std::collections::HashSet<_> = target
            .methods
            .iter()
            .filter(|method| method.receiver == MethodReceiver::Instance)
            .map(|method| method.name.clone())
            .collect();
        let class_ty = crate::ty::Ty::Class {
            id: target.name.clone(),
            args: Vec::new(),
        };
        for method in &mut target.methods {
            if method.receiver == MethodReceiver::Instance {
                qualify_implicit_instance_calls(&mut method.body, &instance_methods, &class_ty);
            }
        }
    }
}

fn collect_explicit_calls_for_class(
    expr: &Expr,
    class: &ClassId,
    out: &mut Vec<crate::ident::Symbol>,
) {
    if let ExprNode::Send {
        recv: Some(recv),
        method,
        ..
    } = &*expr.node
    {
        if matches!(
            recv.ty.as_ref(),
            Some(crate::ty::Ty::Class { id, .. }) if id == class
        ) {
            out.push(method.clone());
        }
    }
    expr.node.for_each_child(&mut |child| {
        collect_explicit_calls_for_class(child, class, out)
    });
}

fn collect_implicit_calls(expr: &Expr, out: &mut Vec<crate::ident::Symbol>) {
    if let ExprNode::Send {
        recv: None, method, ..
    } = &*expr.node
    {
        out.push(method.clone());
    }
    expr.node
        .for_each_child(&mut |child| collect_implicit_calls(child, out));
}

fn qualify_implicit_instance_calls(
    expr: &mut Expr,
    instance_methods: &std::collections::HashSet<crate::ident::Symbol>,
    class_ty: &crate::ty::Ty,
) {
    expr.node.for_each_child_mut(&mut |child| {
        qualify_implicit_instance_calls(child, instance_methods, class_ty)
    });
    let ExprNode::Send { recv, method, .. } = &mut *expr.node else {
        return;
    };
    if recv.is_some() || !instance_methods.contains(method) {
        return;
    }
    let mut self_ref = Expr::new(expr.span, ExprNode::SelfRef);
    self_ref.ty = Some(class_ty.clone());
    *recv = Some(self_ref);
}

fn contains_super(expr: &Expr) -> bool {
    if matches!(&*expr.node, ExprNode::Super { .. }) {
        return true;
    }
    let mut found = false;
    expr.node
        .for_each_child(&mut |child| found |= contains_super(child));
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dialect::{AccessorKind, MethodDef, Param};
    use crate::effect::EffectSet;
    use crate::expr::{Expr, ExprNode, LValue, Literal};
    use crate::ident::Symbol;
    use crate::span::Span;
    use crate::ty::Ty;

    fn class(name: &str, parent: Option<&str>, methods: Vec<MethodDef>) -> LibraryClass {
        LibraryClass {
            name: ClassId(Symbol::from(name)),
            is_module: false,
            parent: parent.map(|parent| ClassId(Symbol::from(parent))),
            parent_span: Span::synthetic(),
            includes: Vec::new(),
            methods,
            class_ivar_initializers: Vec::new(),
            nullable_columns: Vec::new(),
            origin: None,
            constants: Vec::new(),
            unknown_calls: Vec::new(),
        }
    }

    fn initializer(name: &str, body: Expr) -> MethodDef {
        MethodDef {
            visibility: crate::dialect::MethodVisibility::Public,
            unsupported_formals: None,
            has_anonymous_block: false,
            name_span: Span::synthetic(),
            name: Symbol::from(name),
            receiver: MethodReceiver::Instance,
            params: vec![Param::positional(Symbol::from("ua"))],
            body,
            signature: Some(Ty::Fn {
                params: vec![crate::ty::Param {
                    name: Symbol::from("ua"),
                    ty: Ty::Str,
                    kind: crate::ty::ParamKind::Required,
                }],
                block: None,
                ret: Box::new(Ty::Nil),
                effects: EffectSet::default(),
            }),
            effects: EffectSet::default(),
            enclosing_class: Some(Symbol::from(name)),
            kind: AccessorKind::Method,
            is_async: false,
            mutates_self: false,
            block_param: None,
        }
    }

    fn platform_initializer() -> MethodDef {
        let span = Span::synthetic();
        let body = Expr::new(
            span,
            ExprNode::Assign {
                target: LValue::Ivar {
                    name: Symbol::from("user_agent_string"),
                },
                value: Expr::new(
                    span,
                    ExprNode::Var {
                        id: crate::ident::VarId(0),
                        name: Symbol::from("ua"),
                    },
                ),
            },
        );
        initializer("initialize", body)
    }

    #[test]
    fn inherited_methods_and_implicit_calls_are_flattened() {
        let span = Span::synthetic();
        let bare_call = |name: &str| {
            Expr::new(
                span,
                ExprNode::Send {
                    recv: None,
                    method: Symbol::from(name),
                    args: Vec::new(),
                    block: None,
                    parenthesized: false,
                },
            )
        };
        let parent = class(
            "PlatformAgent",
            None,
            vec![
                initializer("user_agent", bare_call("parse_user_agent")),
                initializer("match_pred", bare_call("user_agent")),
                initializer("os", bare_call("user_agent")),
                initializer("unrelated", bare_call("parse_user_agent")),
            ],
        );
        let own_predicate = initializer(
            "ios?",
            Expr::new(
                span,
                ExprNode::Seq {
                    exprs: vec![bare_call("match_pred"), bare_call("os")],
                },
            ),
        );
        let mut targets = vec![class(
            "ApplicationPlatform",
            Some("PlatformAgent"),
            vec![own_predicate],
        )];

        flatten_inherited_instance_methods(&mut targets, &[parent]);

        for name in ["user_agent", "match_pred", "os"] {
            assert!(
                targets[0]
                    .methods
                    .iter()
                    .any(|method| method.name.as_str() == name),
                "missing inherited method {name}"
            );
        }
        assert!(
            targets[0]
                .methods
                .iter()
                .all(|method| method.name.as_str() != "unrelated"),
            "unreferenced ancestor methods should not be copied"
        );
        let predicate = targets[0]
            .methods
            .iter()
            .find(|method| method.name.as_str() == "ios?")
            .expect("child predicate");
        let ExprNode::Seq { exprs } = &*predicate.body.node else {
            panic!("expected sequence body")
        };
        for call in exprs {
            assert!(matches!(
                &*call.node,
                ExprNode::Send {
                    recv: Some(receiver),
                    ..
                } if matches!(&*receiver.node, ExprNode::SelfRef)
            ));
        }
    }

    #[test]
    fn typed_consumer_calls_flatten_only_the_used_inherited_method() {
        let span = Span::synthetic();
        let call = Expr::new(
            span,
            ExprNode::Send {
                recv: Some({
                    let mut receiver = Expr::new(
                        span,
                        ExprNode::Var {
                            id: crate::ident::VarId(0),
                            name: Symbol::from("platform"),
                        },
                    );
                    receiver.ty = Some(Ty::Class {
                        id: ClassId(Symbol::from("ApplicationPlatform")),
                        args: Vec::new(),
                    });
                    receiver
                }),
                method: Symbol::from("browser"),
                args: Vec::new(),
                block: None,
                parenthesized: false,
            },
        );
        let parent = class(
            "PlatformAgent",
            None,
            vec![
                initializer("browser", call.clone()),
                initializer("unrelated", call.clone()),
            ],
        );
        let mut targets = vec![class(
            "ApplicationPlatform",
            Some("PlatformAgent"),
            Vec::new(),
        )];
        let consumers = vec![class("PwaViews", None, vec![initializer("show", call)])];

        flatten_inherited_instance_methods_for_consumers(
            &mut targets,
            &[parent],
            &consumers,
        );

        assert!(
            targets[0]
                .methods
                .iter()
                .any(|method| method.name.as_str() == "browser"),
            "the concrete view call requires ApplicationPlatform::browser"
        );
        assert!(
            targets[0]
                .methods
                .iter()
                .all(|method| method.name.as_str() != "unrelated"),
            "unreferenced inherited instance methods must remain omitted"
        );
    }

    #[test]
    fn inherited_initialize_becomes_the_child_constructor() {
        let parent = class("PlatformAgent", None, vec![platform_initializer()]);
        let mut targets = vec![class("ApplicationPlatform", Some("PlatformAgent"), vec![])];

        flatten_inherited_initializers(&mut targets, &[parent]);

        let initializer = targets[0]
            .methods
            .iter()
            .find(|method| method.name.as_str() == "initialize")
            .expect("inherited initializer");
        assert_eq!(
            initializer.enclosing_class,
            Some(Symbol::from("ApplicationPlatform"))
        );
        assert_eq!(initializer.params[0].name.as_str(), "ua");
        assert!(matches!(
            &*initializer.body.node,
            ExprNode::Assign {
                target: LValue::Ivar { name },
                value,
            } if name.as_str() == "user_agent_string"
                && matches!(&*value.node, ExprNode::Var { name, .. } if name.as_str() == "ua")
        ));
    }

    #[test]
    fn child_initializer_overrides_inherited_initializer() {
        let parent = class("PlatformAgent", None, vec![platform_initializer()]);
        let child_initializer = initializer(
            "initialize",
            Expr::new(
                Span::synthetic(),
                ExprNode::Lit {
                    value: Literal::Nil,
                },
            ),
        );
        let mut targets = vec![class(
            "ApplicationPlatform",
            Some("PlatformAgent"),
            vec![child_initializer.clone()],
        )];

        flatten_inherited_initializers(&mut targets, &[parent]);

        assert_eq!(targets[0].methods.len(), 1);
        assert_eq!(targets[0].methods[0], child_initializer);
    }

    #[test]
    fn initializer_with_super_is_not_flattened() {
        let parent = class(
            "PlatformAgent",
            None,
            vec![initializer(
                "initialize",
                Expr::new(Span::synthetic(), ExprNode::Super { args: None }),
            )],
        );
        let mut targets = vec![class("ApplicationPlatform", Some("PlatformAgent"), vec![])];

        flatten_inherited_initializers(&mut targets, &[parent]);

        assert!(targets[0].methods.is_empty());
    }
}
