//! Ground the zero-argument block form of ActiveSupport's
//! `Enumerable#index_with` to the shared runtime function. The
//! default-value form, with or without a block, is left untouched
//! because it has a different contract and needs a sentinel.

use crate::expr::{Expr, ExprNode};
use crate::ident::Symbol;
use crate::ty::Ty;

pub(crate) fn rewrite_node(expr: &mut Expr) {
    let span = expr.span;
    let ExprNode::Send {
        recv,
        method,
        args,
        block: Some(_),
        parenthesized,
    } = &mut *expr.node
    else {
        return;
    };
    if method.as_str() != "index_with" || !args.is_empty() {
        return;
    }
    let Some(receiver) = recv.take() else { return };
    *recv = Some(Expr::new(
        span,
        ExprNode::Const {
            path: vec![Symbol::from("ActiveSupport")],
        },
    ));
    *method = Symbol::from("index_with");
    args.push(receiver);
    *parenthesized = true;
    expr.ty = Some(Ty::Hash {
        key: Box::new(Ty::Untyped),
        value: Box::new(Ty::Untyped),
    });
}
