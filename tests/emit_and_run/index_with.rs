//! The block form of ActiveSupport `Enumerable#index_with` on emitted CRuby.

use super::index_with;

#[test]
fn index_with_block_form_runs_in_emitted_cruby() {
    let run = index_with::overlay().run_ruby(index_with::ASSERTIONS);
    run.assert_passes();
    assert!(
        run.stdout.contains("index_with contract passed"),
        "{}",
        run.stdout
    );
}
