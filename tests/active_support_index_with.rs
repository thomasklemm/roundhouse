//! Rails 8.1.4 differential and lowering contract for block-form
//! `Enumerable#index_with`.

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;

use roundhouse::emit::ruby;
use roundhouse::ingest::ingest_app_from_tree;

#[test]
fn index_with_matches_rails_814_for_values_duplicates_and_empty_input() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let script = r#"
require "active_support"
require "active_support/core_ext/enumerable"
load ARGV.fetch(0)

raise "expected Rails 8.1.4, got #{ActiveSupport.version}" unless ActiveSupport.version.to_s == "8.1.4"

def assert(value, message)
  raise message unless value
end

[
  [[], ->(item) { item }],
  [["a", "bb", "a"], ->(item) { item.length }],
  [[1, 2, 3], ->(item) { item.even? }],
].each do |items, transform|
  rails = items.index_with(&transform)
  runtime = ActiveSupport.index_with(items, &transform)
  assert(runtime == rails, "index_with result differs from Rails 8.1.4: #{items.inspect}")
end

seen = []
result = ActiveSupport.index_with(["a", "bb", "a"]) { |item| seen << item; item.length }
assert(result == { "a" => 1, "bb" => 2 }, "duplicate keys must keep the final block result")
assert(seen == ["a", "bb", "a"], "block runs once per item in order")
puts "ActiveSupport 8.1.4 index_with differential passed"
"#;
    let output = Command::new("ruby")
        .arg("-e")
        .arg(script)
        .arg(root.join("runtime/ruby/active_support_enumerable_ext.rb"))
        .output()
        .expect("ruby is on PATH");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success() && stdout.contains("differential passed"),
        "Rails 8.1.4 differential failed\n=== stdout ===\n{stdout}\n=== stderr ===\n{stderr}"
    );
}

#[test]
fn zero_arg_block_form_is_grounded_once_to_the_shared_runtime() {
    let mut tree: HashMap<PathBuf, Vec<u8>> = HashMap::new();
    tree.insert(
        PathBuf::from("app/models/index_with_probe.rb"),
        b"class IndexWithProbe\n  def self.values\n    [\"a\", \"bb\"].index_with { |item| item.length }\n  end\nend\n".to_vec(),
    );
    let mut app = ingest_app_from_tree(tree).expect("ingest");
    roundhouse::session::analyze_and_lower(&mut app);
    let emitted = ruby::emit_library(&app)
        .into_iter()
        .find(|file| file.path.ends_with("index_with_probe.rb"))
        .expect("probe emitted")
        .content;
    assert!(
        emitted.contains("ActiveSupport.index_with([\"a\", \"bb\"])")
            || emitted.contains("ActiveSupport.index_with([\"a\", \"bb\"]){"),
        "{emitted}"
    );
}

#[test]
fn default_value_forms_are_not_misrouted_to_the_block_runtime() {
    let mut tree: HashMap<PathBuf, Vec<u8>> = HashMap::new();
    tree.insert(
        PathBuf::from("app/models/index_with_probe.rb"),
        b"class IndexWithProbe\n  def self.with_default\n    [\"a\"].index_with(\"fallback\")\n  end\n  def self.with_default_block\n    [\"a\"].index_with(\"fallback\") { |item| item.upcase }\n  end\nend\n".to_vec(),
    );
    let mut app = ingest_app_from_tree(tree).expect("ingest");
    roundhouse::session::analyze_and_lower(&mut app);
    let emitted = ruby::emit_library(&app)
        .into_iter()
        .find(|file| file.path.ends_with("index_with_probe.rb"))
        .expect("probe emitted")
        .content;
    assert!(!emitted.contains("ActiveSupport.index_with"), "{emitted}");
    assert_eq!(
        emitted.matches("index_with(\"fallback\")").count(),
        2,
        "{emitted}"
    );
}
