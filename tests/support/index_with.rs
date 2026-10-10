pub const SOURCE: &str = r#"class IndexWithProbe
  def self.values
    ["a", "bb", "a"].index_with { |item| item.length }
  end
end
"#;

pub const ASSERTIONS: &str = r#"
values = IndexWithProbe.values
raise "unexpected index_with result: #{values.inspect}" unless values == { "a" => 1, "bb" => 2 }
puts "index_with contract passed"
"#;

pub fn overlay() -> super::emit_and_run::Overlay {
    super::emit_and_run::real_blog().write("app/models/index_with_probe.rb", SOURCE)
}
