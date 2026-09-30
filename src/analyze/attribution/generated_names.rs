//! The methods a state-machine declaration generates, read off the
//! declaration itself. Diagnostic-only: the names label a failed
//! dispatch as the gem's, they never become typed methods.
//!
//! `aasm` and `state_machines` name their methods after the author's
//! states and events (`event :publish` → `publish!`, `may_publish?`), so
//! no fixed list in `gems::SURFACES` can hold them. The rules below were
//! recorded against aasm 6.0 and state_machines-activerecord 0.200 by
//! diffing each declaring class's methods against a bare model's.

use std::collections::BTreeSet;

use ruby_prism::{CallNode, Node};

/// One declaration's DSL (`aasm`, `state_machine`) and the instance and
/// class methods it generates. The diagnostic does not tell `@article.x`
/// from `Article.x` apart, so both land in one set.
pub(super) struct Generated {
    pub dsl: &'static str,
    pub names: BTreeSet<String>,
}

/// The declarations among a class or module body's statements, looking
/// through `included do` and `with_options do`, where concerns keep them.
pub(super) fn harvest_body(body: &Node<'_>, out: &mut Vec<Generated>) {
    let Some(statements) = body.as_statements_node() else { return };
    for statement in statements.body().iter() {
        let Some(call) = statement.as_call_node() else { continue };
        if call.receiver().is_some() {
            continue;
        }
        let Some(block) = call.block().and_then(|b| b.as_block_node()) else { continue };
        match call.name().as_slice() {
            b"included" | b"with_options" => {
                if let Some(inner) = block.body() {
                    harvest_body(&inner, out);
                }
            }
            b"aasm" => out.push(Generated { dsl: "aasm", names: aasm(&call, block.body()) }),
            b"state_machine" => out.push(Generated {
                dsl: "state_machine",
                names: state_machine(&call, block.body()),
            }),
            _ => {}
        }
    }
}

/// `state :draft` → `draft?` and the `draft` scope; `event :publish` →
/// `publish`, `publish!`, `publish_without_validation!`, `may_publish?`.
/// Under `namespace: :rev` states read `rev_draft?` (the scope keeps
/// both spellings) and events `publish_rev`.
fn aasm(call: &CallNode<'_>, body: Option<Node<'_>>) -> BTreeSet<String> {
    let namespace = option(call, "namespace").as_ref().and_then(name_of);
    let scopes = option(call, "create_scopes").map_or(true, |v| v.as_false_node().is_none());
    let (mut states, mut events) = (Vec::new(), Vec::new());
    for statement in statements(body) {
        let Some(inner) = statement.as_call_node().filter(|c| c.receiver().is_none()) else { continue };
        match inner.name().as_slice() {
            b"state" => states.extend(positional(&inner).iter().filter_map(name_of)),
            b"event" => events.extend(positional(&inner).first().and_then(name_of)),
            _ => {}
        }
    }
    let mut names = BTreeSet::new();
    for state in &states {
        match &namespace {
            Some(ns) => {
                names.insert(format!("{ns}_{state}?"));
                if scopes {
                    names.insert(format!("{ns}_{state}"));
                }
            }
            None => {
                names.insert(format!("{state}?"));
            }
        }
        if scopes {
            names.insert(state.clone());
        }
    }
    for event in &events {
        let base = match &namespace {
            Some(ns) => format!("{event}_{ns}"),
            None => event.clone(),
        };
        names.insert(format!("{base}!"));
        names.insert(format!("{base}_without_validation!"));
        names.insert(format!("may_{base}?"));
        names.insert(base);
    }
    names
}

/// `state_machine :status` → the attribute's own surface (`status_name`,
/// `with_status`, …), each state's `open?`, and each event's `close`,
/// `close!`, `can_close?`, `close_transition`. States are also declared
/// by naming them in a transition or as `initial:`; under `namespace:
/// "alarm"` states read `alarm_open?` and events `close_alarm`.
fn state_machine(call: &CallNode<'_>, body: Option<Node<'_>>) -> BTreeSet<String> {
    let attr = positional(call).first().and_then(name_of).unwrap_or_else(|| "state".to_string());
    let namespace = option(call, "namespace").as_ref().and_then(name_of);
    let mut states: BTreeSet<String> = option(call, "initial").as_ref().and_then(name_of).into_iter().collect();
    let mut events = BTreeSet::new();
    if let Some(body) = body {
        collect_machine(&body, &mut states, &mut events);
    }
    let plural = crate::naming::pluralize_snake(&attr);
    let mut names: BTreeSet<String> = [
        format!("{attr}?"),
        format!("{attr}_name"),
        format!("{attr}_events"),
        format!("{attr}_transitions"),
        format!("{attr}_paths"),
        format!("{attr}_event"),
        format!("{attr}_event="),
        format!("{attr}_event_transition"),
        format!("{attr}_event_transition="),
        format!("fire_{attr}_event"),
        format!("human_{attr}_name"),
        format!("human_{attr}_event_name"),
        format!("with_{attr}"),
        format!("with_{plural}"),
        format!("without_{attr}"),
        format!("without_{plural}"),
    ]
    .into_iter()
    .collect();
    for state in &states {
        names.insert(match &namespace {
            Some(ns) => format!("{ns}_{state}?"),
            None => format!("{state}?"),
        });
    }
    for event in &events {
        let base = match &namespace {
            Some(ns) => format!("{event}_{ns}"),
            None => event.clone(),
        };
        names.insert(format!("{base}!"));
        names.insert(format!("can_{base}?"));
        names.insert(format!("{base}_transition"));
        names.insert(base);
    }
    names
}

/// Every `state`, `event` and `transition` anywhere in the machine's
/// block (events nest their transitions; states may carry blocks).
fn collect_machine(node: &Node<'_>, states: &mut BTreeSet<String>, events: &mut BTreeSet<String>) {
    if let Some(call) = node.as_call_node().filter(|c| c.receiver().is_none()) {
        match call.name().as_slice() {
            b"state" => positional(&call).iter().for_each(|a| symbols(a, states)),
            b"event" => positional(&call).iter().for_each(|a| symbols(a, events)),
            b"transition" => {
                for pair in keywords(&call) {
                    let key = name_of(&pair.key());
                    match key.as_deref() {
                        Some("on" | "except_on") => symbols(&pair.value(), events),
                        // Method names, not states.
                        Some("if" | "unless") => {}
                        Some("from" | "to" | "except_from" | "except_to") => symbols(&pair.value(), states),
                        _ => {
                            symbols(&pair.key(), states);
                            symbols(&pair.value(), states);
                        }
                    }
                }
            }
            _ => {}
        }
        if let Some(block) = call.block().and_then(|b| b.as_block_node()).and_then(|b| b.body()) {
            collect_machine(&block, states, events);
        }
    } else if let Some(statements) = node.as_statements_node() {
        for statement in statements.body().iter() {
            collect_machine(&statement, states, events);
        }
    }
}

/// Symbols in a state position: a literal, an array of them, or the
/// operands of `all - [:parked]`. `all`, `any` and `same` name none.
fn symbols(node: &Node<'_>, out: &mut BTreeSet<String>) {
    if let Some(name) = node.as_symbol_node().and_then(|_| name_of(node)) {
        out.insert(name);
    } else if let Some(array) = node.as_array_node() {
        array.elements().iter().for_each(|e| symbols(&e, out));
    } else if let Some(call) = node.as_call_node() {
        call.receiver().iter().for_each(|r| symbols(r, out));
        positional(&call).iter().for_each(|a| symbols(a, out));
    }
}

fn statements<'pr>(body: Option<Node<'pr>>) -> Vec<Node<'pr>> {
    body.and_then(|b| b.as_statements_node()).map(|s| s.body().iter().collect()).unwrap_or_default()
}

fn positional<'pr>(call: &CallNode<'pr>) -> Vec<Node<'pr>> {
    call.arguments()
        .map(|a| a.arguments().iter().filter(|n| n.as_keyword_hash_node().is_none()).collect())
        .unwrap_or_default()
}

fn keywords<'pr>(call: &CallNode<'pr>) -> Vec<ruby_prism::AssocNode<'pr>> {
    let Some(args) = call.arguments() else { return Vec::new() };
    let mut out = Vec::new();
    for arg in args.arguments().iter() {
        let elements = match (arg.as_keyword_hash_node(), arg.as_hash_node()) {
            (Some(kh), _) => kh.elements(),
            (_, Some(h)) => h.elements(),
            _ => continue,
        };
        out.extend(elements.iter().filter_map(|e| e.as_assoc_node()));
    }
    out
}

fn option<'pr>(call: &CallNode<'pr>, key: &str) -> Option<Node<'pr>> {
    keywords(call).into_iter().find(|p| name_of(&p.key()).as_deref() == Some(key)).map(|p| p.value())
}

/// A symbol's or string's text.
fn name_of(node: &Node<'_>) -> Option<String> {
    if let Some(sym) = node.as_symbol_node() {
        return Some(String::from_utf8_lossy(sym.unescaped()).into_owned());
    }
    node.as_string_node().map(|s| String::from_utf8_lossy(s.unescaped()).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn harvest(source: &str) -> Vec<(&'static str, Vec<String>)> {
        let parsed = ruby_prism::parse(source.as_bytes());
        let program = parsed.node();
        let class = program.as_program_node().unwrap().statements().body().iter().next().unwrap();
        let body = class
            .as_class_node()
            .and_then(|c| c.body())
            .or_else(|| class.as_module_node().and_then(|m| m.body()))
            .unwrap();
        let mut out = Vec::new();
        harvest_body(&body, &mut out);
        out.into_iter().map(|g| (g.dsl, g.names.into_iter().collect())).collect()
    }

    fn names(list: &[&str]) -> Vec<String> {
        let mut v: Vec<String> = list.iter().map(|s| s.to_string()).collect();
        v.sort();
        v
    }

    // Each expectation is the gem's own answer: the declaring model's
    // `instance_methods` and `methods` minus a bare model's, less the
    // fixed internals every machine gets (`aasm_read_state`,
    // `fire_events`, `initialize_state_machines`, …).

    #[test]
    fn aasm_states_and_events() {
        let got = harvest(
            "class A < ApplicationRecord
  aasm column: :state do
    state :draft, initial: true
    state :published
    event :publish do
      transitions from: :draft, to: :published
    end
  end
end",
        );
        assert_eq!(
            got,
            vec![(
                "aasm",
                names(&[
                    "draft?", "published?", "draft", "published", "publish", "publish!",
                    "publish_without_validation!", "may_publish?",
                ])
            )]
        );
    }

    #[test]
    fn aasm_namespace() {
        let got = harvest(
            "class B < ApplicationRecord
  aasm(:review, column: :review, namespace: :rev) do
    state :pending, initial: true
    state :ok
    event(:approve) { transitions from: :pending, to: :ok }
  end
end",
        );
        assert_eq!(
            got,
            vec![(
                "aasm",
                names(&[
                    "rev_pending?", "rev_ok?", "pending", "ok", "rev_pending", "rev_ok",
                    "approve_rev", "approve_rev!", "approve_rev_without_validation!", "may_approve_rev?",
                ])
            )]
        );
    }

    #[test]
    fn state_machine_states_from_transitions() {
        let got = harvest(
            "class C < ApplicationRecord
  state_machine :status, initial: :open do
    event(:close) { transition open: :closed }
    event(:reopen) { transition all - [:open] => :open, :archived => same }
    state :limbo
  end
end",
        );
        assert_eq!(
            got,
            vec![(
                "state_machine",
                names(&[
                    "archived?", "closed?", "limbo?", "open?",
                    "close", "close!", "can_close?", "close_transition",
                    "reopen", "reopen!", "can_reopen?", "reopen_transition",
                    "status?", "status_event", "status_event=", "status_event_transition",
                    "status_event_transition=", "status_events", "status_name", "status_paths",
                    "status_transitions", "fire_status_event", "human_status_name",
                    "human_status_event_name", "with_status", "with_statuses", "without_status",
                    "without_statuses",
                ])
            )]
        );
    }

    #[test]
    fn state_machine_namespace_and_default_attribute() {
        let got = harvest(
            "class D < ApplicationRecord
  state_machine :alarm_state, initial: :active, namespace: \"alarm\" do
    event(:enable) { transition all => :active }
    event(:disable) { transition from: [:active], to: :off }
  end
  state_machine do
    event(:go) { transition :idle => :running }
  end
end",
        );
        assert_eq!(
            got,
            vec![
                (
                    "state_machine",
                    names(&[
                        "alarm_active?", "alarm_off?",
                        "enable_alarm", "enable_alarm!", "can_enable_alarm?", "enable_alarm_transition",
                        "disable_alarm", "disable_alarm!", "can_disable_alarm?", "disable_alarm_transition",
                        "alarm_state?", "alarm_state_event", "alarm_state_event=",
                        "alarm_state_event_transition", "alarm_state_event_transition=",
                        "alarm_state_events", "alarm_state_name", "alarm_state_paths",
                        "alarm_state_transitions", "fire_alarm_state_event", "human_alarm_state_name",
                        "human_alarm_state_event_name", "with_alarm_state", "with_alarm_states",
                        "without_alarm_state", "without_alarm_states",
                    ])
                ),
                (
                    "state_machine",
                    names(&[
                        "idle?", "running?", "go", "go!", "can_go?", "go_transition",
                        "state?", "state_event", "state_event=", "state_event_transition",
                        "state_event_transition=", "state_events", "state_name", "state_paths",
                        "state_transitions", "fire_state_event", "human_state_name",
                        "human_state_event_name", "with_state", "with_states", "without_state",
                        "without_states",
                    ])
                ),
            ]
        );
    }

    #[test]
    fn concern_blocks_and_non_declarations() {
        let got = harvest(
            "module Workflow
  extend ActiveSupport::Concern
  included do
    with_options do
      state_machine :delivery_status do
        event(:deliver) { transition waiting: :delivered, if: :ready? }
      end
    end
  end
  aasm
  self.aasm do; end
  def ordinary
    aasm do; end
  end
end",
        );
        assert_eq!(got.len(), 1, "only the block declaration inside `included do` counts");
        let (dsl, found) = &got[0];
        assert_eq!(*dsl, "state_machine");
        assert!(found.contains(&"deliver!".to_string()));
        assert!(found.contains(&"delivered?".to_string()));
        assert!(!found.contains(&"ready??".to_string()) && !found.contains(&"ready?".to_string()));
    }
}
